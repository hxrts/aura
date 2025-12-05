//! Guardian relationship effect trait
//!
//! Application-level effects for creating and managing guardian bindings
//! via relational contexts. Implementations live in higher layers (protocol
//! or feature crates) and must use consensus-backed GuardianBinding facts.

use crate::epochs::Epoch;
use crate::frost::{PublicKeyPackage, Share};
use crate::relational::{GuardianBinding, GuardianParameters};
use crate::time::PhysicalTime;
use crate::{AuthorityId, ContextId, Hash32, Result};
use async_trait::async_trait;
use std::collections::HashMap;

/// Input for requesting or cancelling a guardian relationship
#[derive(Debug, Clone)]
pub struct GuardianRequestInput {
    /// Relational context where the request is recorded
    pub context: ContextId,
    /// Account authority to be protected
    pub account: AuthorityId,
    /// Prospective guardian authority
    pub guardian: AuthorityId,
    /// Commitment of the account authority (reduced state)
    pub account_commitment: Hash32,
    /// Commitment of the guardian authority (reduced state)
    pub guardian_commitment: Hash32,
    /// Parameters proposed for this guardian binding
    pub parameters: GuardianParameters,
    /// Timestamp when the request is made (uses unified time system)
    pub requested_at: PhysicalTime,
    /// Optional expiration for the request (uses unified time system)
    pub expires_at: Option<PhysicalTime>,
}

impl GuardianRequestInput {
    /// Get timestamp in milliseconds (backward compatibility)
    pub fn requested_at_ms(&self) -> u64 {
        self.requested_at.ts_ms
    }

    /// Get expiration in milliseconds (backward compatibility)
    pub fn expires_at_ms(&self) -> Option<u64> {
        self.expires_at.as_ref().map(|t| t.ts_ms)
    }
}

/// Consensus inputs required to finalize a guardian binding
#[derive(Debug, Clone)]
pub struct GuardianAcceptInput {
    /// Relational context where the binding will be stored
    pub context: ContextId,
    /// Account authority being protected
    pub account: AuthorityId,
    /// Guardian authority
    pub guardian: AuthorityId,
    /// Commitment of the account authority (prestate)
    pub account_commitment: Hash32,
    /// Commitment of the guardian authority (prestate)
    pub guardian_commitment: Hash32,
    /// Guardian binding parameters
    pub parameters: GuardianParameters,
    /// Consensus key packages for witnesses (indexed by AuthorityId)
    pub key_packages: HashMap<AuthorityId, Share>,
    /// Group public key for the witness set
    pub group_public_key: PublicKeyPackage,
    /// Epoch for consensus
    pub epoch: Epoch,
    // Note: Consensus configuration hash (witness set, quorum) already encoded
    // into the public key / packages; kept explicit for future config
}

#[async_trait]
pub trait GuardianEffects: Send + Sync {
    /// Record a guardian request in the relational context
    async fn request_guardian(&self, input: GuardianRequestInput) -> Result<()>;

    /// Cancel a previously issued guardian request
    async fn cancel_guardian_request(&self, input: GuardianRequestInput) -> Result<()>;

    /// Accept a guardian request and create a consensus-backed GuardianBinding
    async fn accept_guardian_request(&self, input: GuardianAcceptInput) -> Result<GuardianBinding>;
}
