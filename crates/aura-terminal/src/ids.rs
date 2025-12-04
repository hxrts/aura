//! Deterministic identifier derivation utilities for CLI and TUI layers.
//!
//! These helpers keep Layer 7 reproducible and simulator-friendly by
//! deriving identifiers from stable string seeds instead of consuming
//! ambient entropy. Seeds should be stable per scenario/user so outputs
//! remain deterministic across runs.

use aura_core::hash;
use aura_core::identifiers::{AuthorityId, ContextId, DeviceId, GuardianId};
use uuid::Uuid;

fn digest(seed: &str, domain: &str) -> [u8; 32] {
    let mut h = hash::hasher();
    h.update(domain.as_bytes());
    h.update(seed.as_bytes());
    h.finalize()
}

/// Deterministically derive an AuthorityId from a seed label.
pub fn authority_id(seed: &str) -> AuthorityId {
    AuthorityId::new_from_entropy(digest(seed, "aura-terminal/authority"))
}

/// Deterministically derive a ContextId from a seed label.
pub fn context_id(seed: &str) -> ContextId {
    ContextId::new_from_entropy(digest(seed, "aura-terminal/context"))
}

/// Deterministically derive a DeviceId from a seed label.
pub fn device_id(seed: &str) -> DeviceId {
    DeviceId::new_from_entropy(digest(seed, "aura-terminal/device"))
}

/// Deterministically derive a UUID from a seed label (for non-core IDs).
pub fn uuid(seed: &str) -> Uuid {
    let bytes = digest(seed, "aura-terminal/uuid");
    let mut uuid_bytes = [0u8; 16];
    uuid_bytes.copy_from_slice(&bytes[..16]);
    Uuid::from_bytes(uuid_bytes)
}

/// Deterministically derive a GuardianId from a seed label.
pub fn guardian_id(seed: &str) -> GuardianId {
    GuardianId::new_from_entropy(digest(seed, "aura-terminal/guardian"))
}
