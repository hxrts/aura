//! Guardian relationship types for recovery contexts
//!
//! This module defines the domain types used for guardian configuration
//! and relationships in cross-authority contexts.

use crate::time::{PhysicalTime, TimeDomain, TimeStamp};
use crate::Hash32;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Guardian binding between an account and guardian authority
///
/// This binding establishes a guardian relationship that allows
/// the guardian to participate in recovery operations for the account.
/// It is a pure domain type with no protocol logic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuardianBinding {
    /// Commitment hash of the account authority
    pub account_commitment: Hash32,
    /// Commitment hash of the guardian authority
    pub guardian_commitment: Hash32,
    /// Parameters governing this guardian relationship
    pub parameters: GuardianParameters,
    /// Optional consensus proof if binding required agreement
    pub consensus_proof: Option<super::consensus::ConsensusProof>,
}

impl GuardianBinding {
    /// Create a new guardian binding
    pub fn new(
        account_commitment: Hash32,
        guardian_commitment: Hash32,
        parameters: GuardianParameters,
    ) -> Self {
        Self {
            account_commitment,
            guardian_commitment,
            parameters,
            consensus_proof: None,
        }
    }

    /// Create a guardian binding with consensus proof
    #[must_use]
    pub fn with_consensus_proof(
        account_commitment: Hash32,
        guardian_commitment: Hash32,
        parameters: GuardianParameters,
        consensus_proof: super::consensus::ConsensusProof,
    ) -> Self {
        Self {
            account_commitment,
            guardian_commitment,
            parameters,
            consensus_proof: Some(consensus_proof),
        }
    }

    /// Check if this binding has consensus proof
    pub fn has_consensus_proof(&self) -> bool {
        self.consensus_proof.is_some()
    }

    /// Check if this binding has expired at the given time
    pub fn is_expired_at_ms(&self, now_ms: u64) -> bool {
        self.parameters.is_expired_at_ms(now_ms)
    }

    /// Check if this binding has expired at the given physical time
    pub fn is_expired_at_time(&self, now: &PhysicalTime) -> bool {
        self.parameters.is_expired_at_ms(now.ts_ms)
    }

    /// Get the recovery delay for this binding
    pub fn recovery_delay(&self) -> Duration {
        self.parameters.recovery_delay
    }

    /// Check if notification is required for recovery
    pub fn notification_required(&self) -> bool {
        self.parameters.notification_required
    }
}

/// Parameters for guardian relationships
///
/// These parameters define the operational constraints and policies
/// for a guardian relationship.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuardianParameters {
    /// Time delay required before recovery can be executed
    pub recovery_delay: Duration,
    /// Whether notification to the account is required
    pub notification_required: bool,
    /// Optional expiration time for this binding
    pub expiration: Option<TimeStamp>, // PhysicalClock only
}

impl Default for GuardianParameters {
    fn default() -> Self {
        Self {
            recovery_delay: Duration::from_secs(24 * 60 * 60), // 24 hours
            notification_required: true,
            expiration: None,
        }
    }
}

impl GuardianParameters {
    /// Create new guardian parameters with custom values
    pub fn new(
        recovery_delay: Duration,
        notification_required: bool,
        expiration: Option<TimeStamp>,
    ) -> Self {
        Self {
            recovery_delay,
            notification_required,
            expiration,
        }
    }

    /// Create guardian parameters with default security settings
    pub fn secure_defaults() -> Self {
        Self {
            recovery_delay: Duration::from_secs(72 * 60 * 60), // 72 hours for security
            notification_required: true,
            expiration: None,
        }
    }

    /// Create guardian parameters for emergency scenarios with 7 day expiration
    pub fn emergency() -> Self {
        Self {
            recovery_delay: Duration::from_secs(60 * 60), // 1 hour for emergencies
            notification_required: false,                 // Skip notification in emergencies
            expiration: Some(TimeStamp::PhysicalClock(PhysicalTime {
                ts_ms: 7 * 24 * 60 * 60 * 1000, // 7 days from epoch (this will be replaced in practice)
                uncertainty: None,
            })),
        }
    }

    /// Set expiration using a physical clock timestamp (milliseconds since UNIX epoch)
    #[must_use]
    pub fn with_expiration_ms(mut self, expiration_ms: u64) -> Self {
        self.expiration = Some(TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: expiration_ms,
            uncertainty: None,
        }));
        self
    }

    /// Check if these parameters are expired at the given time (ms)
    pub fn is_expired_at_ms(&self, now_ms: u64) -> bool {
        if let Some(TimeStamp::PhysicalClock(exp)) = &self.expiration {
            now_ms > exp.ts_ms
        } else {
            false
        }
    }

    /// Check if these are emergency parameters (short delay, no notification)
    pub fn is_emergency_config(&self) -> bool {
        self.recovery_delay.as_secs() <= 3600 && !self.notification_required
    }
}

impl GuardianParameters {
    fn expiration_ms(&self) -> u64 {
        match &self.expiration {
            Some(TimeStamp::PhysicalClock(p)) => p.ts_ms,
            Some(other) => {
                let index = other.to_index_ms();
                if index.domain() == TimeDomain::PhysicalClock {
                    index.value()
                } else {
                    0
                }
            }
            None => 0,
        }
    }
}

impl PartialOrd for GuardianParameters {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for GuardianParameters {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (
            self.recovery_delay,
            self.notification_required,
            self.expiration_ms(),
        )
            .cmp(&(
                other.recovery_delay,
                other.notification_required,
                other.expiration_ms(),
            ))
    }
}

impl PartialOrd for GuardianBinding {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for GuardianBinding {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (
            self.account_commitment,
            self.guardian_commitment,
            &self.parameters,
            &self.consensus_proof,
        )
            .cmp(&(
                other.account_commitment,
                other.guardian_commitment,
                &other.parameters,
                &other.consensus_proof,
            ))
    }
}

/// Builder for creating guardian bindings with fluent interface
pub struct GuardianBindingBuilder {
    account_commitment: Option<Hash32>,
    guardian_commitment: Option<Hash32>,
    parameters: GuardianParameters,
}

impl GuardianBindingBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self {
            account_commitment: None,
            guardian_commitment: None,
            parameters: GuardianParameters::default(),
        }
    }

    /// Set the account commitment
    pub fn account(mut self, commitment: Hash32) -> Self {
        self.account_commitment = Some(commitment);
        self
    }

    /// Set the guardian commitment
    pub fn guardian(mut self, commitment: Hash32) -> Self {
        self.guardian_commitment = Some(commitment);
        self
    }

    /// Set the recovery delay
    pub fn recovery_delay(mut self, delay: Duration) -> Self {
        self.parameters.recovery_delay = delay;
        self
    }

    /// Set notification requirement
    pub fn notification_required(mut self, required: bool) -> Self {
        self.parameters.notification_required = required;
        self
    }

    /// Set expiration time
    pub fn expires_at_ms(mut self, expiration_ms: u64) -> Self {
        self.parameters = self.parameters.with_expiration_ms(expiration_ms);
        self
    }

    /// Use secure default parameters (72 hour delay, notification required)
    pub fn secure_defaults(mut self) -> Self {
        self.parameters = GuardianParameters::secure_defaults();
        self
    }

    /// Use emergency parameters (1 hour delay, no notification, 7 day expiration)
    pub fn emergency_config(mut self) -> Self {
        self.parameters = GuardianParameters::emergency();
        self
    }

    /// Build the guardian binding
    pub fn build(self) -> Result<GuardianBinding, &'static str> {
        let account_commitment = self
            .account_commitment
            .ok_or("Account commitment required")?;
        let guardian_commitment = self
            .guardian_commitment
            .ok_or("Guardian commitment required")?;

        Ok(GuardianBinding::new(
            account_commitment,
            guardian_commitment,
            self.parameters,
        ))
    }
}

impl Default for GuardianBindingBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_guardian_binding_builder() {
        let account = Hash32::default();
        let guardian = Hash32([1u8; 32]);

        let binding = GuardianBindingBuilder::new()
            .account(account)
            .guardian(guardian)
            .recovery_delay(Duration::from_secs(3600))
            .notification_required(false)
            .build()
            .unwrap();

        assert_eq!(binding.account_commitment, account);
        assert_eq!(binding.guardian_commitment, guardian);
        assert_eq!(binding.parameters.recovery_delay, Duration::from_secs(3600));
        assert!(!binding.parameters.notification_required);
    }

    #[test]
    fn test_guardian_parameters_defaults() {
        let params = GuardianParameters::default();
        assert_eq!(params.recovery_delay, Duration::from_secs(24 * 60 * 60));
        assert!(params.notification_required);
        assert!(params.expiration.is_none());
    }

    #[test]
    fn test_guardian_parameters_emergency() {
        let params = GuardianParameters::emergency();
        assert!(params.is_emergency_config());
        assert!(params.expiration.is_some());
    }

    #[test]
    fn test_guardian_binding_expiration() {
        let account = Hash32::default();
        let guardian = Hash32([1u8; 32]);

        // Create binding that expires in the past
        let now_ms = 10_000_000u64; // Large enough to avoid underflow
        let past_ms = now_ms - 3_600_000; // one hour earlier
        let expired_params = GuardianParameters::new(
            Duration::from_secs(3600),
            true,
            Some(TimeStamp::PhysicalClock(PhysicalTime {
                ts_ms: past_ms,
                uncertainty: None,
            })),
        );

        let binding = GuardianBinding::new(account, guardian, expired_params);
        assert!(binding.is_expired_at_ms(now_ms));

        // Create binding that expires in the future
        let future_ms = now_ms + 3_600_000;
        let valid_params = GuardianParameters::new(
            Duration::from_secs(3600),
            true,
            Some(TimeStamp::PhysicalClock(PhysicalTime {
                ts_ms: future_ms,
                uncertainty: None,
            })),
        );

        let binding = GuardianBinding::new(account, guardian, valid_params);
        assert!(!binding.is_expired_at_ms(now_ms));
    }
}
