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
#[must_use]
pub fn authority_id(seed: &str) -> AuthorityId {
    AuthorityId::new_from_entropy(digest(seed, "aura-terminal/authority"))
}

/// Deterministically derive a ContextId from a seed label.
#[must_use]
pub fn context_id(seed: &str) -> ContextId {
    ContextId::new_from_entropy(digest(seed, "aura-terminal/context"))
}

/// Deterministically derive a DeviceId from a seed label.
#[must_use]
pub fn device_id(seed: &str) -> DeviceId {
    DeviceId::new_from_entropy(digest(seed, "aura-terminal/device"))
}

/// Deterministically derive a UUID from a seed label (for non-core IDs).
#[must_use]
pub fn uuid(seed: &str) -> Uuid {
    let bytes = digest(seed, "aura-terminal/uuid");
    let mut uuid_bytes = [0u8; 16];
    uuid_bytes.copy_from_slice(&bytes[..16]);
    Uuid::from_bytes(uuid_bytes)
}

/// Deterministically derive a GuardianId from a seed label.
#[must_use]
pub fn guardian_id(seed: &str) -> GuardianId {
    GuardianId::new_from_entropy(digest(seed, "aura-terminal/guardian"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_digest_is_deterministic() {
        let d1 = digest("test-seed", "test-domain");
        let d2 = digest("test-seed", "test-domain");
        assert_eq!(d1, d2, "Same inputs should produce same digest");
    }

    #[test]
    fn test_digest_varies_with_seed() {
        let d1 = digest("seed-a", "domain");
        let d2 = digest("seed-b", "domain");
        assert_ne!(d1, d2, "Different seeds should produce different digests");
    }

    #[test]
    fn test_digest_varies_with_domain() {
        let d1 = digest("seed", "domain-a");
        let d2 = digest("seed", "domain-b");
        assert_ne!(d1, d2, "Different domains should produce different digests");
    }

    #[test]
    fn test_authority_id_is_deterministic() {
        let id1 = authority_id("alice");
        let id2 = authority_id("alice");
        assert_eq!(id1, id2, "Same seed should produce same authority ID");
    }

    #[test]
    fn test_authority_id_varies_with_seed() {
        let id1 = authority_id("alice");
        let id2 = authority_id("bob");
        assert_ne!(
            id1, id2,
            "Different seeds should produce different authority IDs"
        );
    }

    #[test]
    fn test_context_id_is_deterministic() {
        let id1 = context_id("general-chat");
        let id2 = context_id("general-chat");
        assert_eq!(id1, id2, "Same seed should produce same context ID");
    }

    #[test]
    fn test_device_id_is_deterministic() {
        let id1 = device_id("laptop");
        let id2 = device_id("laptop");
        assert_eq!(id1, id2, "Same seed should produce same device ID");
    }

    #[test]
    fn test_uuid_is_deterministic() {
        let u1 = uuid("invitation-123");
        let u2 = uuid("invitation-123");
        assert_eq!(u1, u2, "Same seed should produce same UUID");
    }

    #[test]
    fn test_uuid_varies_with_seed() {
        let u1 = uuid("invitation-1");
        let u2 = uuid("invitation-2");
        assert_ne!(u1, u2, "Different seeds should produce different UUIDs");
    }

    #[test]
    fn test_guardian_id_is_deterministic() {
        let id1 = guardian_id("carol-guardian");
        let id2 = guardian_id("carol-guardian");
        assert_eq!(id1, id2, "Same seed should produce same guardian ID");
    }

    #[test]
    fn test_different_id_types_are_independent() {
        // Even with the same seed, different ID types should produce different values
        // due to domain separation
        let auth = authority_id("test");
        let ctx = context_id("test");
        let dev = device_id("test");
        let guard = guardian_id("test");

        // The internal UUIDs should all be different due to domain separation
        assert_ne!(auth.0, dev.0, "Authority and device IDs should differ");
        assert_ne!(ctx.0, guard.0, "Context and guardian IDs should differ");
    }
}
