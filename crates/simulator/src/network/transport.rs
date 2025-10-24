//! Simulated transport for testing
//!
//! This module provides a `SimulatedTransport` that implements the `Transport` trait
//! and integrates with the simulation engine's network fabric.

use async_trait::async_trait;
use aura_transport::{
    BroadcastResult, Connection, ConnectionBuilder, PresenceTicket, Result, 
    Transport as AuraTransport,
};
use aura_coordination::Transport as CoordinationTransport;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;

use crate::{DeliverySemantics, Effect, Envelope, ParticipantId};

/// Simulated transport for testing
///
/// This transport integrates with the simulation engine's network fabric,
/// routing messages through the simulated network with controlled latency
/// and failure injection.
pub struct SimulatedTransport {
    /// This participant's ID
    participant_id: ParticipantId,

    /// Active connections (peer_id -> connection)
    connections: Arc<RwLock<HashMap<String, Connection>>>,

    /// Pending received messages (connection_id -> message queue)
    inbox: Arc<Mutex<HashMap<String, Vec<Vec<u8>>>>>,

    /// Callback to emit effects to the simulation runtime
    effect_emitter: Arc<dyn Fn(Effect) + Send + Sync>,
}

impl SimulatedTransport {
    /// Create a new simulated transport
    pub fn new<F>(participant_id: ParticipantId, effect_emitter: F) -> Self
    where
        F: Fn(Effect) + Send + Sync + 'static,
    {
        SimulatedTransport {
            participant_id,
            connections: Arc::new(RwLock::new(HashMap::new())),
            inbox: Arc::new(Mutex::new(HashMap::new())),
            effect_emitter: Arc::new(effect_emitter),
        }
    }

    /// Deliver a message to this transport's inbox
    ///
    /// This is called by the simulation runtime when a message arrives.
    pub async fn deliver_message(&self, conn_id: &str, message: Vec<u8>) {
        let mut inbox = self.inbox.lock().await;
        inbox
            .entry(conn_id.to_string())
            .or_insert_with(Vec::new)
            .push(message);
    }
}

#[async_trait]
impl AuraTransport for SimulatedTransport {
    async fn connect(
        &self,
        peer_id: &str,
        _my_ticket: &PresenceTicket,
        _peer_ticket: &PresenceTicket,
    ) -> Result<Connection> {
        // In simulation, we skip ticket verification
        // Use ConnectionBuilder to create a connection
        let connection = ConnectionBuilder::new(peer_id).build();

        // Store connection
        let mut connections = self.connections.write().await;
        connections.insert(peer_id.to_string(), connection.clone());

        // Initialize inbox for this connection
        let mut inbox = self.inbox.lock().await;
        inbox.insert(connection.id().to_string(), Vec::new());

        Ok(connection)
    }

    async fn send(&self, conn: &Connection, message: &[u8]) -> Result<()> {
        // Convert peer_id string to ParticipantId
        // In simulation, we use deterministic UUIDs based on peer_id
        let recipient = ParticipantId::from_name(conn.peer_id());

        // Create envelope
        let envelope = Envelope {
            message_id: Uuid::new_v4(),
            sender: self.participant_id,
            recipients: vec![recipient],
            payload: message.to_vec(),
            delivery: DeliverySemantics::ReliableUnicast,
        };

        // Emit send effect
        (self.effect_emitter)(Effect::Send(envelope));

        Ok(())
    }

    async fn receive(&self, conn: &Connection, _timeout: Duration) -> Result<Option<Vec<u8>>> {
        // In simulation, we don't use actual timeouts
        // Instead, we check the inbox and return immediately
        let mut inbox = self.inbox.lock().await;

        if let Some(messages) = inbox.get_mut(conn.id()) {
            if let Some(message) = messages.first() {
                let message = message.clone();
                messages.remove(0);
                return Ok(Some(message));
            }
        }

        // No message available
        Ok(None)
    }

    async fn broadcast(
        &self,
        connections: &[Connection],
        message: &[u8],
    ) -> Result<BroadcastResult> {
        let mut succeeded = Vec::new();
        let mut failed = Vec::new();

        for conn in connections {
            match self.send(conn, message).await {
                Ok(_) => succeeded.push(conn.peer_id().to_string()),
                Err(_) => failed.push(conn.peer_id().to_string()),
            }
        }

        Ok(BroadcastResult { succeeded, failed })
    }

    async fn disconnect(&self, conn: &Connection) -> Result<()> {
        let mut connections = self.connections.write().await;
        connections.remove(conn.peer_id());

        let mut inbox = self.inbox.lock().await;
        inbox.remove(conn.id());

        Ok(())
    }

    async fn is_connected(&self, conn: &Connection) -> bool {
        let connections = self.connections.read().await;
        connections.contains_key(conn.peer_id())
    }
}

/// Implementation of aura_coordination::Transport for simulator compatibility
#[async_trait]
impl CoordinationTransport for SimulatedTransport {
    async fn send_message(&self, peer_id: &str, message: &[u8]) -> std::result::Result<(), String> {
        // Convert to aura_transport format by finding the connection
        let connections = self.connections.read().await;
        if let Some(conn) = connections.get(peer_id) {
            self.send(conn, message).await
                .map_err(|e| format!("Transport error: {:?}", e))
        } else {
            Err(format!("Peer {} not connected", peer_id))
        }
    }
    
    async fn broadcast_message(&self, message: &[u8]) -> std::result::Result<(), String> {
        let connections = self.connections.read().await;
        let conn_list: Vec<Connection> = connections.values().cloned().collect();
        drop(connections);
        
        self.broadcast(&conn_list, message).await
            .map(|_| ()) // Ignore broadcast result details for coordination layer
            .map_err(|e| format!("Broadcast error: {:?}", e))
    }
    
    async fn is_peer_reachable(&self, peer_id: &str) -> bool {
        let connections = self.connections.read().await;
        connections.contains_key(peer_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[tokio::test]
    async fn test_simulated_transport_connect() {
        let participant = ParticipantId::from_name("alice");
        let effects = Arc::new(Mutex::new(Vec::new()));
        let effects_clone = effects.clone();

        let transport = SimulatedTransport::new(participant, move |effect| {
            let effects = effects_clone.clone();
            tokio::spawn(async move {
                effects.lock().await.push(effect);
            });
        });

        // Create dummy tickets
        let my_ticket = PresenceTicket::new(Uuid::new_v4(), Uuid::new_v4(), 0, 3600).unwrap();

        let peer_ticket = my_ticket.clone();

        let conn = transport
            .connect("bob", &my_ticket, &peer_ticket)
            .await
            .unwrap();

        assert_eq!(conn.peer_id(), "bob");
        assert!(transport.is_connected(&conn).await);
    }

    #[tokio::test]
    async fn test_simulated_transport_send() {
        let participant = ParticipantId::from_name("alice");
        let effects = Arc::new(Mutex::new(Vec::new()));
        let effects_clone = effects.clone();

        let transport = SimulatedTransport::new(participant, move |effect| {
            let effects = effects_clone.clone();
            tokio::spawn(async move {
                effects.lock().await.push(effect);
            });
        });

        let my_ticket = PresenceTicket::new(Uuid::new_v4(), Uuid::new_v4(), 0, 3600).unwrap();

        let conn = transport
            .connect("bob", &my_ticket, &my_ticket)
            .await
            .unwrap();

        transport.send(&conn, b"hello").await.unwrap();

        // Give async task a moment to execute
        tokio::time::sleep(Duration::from_millis(10)).await;

        let effects = effects.lock().await;
        assert_eq!(effects.len(), 1);

        match &effects[0] {
            Effect::Send(envelope) => {
                assert_eq!(envelope.payload, b"hello");
                assert_eq!(envelope.sender, participant);
            }
            _ => panic!("Expected Send effect"),
        }
    }

    #[tokio::test]
    async fn test_simulated_transport_receive() {
        let participant = ParticipantId::from_name("alice");
        let transport = SimulatedTransport::new(participant, |_| {});

        let my_ticket = PresenceTicket::new(Uuid::new_v4(), Uuid::new_v4(), 0, 3600).unwrap();

        let conn = transport
            .connect("bob", &my_ticket, &my_ticket)
            .await
            .unwrap();

        // Deliver a message
        transport
            .deliver_message(conn.id(), b"hello".to_vec())
            .await;

        // Receive it
        let message = transport
            .receive(&conn, Duration::from_secs(1))
            .await
            .unwrap();
        assert_eq!(message, Some(b"hello".to_vec()));

        // No more messages
        let message = transport
            .receive(&conn, Duration::from_secs(1))
            .await
            .unwrap();
        assert_eq!(message, None);
    }
}
