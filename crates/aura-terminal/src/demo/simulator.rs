//! # Demo Simulator
//!
//! Coordinates simulated peer agents (Alice, Charlie) for demo mode.
//!
//! The simulator runs alongside the TUI, processing events for the simulated
//! agents and routing their responses back to the TUI.

use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;
use tokio::time::{interval, Duration};

use aura_core::identifiers::AuthorityId;

use super::{AgentFactory, AgentResponse, SimulatedAgent, SimulatedBridge};

/// Demo simulator that manages Alice and Charlie peer agents
pub struct DemoSimulator {
    /// Simulation seed for determinism
    seed: u64,

    /// Alice agent
    alice: Arc<Mutex<SimulatedAgent>>,

    /// Charlie agent
    charlie: Arc<Mutex<SimulatedAgent>>,

    /// Bridge for routing events between TUI and agents
    bridge: Arc<SimulatedBridge>,

    /// Response sender for agents (used by Alice/Charlie to send responses)
    #[allow(dead_code)]
    response_tx: mpsc::UnboundedSender<(AuthorityId, AgentResponse)>,

    /// Background event loop handle
    event_loop_handle: Option<JoinHandle<()>>,

    /// Shutdown signal
    shutdown_tx: Option<mpsc::Sender<()>>,
}

impl DemoSimulator {
    /// Create a new demo simulator with the given seed
    pub async fn new(seed: u64) -> anyhow::Result<Self> {
        // Create Alice and Charlie agents
        let (alice, charlie) = AgentFactory::create_demo_agents(seed).await?;

        // Get Bob's authority ID - MUST match how TUI derives it in handle_tui_launch()
        // The TUI uses device_id_str = "demo:bob" and computes:
        //   authority_entropy = aura_core::hash::hash("authority:demo:bob")
        //   authority_id = AuthorityId::new_from_entropy(authority_entropy)
        // We need to use the SAME derivation here.
        let bob_device_id_str = "demo:bob";
        let bob_authority_entropy =
            aura_core::hash::hash(format!("authority:{}", bob_device_id_str).as_bytes());
        let bob_authority =
            aura_core::identifiers::AuthorityId::new_from_entropy(bob_authority_entropy);

        // Get Bob's context ID for guardian bindings (same derivation as TUI)
        let bob_context_entropy =
            aura_core::hash::hash(format!("context:{}", bob_device_id_str).as_bytes());
        let bob_context =
            aura_core::identifiers::ContextId::new_from_entropy(bob_context_entropy);

        // Create the bridge that routes events
        let (bridge, response_tx) = SimulatedBridge::new(bob_authority, None);

        // Set up response channels for agents
        let mut alice = alice;
        let mut charlie = charlie;
        alice.set_response_channel(response_tx.clone());
        charlie.set_response_channel(response_tx.clone());

        // Configure Alice and Charlie as guardians for Bob
        // This is the "pre-setup" state for the demo - Alice and Charlie
        // are already Bob's guardians when the demo starts
        alice.add_guardian_for(bob_authority, bob_context);
        charlie.add_guardian_for(bob_authority, bob_context);

        Ok(Self {
            seed,
            alice: Arc::new(Mutex::new(alice)),
            charlie: Arc::new(Mutex::new(charlie)),
            bridge: Arc::new(bridge),
            response_tx,
            event_loop_handle: None,
            shutdown_tx: None,
        })
    }

    /// Get the simulation seed
    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// Get Alice's authority ID
    pub async fn alice_authority(&self) -> AuthorityId {
        self.alice.lock().await.authority_id()
    }

    /// Get Charlie's authority ID
    pub async fn charlie_authority(&self) -> AuthorityId {
        self.charlie.lock().await.authority_id()
    }

    /// Get the bridge for connecting to the TUI
    pub fn bridge(&self) -> Arc<SimulatedBridge> {
        self.bridge.clone()
    }

    /// Start the simulated agents and event loop
    pub async fn start(&mut self) -> anyhow::Result<()> {
        // Start both agents
        self.alice.lock().await.start().await?;
        self.charlie.lock().await.start().await?;

        // Create shutdown channel
        let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);
        self.shutdown_tx = Some(shutdown_tx);

        // Subscribe to agent events from the bridge
        let mut event_rx = self.bridge.subscribe_agent_events();

        // Clone references for the event loop
        let alice = self.alice.clone();
        let charlie = self.charlie.clone();
        let bridge = self.bridge.clone();

        // Spawn event processing loop
        let handle = tokio::spawn(async move {
            // Interval for processing responses (check every 100ms)
            let mut response_interval = interval(Duration::from_millis(100));

            loop {
                tokio::select! {
                    // Check for shutdown signal
                    _ = shutdown_rx.recv() => {
                        tracing::info!("Demo simulator shutting down");
                        break;
                    }

                    // Process incoming events from TUI
                    event = event_rx.recv() => {
                        match event {
                            Ok(agent_event) => {
                                // Route event to both agents
                                let mut alice_guard = alice.lock().await;
                                let mut charlie_guard = charlie.lock().await;

                                // Process event in Alice
                                if let Ok(responses) = alice_guard.process_event(&agent_event).await {
                                    for response in responses {
                                        tracing::debug!("Alice response: {:?}", response);
                                    }
                                }

                                // Process event in Charlie
                                if let Ok(responses) = charlie_guard.process_event(&agent_event).await {
                                    for response in responses {
                                        tracing::debug!("Charlie response: {:?}", response);
                                    }
                                }

                                // Immediately process any responses generated
                                bridge.process_responses().await;
                            }
                            Err(e) => {
                                tracing::warn!("Event receive error: {}", e);
                            }
                        }
                    }

                    // Periodically process responses (for async responses)
                    _ = response_interval.tick() => {
                        bridge.process_responses().await;
                    }
                }
            }
        });

        self.event_loop_handle = Some(handle);

        tracing::info!(
            "Demo simulator started with seed {} - Alice and Charlie are online",
            self.seed
        );

        Ok(())
    }

    /// Stop the simulator
    pub async fn stop(&mut self) -> anyhow::Result<()> {
        // Send shutdown signal
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(()).await;
        }

        // Wait for event loop to finish
        if let Some(handle) = self.event_loop_handle.take() {
            let _ = handle.await;
        }

        // Stop agents
        self.alice.lock().await.stop().await?;
        self.charlie.lock().await.stop().await?;

        tracing::info!("Demo simulator stopped");
        Ok(())
    }

    /// Get statistics for all agents
    pub async fn get_statistics(&self) -> Vec<super::AgentStatistics> {
        vec![
            self.alice.lock().await.get_statistics(),
            self.charlie.lock().await.get_statistics(),
        ]
    }

    /// Get the number of connected peers (always 2 for demo: Alice + Charlie)
    pub fn peer_count(&self) -> usize {
        2
    }
}

impl Drop for DemoSimulator {
    fn drop(&mut self) {
        // Best effort cleanup - can't await in drop
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.try_send(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_demo_simulator_creation() {
        let simulator = DemoSimulator::new(2024).await.unwrap();
        assert_eq!(simulator.seed(), 2024);
        assert_eq!(simulator.peer_count(), 2);
    }

    #[tokio::test]
    async fn test_demo_simulator_start_stop() {
        let mut simulator = DemoSimulator::new(2024).await.unwrap();
        simulator.start().await.unwrap();

        // Get statistics
        let stats = simulator.get_statistics().await;
        assert_eq!(stats.len(), 2);
        assert!(stats.iter().any(|s| s.name == "Alice"));
        assert!(stats.iter().any(|s| s.name == "Charlie"));

        simulator.stop().await.unwrap();
    }
}
