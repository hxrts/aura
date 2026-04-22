//! Trusted public-key resolution interfaces.

use crate::{AuthorityId, DeviceId, Hash32};

/// Public-key domain resolved by the trusted key registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TrustedKeyDomain {
    /// Authority threshold/FROST verification key for a specific epoch.
    AuthorityThreshold,
    /// Enrolled device verification key.
    Device,
    /// Guardian authority verification key.
    Guardian,
    /// Release or OTA signing verification key.
    Release,
}

/// Lifecycle status for trusted key material.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrustedKeyStatus {
    /// The key is valid for verifier use.
    Active,
    /// The key was replaced by a newer epoch/key and must not verify new input.
    Rotated { replaced_by_epoch: Option<u64> },
    /// The key was explicitly revoked.
    Revoked { reason: String },
}

/// Trusted key bytes plus local lifecycle metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedPublicKey {
    domain: TrustedKeyDomain,
    bytes: Vec<u8>,
    status: TrustedKeyStatus,
    epoch: Option<u64>,
    key_hash: Hash32,
}

impl TrustedPublicKey {
    /// Construct active trusted key material.
    #[must_use]
    pub fn active(
        domain: TrustedKeyDomain,
        epoch: Option<u64>,
        bytes: Vec<u8>,
        key_hash: Hash32,
    ) -> Self {
        Self {
            domain,
            bytes,
            status: TrustedKeyStatus::Active,
            epoch,
            key_hash,
        }
    }

    /// Mark key material inactive.
    pub fn set_status(&mut self, status: TrustedKeyStatus) {
        self.status = status;
    }

    /// The key domain this material belongs to.
    #[must_use]
    pub fn domain(&self) -> TrustedKeyDomain {
        self.domain
    }

    /// The epoch this key belongs to, when epoch-scoped.
    #[must_use]
    pub fn epoch(&self) -> Option<u64> {
        self.epoch
    }

    /// Raw public-key bytes for the low-level verifier.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Stable hash of the trusted public key.
    #[must_use]
    pub fn key_hash(&self) -> Hash32 {
        self.key_hash
    }

    /// Current lifecycle status.
    #[must_use]
    pub fn status(&self) -> &TrustedKeyStatus {
        &self.status
    }
}

/// Errors returned by trusted key resolution.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KeyResolutionError {
    /// Key material was empty and cannot be trusted.
    #[error("trusted {domain:?} key material is empty")]
    EmptyKey { domain: TrustedKeyDomain },
    /// No trusted key exists for the requested principal and epoch/domain.
    #[error("unknown trusted {domain:?} key")]
    Unknown { domain: TrustedKeyDomain },
    /// The requested key exists but is no longer active.
    #[error("trusted {domain:?} key is not active: {status:?}")]
    Inactive {
        domain: TrustedKeyDomain,
        status: TrustedKeyStatus,
    },
}

/// Trusted key resolver consumed by verifier boundaries.
pub trait TrustedKeyResolver {
    /// Resolve an active authority threshold key for the requested epoch.
    fn resolve_authority_threshold_key(
        &self,
        authority: AuthorityId,
        epoch: u64,
    ) -> Result<TrustedPublicKey, KeyResolutionError>;

    /// Resolve an active enrolled device key.
    fn resolve_device_key(&self, device: DeviceId) -> Result<TrustedPublicKey, KeyResolutionError>;

    /// Resolve an active guardian key.
    fn resolve_guardian_key(
        &self,
        guardian: AuthorityId,
    ) -> Result<TrustedPublicKey, KeyResolutionError>;

    /// Resolve an active release/OTA signing key.
    fn resolve_release_key(
        &self,
        authority: AuthorityId,
    ) -> Result<TrustedPublicKey, KeyResolutionError>;
}
