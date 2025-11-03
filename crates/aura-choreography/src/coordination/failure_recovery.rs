//! Coordinator failure recovery choreography

use super::{CoordinatorMonitor, EpochBumpChoreography};
use aura_protocol::effects::choreographic::ChoreographicRole;
use crate::patterns::lottery::DecentralizedLottery;
use rumpsteak_choreography::{ChoreoHandler, ChoreographyError};
use std::time::Duration;

/// Complete coordinator failure recovery protocol
pub struct CoordinatorFailureRecovery {
    participants: Vec<ChoreographicRole>,
    heartbeat_interval: Duration,
    timeout_threshold: Duration,
}

impl CoordinatorFailureRecovery {
    pub fn new(
        participants: Vec<ChoreographicRole>,
        heartbeat_interval: Duration,
        timeout_threshold: Duration,
    ) -> Self {
        Self {
            participants,
            heartbeat_interval,
            timeout_threshold,
        }
    }

    /// Execute failure detection and recovery
    pub async fn recover_from_failure<H: ChoreoHandler<Role = ChoreographicRole>>(
        &self,
        handler: &mut H,
        endpoint: &mut H::Endpoint,
        my_role: ChoreographicRole,
        current_coordinator: ChoreographicRole,
    ) -> Result<ChoreographicRole, ChoreographyError> {
        // Step 1: Monitor coordinator heartbeat
        let monitor = CoordinatorMonitor::new(
            self.participants.clone(),
            self.heartbeat_interval,
            self.timeout_threshold,
        );

        let is_alive = monitor
            .monitor_coordinator(handler, endpoint, my_role, current_coordinator)
            .await?;

        if is_alive {
            // Coordinator is still alive
            return Ok(current_coordinator);
        }

        // Step 2: If timeout detected, verify with other participants
        if my_role != current_coordinator {
            let consensus = monitor
                .report_coordinator_failure(handler, endpoint, my_role, current_coordinator)
                .await?;

            if !consensus {
                // Not enough participants agree on failure
                return Ok(current_coordinator);
            }
        }

        // Step 3: Bump session epoch to invalidate stale state
        let epoch_bumper = EpochBumpChoreography::new(
            self.participants.clone(),
            self.participants.len().div_ceil(2), // Require majority
        );

        // For simplicity, assume current epoch is 1 - in production this would come from BaseContext
        let current_epoch = 1;
        let _new_epoch = epoch_bumper
            .bump_epoch(
                handler,
                endpoint,
                my_role,
                current_epoch,
                format!("Coordinator {} failed", current_coordinator.role_index),
            )
            .await?;

        // Step 4: Re-run lottery with fresh randomness
        let lottery = DecentralizedLottery::new(
            self.participants
                .iter()
                .filter(|p| **p != current_coordinator)
                .cloned()
                .collect(),
        );

        let new_coordinator = lottery.execute(handler, endpoint, my_role).await?;

        Ok(new_coordinator)
    }

    /// Run a protocol with automatic failure recovery
    pub async fn run_with_recovery<H, F, T>(
        &self,
        handler: &mut H,
        endpoint: &mut H::Endpoint,
        my_role: ChoreographicRole,
        initial_coordinator: ChoreographicRole,
        protocol: F,
    ) -> Result<(T, ChoreographicRole), ChoreographyError>
    where
        H: ChoreoHandler<Role = ChoreographicRole>,
        F: Fn(
            ChoreographicRole,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<T, ChoreographyError>> + Send>,
        >,
    {
        let mut current_coordinator = initial_coordinator;
        let max_retries = 3;

        for _retry in 0..max_retries {
            // Try to run the protocol
            match protocol(current_coordinator).await {
                Ok(result) => return Ok((result, current_coordinator)),
                Err(ChoreographyError::Timeout(_)) => {
                    // Coordinator may have failed, try recovery
                    match self
                        .recover_from_failure(handler, endpoint, my_role, current_coordinator)
                        .await
                    {
                        Ok(new_coordinator) => {
                            current_coordinator = new_coordinator;
                            continue;
                        }
                        Err(e) => return Err(e),
                    }
                }
                Err(e) => return Err(e),
            }
        }

        Err(ChoreographyError::ProtocolViolation(
            "Max retries exceeded in failure recovery".to_string(),
        ))
    }
}
