//! Reconnect behavior for SecureChannels with rendezvous integration
//!
//! This module implements the reconnection logic specified in work/007.md:
//! - Re-run rendezvous protocol on channel teardown
//! - Ensure receipts never cross epoch boundaries
//! - Implement exponential backoff and retry limits

use crate::secure_channel::{ChannelKey, SecureChannelRegistry, TeardownReason};
use aura_core::{flow::FlowBudget, session_epochs::Epoch, AuraError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::{Mutex, RwLock};
use tokio::time::sleep;
use tracing::{debug, error, info, warn};

/// Reconnect attempt information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconnectAttempt {
    /// Channel that needs reconnection
    pub channel_key: ChannelKey,
    /// Attempt number (1-based)
    pub attempt_number: u32,
    /// When this attempt should be executed
    pub scheduled_at: u64,
    /// Previous failure reason
    pub previous_failure: Option<TeardownReason>,
    /// Current epoch for this reconnection
    pub epoch: Epoch,
    /// Flow budget for the new channel
    pub flow_budget: FlowBudget,
}

/// Configuration for reconnection behavior
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconnectConfig {
    /// Maximum number of reconnect attempts before giving up
    pub max_attempts: u32,
    /// Base delay for exponential backoff in seconds
    pub base_delay_seconds: u64,
    /// Maximum delay between attempts in seconds
    pub max_delay_seconds: u64,
    /// Backoff multiplier (e.g., 2.0 for doubling)
    pub backoff_multiplier: f64,
    /// Whether to reset attempt count on successful connection
    pub reset_on_success: bool,
    /// Whether to require epoch advancement for reconnection
    pub require_epoch_advancement: bool,
}

impl Default for ReconnectConfig {
    fn default() -> Self {
        Self {
            max_attempts: 5,
            base_delay_seconds: 1,
            max_delay_seconds: 300, // 5 minutes
            backoff_multiplier: 2.0,
            reset_on_success: true,
            require_epoch_advancement: true,
        }
    }
}

/// Reconnection coordinator that manages retry logic and rendezvous integration
pub struct ReconnectCoordinator {
    /// Registry to coordinate with
    registry: Arc<SecureChannelRegistry>,
    /// Pending reconnect attempts
    pending_attempts: Arc<Mutex<Vec<ReconnectAttempt>>>,
    /// Failed channels that exceeded retry limit
    failed_channels: Arc<RwLock<HashMap<ChannelKey, u32>>>,
    /// Configuration for reconnect behavior
    config: ReconnectConfig,
    /// Current epoch for ensuring epoch boundaries
    current_epoch: Arc<RwLock<Epoch>>,
}

/// Result of a reconnection attempt
#[derive(Debug, Clone)]
pub enum ReconnectResult {
    /// Reconnection was successful
    Success {
        channel_key: ChannelKey,
        attempt_number: u32,
        new_peer_addr: SocketAddr,
    },
    /// Reconnection failed but will retry
    FailedWillRetry {
        channel_key: ChannelKey,
        attempt_number: u32,
        error: String,
        next_attempt_at: u64,
    },
    /// Reconnection failed and exceeded retry limit
    FailedExhausted {
        channel_key: ChannelKey,
        attempt_number: u32,
        error: String,
    },
    /// Reconnection skipped due to epoch boundary violation
    SkippedEpochViolation {
        channel_key: ChannelKey,
        current_epoch: Epoch,
        required_epoch: Epoch,
    },
}

/// Statistics for reconnection operations
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReconnectStats {
    /// Total reconnection attempts made
    pub total_attempts: u64,
    /// Successful reconnections
    pub successful_reconnects: u64,
    /// Failed reconnections (exhausted retries)
    pub failed_reconnects: u64,
    /// Reconnections skipped due to epoch violations
    pub skipped_epoch_violations: u64,
    /// Currently pending reconnection attempts
    pub pending_attempts: u64,
}

impl ReconnectCoordinator {
    /// Create a new reconnection coordinator
    pub fn new(
        registry: Arc<SecureChannelRegistry>,
        config: ReconnectConfig,
        initial_epoch: Epoch,
    ) -> Self {
        Self {
            registry,
            pending_attempts: Arc::new(Mutex::new(Vec::new())),
            failed_channels: Arc::new(RwLock::new(HashMap::new())),
            config,
            current_epoch: Arc::new(RwLock::new(initial_epoch)),
        }
    }

    /// Create with default configuration
    pub fn with_defaults(registry: Arc<SecureChannelRegistry>, initial_epoch: Epoch) -> Self {
        Self::new(registry, ReconnectConfig::default(), initial_epoch)
    }

    /// Schedule a reconnection attempt for a channel that was torn down
    pub async fn schedule_reconnect(
        &self,
        channel_key: ChannelKey,
        teardown_reason: TeardownReason,
        epoch: Epoch,
        flow_budget: FlowBudget,
    ) -> Result<(), AuraError> {
        let current_epoch = *self.current_epoch.read().await;

        // Ensure we don't violate epoch boundaries
        if self.config.require_epoch_advancement && epoch.value() <= current_epoch.value() {
            warn!(
                context = %channel_key.context.as_str(),
                peer = %channel_key.peer_device.0,
                current_epoch = current_epoch.value(),
                requested_epoch = epoch.value(),
                "Skipping reconnect - epoch not advanced"
            );
            return Ok(());
        }

        let failed_channels = self.failed_channels.read().await;
        let previous_attempts = failed_channels.get(&channel_key).copied().unwrap_or(0);
        drop(failed_channels);

        if previous_attempts >= self.config.max_attempts {
            warn!(
                context = %channel_key.context.as_str(),
                peer = %channel_key.peer_device.0,
                attempts = previous_attempts,
                max_attempts = self.config.max_attempts,
                "Channel exceeded maximum reconnect attempts"
            );
            return Ok(());
        }

        let attempt_number = previous_attempts + 1;
        let delay = self.calculate_backoff_delay(attempt_number);
        let scheduled_at = SystemTime::UNIX_EPOCH
            .elapsed()
            .unwrap_or_default()
            .as_secs()
            + delay;

        let attempt = ReconnectAttempt {
            channel_key: channel_key.clone(),
            attempt_number,
            scheduled_at,
            previous_failure: Some(teardown_reason),
            epoch,
            flow_budget,
        };

        let mut pending = self.pending_attempts.lock().await;
        pending.push(attempt);
        drop(pending);

        info!(
            context = %channel_key.context.as_str(),
            peer = %channel_key.peer_device.0,
            attempt = attempt_number,
            delay = delay,
            "Scheduled reconnection attempt"
        );

        Ok(())
    }

    /// Update the current epoch for epoch boundary enforcement
    pub async fn update_epoch(&self, new_epoch: Epoch) {
        let mut current = self.current_epoch.write().await;
        if new_epoch.value() > current.value() {
            *current = new_epoch;

            info!(
                old_epoch = current.value(),
                new_epoch = new_epoch.value(),
                "ReconnectCoordinator epoch advanced"
            );
        }
    }

    /// Process pending reconnection attempts
    pub async fn process_reconnections(&self) -> Result<Vec<ReconnectResult>, AuraError> {
        let current_time = SystemTime::UNIX_EPOCH
            .elapsed()
            .unwrap_or_default()
            .as_secs();

        let mut pending = self.pending_attempts.lock().await;
        let (ready, not_ready): (Vec<_>, Vec<_>) = pending
            .drain(..)
            .partition(|attempt| attempt.scheduled_at <= current_time);
        *pending = not_ready;
        drop(pending);

        if ready.is_empty() {
            return Ok(Vec::new());
        }

        let mut results = Vec::new();
        let current_epoch = *self.current_epoch.read().await;

        for attempt in ready {
            // Check epoch boundary before attempting reconnection
            if self.config.require_epoch_advancement
                && attempt.epoch.value() <= current_epoch.value()
            {
                results.push(ReconnectResult::SkippedEpochViolation {
                    channel_key: attempt.channel_key,
                    current_epoch,
                    required_epoch: attempt.epoch,
                });
                continue;
            }

            match self.attempt_reconnection(&attempt).await {
                Ok(success_result) => {
                    if self.config.reset_on_success {
                        let mut failed = self.failed_channels.write().await;
                        failed.remove(&attempt.channel_key);
                    }
                    results.push(success_result);
                }
                Err(error) => {
                    let error_msg = error.to_string();
                    let mut failed = self.failed_channels.write().await;
                    let new_attempt_count = attempt.attempt_number;
                    failed.insert(attempt.channel_key.clone(), new_attempt_count);
                    drop(failed);

                    if new_attempt_count >= self.config.max_attempts {
                        results.push(ReconnectResult::FailedExhausted {
                            channel_key: attempt.channel_key,
                            attempt_number: new_attempt_count,
                            error: error_msg,
                        });
                    } else {
                        // Schedule next attempt
                        let next_delay = self.calculate_backoff_delay(new_attempt_count + 1);
                        let next_attempt_at = current_time + next_delay;

                        let next_attempt = ReconnectAttempt {
                            channel_key: attempt.channel_key.clone(),
                            attempt_number: new_attempt_count + 1,
                            scheduled_at: next_attempt_at,
                            previous_failure: attempt.previous_failure,
                            epoch: attempt.epoch,
                            flow_budget: attempt.flow_budget,
                        };

                        let mut pending = self.pending_attempts.lock().await;
                        pending.push(next_attempt);

                        results.push(ReconnectResult::FailedWillRetry {
                            channel_key: attempt.channel_key,
                            attempt_number: new_attempt_count,
                            error: error_msg,
                            next_attempt_at,
                        });
                    }
                }
            }
        }

        Ok(results)
    }

    /// Calculate exponential backoff delay for an attempt
    fn calculate_backoff_delay(&self, attempt_number: u32) -> u64 {
        let delay = self.config.base_delay_seconds as f64
            * self
                .config
                .backoff_multiplier
                .powi((attempt_number - 1) as i32);

        (delay as u64).min(self.config.max_delay_seconds)
    }

    /// Attempt to reconnect a specific channel
    /// This is where rendezvous protocol would be re-executed
    async fn attempt_reconnection(
        &self,
        attempt: &ReconnectAttempt,
    ) -> Result<ReconnectResult, AuraError> {
        debug!(
            context = %attempt.channel_key.context.as_str(),
            peer = %attempt.channel_key.peer_device.0,
            attempt = attempt.attempt_number,
            "Attempting channel reconnection"
        );

        // Step 1: Re-run rendezvous protocol to discover peer transport offers
        // TODO: This would integrate with aura-rendezvous to:
        // 1. Query discovery service for peer's current transport descriptors
        // 2. Exchange transport offers via SBB
        // 3. Perform NAT traversal if needed
        // 4. Establish new encrypted connection

        // For now, simulate the reconnection process
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Step 2: Create or update channel in registry with new epoch and budget
        self.registry
            .get_or_create_channel(
                attempt.channel_key.context.clone(),
                attempt.channel_key.peer_device,
                attempt.epoch,
                attempt.flow_budget,
            )
            .await?;

        // Step 3: Simulate successful connection establishment
        // In real implementation, this would involve:
        // - TCP/QUIC connection establishment
        // - TLS handshake with device identity verification
        // - SecureChannel activation

        let mock_peer_addr: SocketAddr = "192.168.1.100:8080".parse().unwrap();

        Ok(ReconnectResult::Success {
            channel_key: attempt.channel_key.clone(),
            attempt_number: attempt.attempt_number,
            new_peer_addr: mock_peer_addr,
        })
    }

    /// Get statistics about reconnection operations
    pub async fn get_reconnect_stats(&self) -> ReconnectStats {
        let pending = self.pending_attempts.lock().await;
        let failed = self.failed_channels.read().await;

        ReconnectStats {
            total_attempts: failed.values().sum::<u32>() as u64,
            successful_reconnects: 0, // TODO: Track this
            failed_reconnects: failed
                .values()
                .filter(|&&count| count >= self.config.max_attempts)
                .count() as u64,
            skipped_epoch_violations: 0, // TODO: Track this
            pending_attempts: pending.len() as u64,
        }
    }

    /// Cancel pending reconnection attempts for a specific channel
    pub async fn cancel_reconnect(&self, channel_key: &ChannelKey) -> bool {
        let mut pending = self.pending_attempts.lock().await;
        let initial_len = pending.len();
        pending.retain(|attempt| &attempt.channel_key != channel_key);

        let cancelled = initial_len != pending.len();
        if cancelled {
            info!(
                context = %channel_key.context.as_str(),
                peer = %channel_key.peer_device.0,
                "Cancelled pending reconnection attempts"
            );
        }

        cancelled
    }

    /// Start the reconnection processing loop
    pub async fn start_processing_loop(&self, interval: Duration) -> Result<(), AuraError> {
        info!("Starting ReconnectCoordinator processing loop");

        loop {
            let results = self.process_reconnections().await?;

            for result in results {
                match result {
                    ReconnectResult::Success {
                        channel_key,
                        attempt_number,
                        new_peer_addr,
                    } => {
                        info!(
                            context = %channel_key.context.as_str(),
                            peer = %channel_key.peer_device.0,
                            attempt = attempt_number,
                            addr = %new_peer_addr,
                            "Channel reconnection successful"
                        );
                    }
                    ReconnectResult::FailedWillRetry {
                        channel_key,
                        attempt_number,
                        error,
                        ..
                    } => {
                        warn!(
                            context = %channel_key.context.as_str(),
                            peer = %channel_key.peer_device.0,
                            attempt = attempt_number,
                            error = %error,
                            "Channel reconnection failed, will retry"
                        );
                    }
                    ReconnectResult::FailedExhausted {
                        channel_key,
                        attempt_number,
                        error,
                    } => {
                        error!(
                            context = %channel_key.context.as_str(),
                            peer = %channel_key.peer_device.0,
                            attempt = attempt_number,
                            error = %error,
                            "Channel reconnection exhausted all retries"
                        );
                    }
                    ReconnectResult::SkippedEpochViolation {
                        channel_key,
                        current_epoch,
                        required_epoch,
                    } => {
                        debug!(
                            context = %channel_key.context.as_str(),
                            peer = %channel_key.peer_device.0,
                            current_epoch = current_epoch.value(),
                            required_epoch = required_epoch.value(),
                            "Reconnection skipped due to epoch boundary violation"
                        );
                    }
                }
            }

            sleep(interval).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secure_channel::RegistryConfig;
    use aura_core::{ContextId, DeviceId};
    use std::sync::Arc;

    #[tokio::test]
    async fn test_reconnect_coordinator_creation() {
        let registry = Arc::new(SecureChannelRegistry::new(RegistryConfig::default()));
        let coordinator = ReconnectCoordinator::with_defaults(registry, Epoch::new(1));

        let stats = coordinator.get_reconnect_stats().await;
        assert_eq!(stats.pending_attempts, 0);
        assert_eq!(stats.total_attempts, 0);
    }

    #[tokio::test]
    async fn test_schedule_reconnect() {
        let registry = Arc::new(SecureChannelRegistry::new(RegistryConfig::default()));
        let coordinator = ReconnectCoordinator::with_defaults(registry, Epoch::new(1));

        let context = ContextId::new("test_context");
        let peer = DeviceId(uuid::Uuid::new_v4());
        let channel_key = ChannelKey::new(context, peer);
        let epoch = Epoch::new(2);
        let budget = FlowBudget::new(1000, epoch);
        let reason = TeardownReason::EpochRotation {
            old_epoch: Epoch::new(1),
            new_epoch: epoch,
        };

        coordinator
            .schedule_reconnect(channel_key, reason, epoch, budget)
            .await
            .unwrap();

        let stats = coordinator.get_reconnect_stats().await;
        assert_eq!(stats.pending_attempts, 1);
    }

    #[tokio::test]
    async fn test_epoch_boundary_enforcement() {
        let registry = Arc::new(SecureChannelRegistry::new(RegistryConfig::default()));
        let coordinator = ReconnectCoordinator::with_defaults(registry, Epoch::new(5));

        let context = ContextId::new("test_context");
        let peer = DeviceId(uuid::Uuid::new_v4());
        let channel_key = ChannelKey::new(context, peer);
        let old_epoch = Epoch::new(3); // Earlier than current epoch
        let budget = FlowBudget::new(1000, old_epoch);
        let reason = TeardownReason::Manual;

        // This should not schedule reconnect due to epoch boundary violation
        coordinator
            .schedule_reconnect(channel_key, reason, old_epoch, budget)
            .await
            .unwrap();

        let stats = coordinator.get_reconnect_stats().await;
        assert_eq!(stats.pending_attempts, 0); // Should not be scheduled
    }

    #[tokio::test]
    async fn test_backoff_calculation() {
        let config = ReconnectConfig {
            base_delay_seconds: 2,
            backoff_multiplier: 2.0,
            max_delay_seconds: 60,
            ..Default::default()
        };

        let registry = Arc::new(SecureChannelRegistry::new(RegistryConfig::default()));
        let coordinator = ReconnectCoordinator::new(registry, config, Epoch::new(1));

        assert_eq!(coordinator.calculate_backoff_delay(1), 2); // 2 * 2^0 = 2
        assert_eq!(coordinator.calculate_backoff_delay(2), 4); // 2 * 2^1 = 4
        assert_eq!(coordinator.calculate_backoff_delay(3), 8); // 2 * 2^2 = 8
        assert_eq!(coordinator.calculate_backoff_delay(10), 60); // Capped at max_delay
    }
}
