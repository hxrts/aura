//! Guardian relationship types for recovery contexts
//!
//! This module defines the domain types used for guardian configuration
//! and relationships in cross-authority contexts.

use crate::Hash32;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Guardian binding between an account and guardian authority
///
/// This binding establishes a guardian relationship that allows
/// the guardian to participate in recovery operations for the account.
/// It is a pure domain type with no protocol logic.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
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
    pub fn is_expired_at(&self, now: chrono::DateTime<chrono::Utc>) -> bool {
        if let Some(expiration) = self.parameters.expiration {
            now > expiration
        } else {
            false
        }
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
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct GuardianParameters {
    /// Time delay required before recovery can be executed
    pub recovery_delay: Duration,
    /// Whether notification to the account is required
    pub notification_required: bool,
    /// Optional expiration time for this binding
    pub expiration: Option<chrono::DateTime<chrono::Utc>>,
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
        expiration: Option<chrono::DateTime<chrono::Utc>>,
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

    /// Create guardian parameters for emergency scenarios with no expiration
    pub fn emergency() -> Self {
        Self {
            recovery_delay: Duration::from_secs(60 * 60), // 1 hour for emergencies
            notification_required: false,                 // Skip notification in emergencies
            expiration: None,                             // Emergency parameters don't expire
        }
    }

    /// Check if these parameters are expired at the given time
    pub fn is_expired_at(&self, now: chrono::DateTime<chrono::Utc>) -> bool {
        if let Some(expiration) = self.expiration {
            now > expiration
        } else {
            false
        }
    }

    /// Check if these are emergency parameters (short delay, no notification)
    pub fn is_emergency_config(&self) -> bool {
        self.recovery_delay.as_secs() <= 3600 && !self.notification_required
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
    pub fn expires_at(mut self, expiration: chrono::DateTime<chrono::Utc>) -> Self {
        self.parameters.expiration = Some(expiration);
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
        let past_time = chrono::Utc::now() - chrono::Duration::hours(1);
        let expired_params =
            GuardianParameters::new(Duration::from_secs(3600), true, Some(past_time));

        let binding = GuardianBinding::new(account, guardian, expired_params);
        let now = chrono::Utc::now();
        assert!(binding.is_expired_at(now));

        // Create binding that expires in the future
        let future_time = chrono::Utc::now() + chrono::Duration::hours(1);
        let valid_params =
            GuardianParameters::new(Duration::from_secs(3600), true, Some(future_time));

        let binding = GuardianBinding::new(account, guardian, valid_params);
        assert!(!binding.is_expired_at(now));
    }
}
