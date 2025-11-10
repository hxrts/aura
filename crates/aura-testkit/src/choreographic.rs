//! Choreographic test utilities
//!
//! Test utilities for setting up Rumpsteak-based choreographic tests,
//! including mock handlers and test transports.

use rumpsteak::{ChoreoHandler, Located, Transporter, Transport};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, RwLock};
use async_trait::async_trait;

/// Mock choreographic handler for testing
pub struct MockChoreoHandler<R> {
    role: R,
    inbox: Arc<Mutex<mpsc::Receiver<(String, Vec<u8>)>>>,
    transports: Arc<RwLock<HashMap<String, mpsc::Sender<(String, Vec<u8>)>>>>,
}

impl<R> MockChoreoHandler<R> 
where
    R: Clone + Send + Sync + 'static,
{
    /// Create a new mock handler
    pub fn new(role: R) -> Self {
        let (tx, rx) = mpsc::channel(100);
        Self {
            role,
            inbox: Arc::new(Mutex::new(rx)),
            transports: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Add a peer transport
    pub async fn add_peer(&self, role: String, sender: mpsc::Sender<(String, Vec<u8>)>) {
        let mut transports = self.transports.write().await;
        transports.insert(role, sender);
    }

    /// Get the receiver for this handler
    pub fn receiver(&self) -> mpsc::Sender<(String, Vec<u8>)> {
        let (tx, _) = mpsc::channel(100);
        tx
    }
}

#[async_trait]
impl<R> ChoreoHandler<R> for MockChoreoHandler<R>
where
    R: Clone + Send + Sync + 'static,
{
    fn role(&self) -> &R {
        &self.role
    }

    async fn send<M, T>(&self, to: &T::Role, message: Located<M, T::Role>) 
    where
        M: serde::Serialize + Send + 'static,
        T: Transport<R, M> + ?Sized,
        T::Role: serde::Serialize + std::fmt::Display,
    {
        let transports = self.transports.read().await;
        let role_str = to.to_string();
        
        if let Some(sender) = transports.get(&role_str) {
            let msg_bytes = rmp_serde::to_vec(&message).unwrap();
            let _ = sender.send((role_str.clone(), msg_bytes)).await;
        }
    }

    async fn receive<M, T>(&self, from: &T::Role) -> Located<M, T::Role>
    where
        M: serde::de::DeserializeOwned + Send + 'static,
        T: Transport<R, M> + ?Sized,
        T::Role: serde::de::DeserializeOwned + std::fmt::Display + Send,
    {
        let mut inbox = self.inbox.lock().await;
        
        loop {
            if let Some((sender_role, msg_bytes)) = inbox.recv().await {
                if sender_role == from.to_string() {
                    let message: Located<M, T::Role> = rmp_serde::from_slice(&msg_bytes).unwrap();
                    return message;
                }
            }
        }
    }

    async fn multicast<M, T>(&self, to: &[T::Role], message: Located<M, T::Role>)
    where
        M: serde::Serialize + Send + Clone + 'static,
        T: Transport<R, M> + ?Sized,
        T::Role: serde::Serialize + std::fmt::Display,
    {
        let transports = self.transports.read().await;
        let msg_bytes = rmp_serde::to_vec(&message).unwrap();
        
        for role in to {
            let role_str = role.to_string();
            if let Some(sender) = transports.get(&role_str) {
                let _ = sender.send((self.role().to_string(), msg_bytes.clone())).await;
            }
        }
    }
}

/// Create a test choreographic network
///
/// Sets up mock handlers for testing choreographic protocols.
pub async fn test_choreographic_network<R>(roles: Vec<R>) -> HashMap<R, MockChoreoHandler<R>>
where
    R: Clone + Send + Sync + 'static + Eq + std::hash::Hash + std::fmt::Display,
{
    let mut handlers = HashMap::new();
    let mut senders = HashMap::new();

    // Create handlers and collect senders
    for role in &roles {
        let handler = MockChoreoHandler::new(role.clone());
        let sender = handler.receiver();
        senders.insert(role.to_string(), sender);
        handlers.insert(role.clone(), handler);
    }

    // Connect all handlers
    for (role, handler) in &handlers {
        for (other_role, sender) in &senders {
            if role.to_string() != *other_role {
                handler.add_peer(other_role.clone(), sender.clone()).await;
            }
        }
    }

    handlers
}

/// Test transport implementation for choreographies
pub struct TestTransport;

impl<R, M> Transport<R, M> for TestTransport 
where
    R: Send + Sync,
    M: Send + Sync,
{
    type Role = String;
}

impl<R, M> Transporter<R, M> for TestTransport 
where
    R: Send + Sync,
    M: Send + Sync,
{
    type Transport = TestTransport;
}

/// Create test choreographic effects
pub fn test_choreographic_effects() -> crate::Effects {
    crate::test_effects_deterministic(42, 1000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_choreo_handler() {
        let roles = vec!["Alice".to_string(), "Bob".to_string()];
        let handlers = test_choreographic_network(roles).await;
        
        assert_eq!(handlers.len(), 2);
        assert!(handlers.contains_key("Alice"));
        assert!(handlers.contains_key("Bob"));
    }
}