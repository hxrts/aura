//! HTTPS relay transport implementation for relayed communication

use crate::{
    core::traits::Transport, error::TransportErrorBuilder, TransportEnvelope, TransportResult,
};
use async_trait::async_trait;
use aura_types::DeviceId;
use std::{sync::Arc, time::Duration};
use tokio::{
    sync::{mpsc, Mutex},
    time::{interval, timeout, Interval},
};

/// HTTPS relay transport for relayed communication through a central server
pub struct HttpsRelayTransport {
    device_id: DeviceId,
    relay_url: String,
    client: reqwest::Client,
    receiver: Arc<Mutex<mpsc::UnboundedReceiver<TransportEnvelope>>>,
    sender: mpsc::UnboundedSender<TransportEnvelope>,
    poll_interval: Arc<Mutex<Option<Interval>>>,
    running: Arc<Mutex<bool>>,
}

impl HttpsRelayTransport {
    /// Create a new HTTPS relay transport
    pub fn new(device_id: DeviceId, relay_url: String) -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        Self {
            device_id,
            relay_url,
            client: reqwest::Client::new(),
            receiver: Arc::new(Mutex::new(receiver)),
            sender,
            poll_interval: Arc::new(Mutex::new(None)),
            running: Arc::new(Mutex::new(false)),
        }
    }

    /// Start polling for messages from the relay server
    async fn start_polling(&self) -> TransportResult<()> {
        let poll_timer = interval(Duration::from_millis(1000)); // Poll every second

        // Store the interval for cleanup
        *self.poll_interval.lock().await = Some(poll_timer);

        let device_id = self.device_id;
        let relay_url = self.relay_url.clone();
        let client = self.client.clone();
        let sender = self.sender.clone();
        let running = self.running.clone();

        tokio::spawn(async move {
            loop {
                {
                    let running_guard = running.lock().await;
                    if !*running_guard {
                        break;
                    }
                }

                // Poll for messages
                match Self::poll_messages(&client, &relay_url, device_id).await {
                    Ok(envelopes) => {
                        for envelope in envelopes {
                            if sender.send(envelope).is_err() {
                                tracing::warn!("Failed to send polled message to internal queue");
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Failed to poll messages from relay: {}", e);
                    }
                }

                // Wait for next poll interval
                if let Ok(_timer_guard) =
                    tokio::time::timeout(Duration::from_millis(100), async { Some(()) }).await
                {
                    tokio::time::sleep(Duration::from_millis(1000)).await;
                } else {
                    break;
                }
            }
        });

        Ok(())
    }

    /// Poll for messages from the relay server
    async fn poll_messages(
        client: &reqwest::Client,
        relay_url: &str,
        device_id: DeviceId,
    ) -> TransportResult<Vec<TransportEnvelope>> {
        let poll_url = format!("{}/poll/{}", relay_url, device_id);

        let response = client.get(&poll_url).send().await.map_err(|e| {
            TransportErrorBuilder::transport(format!("Failed to poll relay server: {}", e))
        })?;

        if response.status().is_success() {
            let envelopes: Vec<TransportEnvelope> = response.json().await.map_err(|e| {
                TransportErrorBuilder::protocol_error(format!(
                    "Failed to parse relay response: {}",
                    e
                ))
            })?;
            Ok(envelopes)
        } else {
            tracing::debug!(
                "No messages available from relay (status: {})",
                response.status()
            );
            Ok(Vec::new())
        }
    }

    /// Send message to relay server
    async fn send_to_relay(&self, envelope: TransportEnvelope) -> TransportResult<()> {
        let send_url = format!("{}/send", self.relay_url);

        let response = self
            .client
            .post(&send_url)
            .json(&envelope)
            .send()
            .await
            .map_err(|e| {
                TransportErrorBuilder::transport(format!("Failed to send to relay server: {}", e))
            })?;

        if response.status().is_success() {
            tracing::debug!("Message sent to relay for device {}", envelope.to);
            Ok(())
        } else {
            Err(TransportErrorBuilder::protocol_error(format!(
                "Relay server rejected message: {}",
                response.status()
            )))
        }
    }
}

#[async_trait]
impl Transport for HttpsRelayTransport {
    async fn send(&self, envelope: TransportEnvelope) -> TransportResult<()> {
        self.send_to_relay(envelope).await
    }

    async fn receive(
        &self,
        timeout_duration: Duration,
    ) -> TransportResult<Option<TransportEnvelope>> {
        let mut receiver = self.receiver.lock().await;

        match timeout(timeout_duration, receiver.recv()).await {
            Ok(Some(envelope)) => {
                tracing::debug!("HTTPS relay message received from device {}", envelope.from);
                Ok(Some(envelope))
            }
            Ok(None) => Ok(None), // Channel closed
            Err(_) => Ok(None),   // Timeout
        }
    }

    async fn connect(&self, _peer_id: DeviceId) -> TransportResult<()> {
        // For HTTPS relay, connection is implicit through the relay server
        // We just need to register our presence if needed
        let register_url = format!("{}/register/{}", self.relay_url, self.device_id);

        let response = self.client.post(&register_url).send().await.map_err(|e| {
            TransportErrorBuilder::connection(format!("Failed to register with relay: {}", e))
        })?;

        if response.status().is_success() {
            tracing::info!("Registered with HTTPS relay for device {}", self.device_id);
            Ok(())
        } else {
            Err(TransportErrorBuilder::connection(format!(
                "Failed to register with relay: {}",
                response.status()
            )))
        }
    }

    async fn disconnect(&self, _peer_id: DeviceId) -> TransportResult<()> {
        // For HTTPS relay, we can't really disconnect from specific peers
        // since communication is mediated by the relay server
        tracing::debug!("HTTPS relay disconnect request (no-op)");
        Ok(())
    }

    async fn is_reachable(&self, _peer_id: DeviceId) -> bool {
        // For HTTPS relay, we assume peers are reachable through the relay
        // In a real implementation, we might query the relay server for peer status
        true
    }

    async fn start(&mut self) -> TransportResult<()> {
        let mut running = self.running.lock().await;
        if *running {
            return Ok(());
        }

        self.start_polling().await?;
        *running = true;

        tracing::info!(
            "HTTPS relay transport started for device {}",
            self.device_id
        );
        Ok(())
    }

    async fn stop(&mut self) -> TransportResult<()> {
        let mut running = self.running.lock().await;
        if !*running {
            return Ok(());
        }

        *self.poll_interval.lock().await = None;
        *running = false;

        tracing::info!(
            "HTTPS relay transport stopped for device {}",
            self.device_id
        );
        Ok(())
    }

    fn transport_type(&self) -> &'static str {
        "https_relay"
    }
}
