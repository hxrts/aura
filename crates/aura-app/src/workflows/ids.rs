//! # Deterministic ID Derivation
//!
//! Portable utilities for deterministically deriving identifiers from seeds.
//! Useful for demos, simulations, and reproducible test scenarios across
//! all frontends (CLI, TUI, iOS, Android, Web).
//!
//! ## Usage
//!
//! ```rust
//! use aura_app::ui::workflows::ids;
//!
//! // Derive deterministic IDs for a demo scenario
//! let alice_authority = ids::authority_id("demo:2024:Alice");
//! let alice_device = ids::device_id("demo:2024:Alice:device");
//! let general_channel = ids::context_id("demo:2024:general");
//!
//! // Same seed always produces same ID
//! assert_eq!(ids::authority_id("demo:2024:Alice"), alice_authority);
//! ```
//!
//! ## Domain Separation
//!
//! Each ID type uses a unique domain prefix to ensure that the same seed
//! produces different IDs for different types:
//! - `authority_id("alice")` ≠ `device_id("alice")`
//! - `context_id("chat")` ≠ `guardian_id("chat")`

use aura_core::hash;
use aura_core::identifiers::{AuthorityId, ContextId, DeviceId, GuardianId};
use uuid::Uuid;

/// Domain prefix for portable ID derivation.
///
/// Using "aura/" prefix for frontend-agnostic IDs.
const DOMAIN_PREFIX: &str = "aura";

/// Compute a deterministic 32-byte digest from seed and domain.
fn digest(seed: &str, domain: &str) -> [u8; 32] {
    let mut h = hash::hasher();
    h.update(domain.as_bytes());
    h.update(seed.as_bytes());
    h.finalize()
}

/// Deterministically derive an AuthorityId from a seed label.
///
/// # Example
///
/// ```rust
/// use aura_app::ui::workflows::ids::authority_id;
///
/// let id1 = authority_id("alice");
/// let id2 = authority_id("alice");
/// assert_eq!(id1, id2); // Same seed -> same ID
///
/// let id3 = authority_id("bob");
/// assert_ne!(id1, id3); // Different seed -> different ID
/// ```
#[must_use]
pub fn authority_id(seed: &str) -> AuthorityId {
    AuthorityId::new_from_entropy(digest(seed, &format!("{DOMAIN_PREFIX}/authority")))
}

/// Deterministically derive a ContextId from a seed label.
///
/// # Example
///
/// ```rust
/// use aura_app::ui::workflows::ids::context_id;
///
/// let id = context_id("general-chat");
/// assert_eq!(id, context_id("general-chat"));
/// ```
#[must_use]
pub fn context_id(seed: &str) -> ContextId {
    ContextId::new_from_entropy(digest(seed, &format!("{DOMAIN_PREFIX}/context")))
}

/// Deterministically derive a DeviceId from a seed label.
///
/// # Example
///
/// ```rust
/// use aura_app::ui::workflows::ids::device_id;
///
/// let id = device_id("laptop-1");
/// assert_eq!(id, device_id("laptop-1"));
/// ```
#[must_use]
pub fn device_id(seed: &str) -> DeviceId {
    DeviceId::new_from_entropy(digest(seed, &format!("{DOMAIN_PREFIX}/device")))
}

/// Deterministically derive a GuardianId from a seed label.
///
/// # Example
///
/// ```rust
/// use aura_app::ui::workflows::ids::guardian_id;
///
/// let id = guardian_id("carol-guardian");
/// assert_eq!(id, guardian_id("carol-guardian"));
/// ```
#[must_use]
pub fn guardian_id(seed: &str) -> GuardianId {
    GuardianId::new_from_entropy(digest(seed, &format!("{DOMAIN_PREFIX}/guardian")))
}

/// Deterministically derive a UUID from a seed label.
///
/// Useful for non-core identifiers like invitation IDs, session IDs, etc.
///
/// # Example
///
/// ```rust
/// use aura_app::ui::workflows::ids::uuid;
///
/// let id = uuid("invitation-123");
/// assert_eq!(id, uuid("invitation-123"));
/// ```
#[must_use]
pub fn uuid(seed: &str) -> Uuid {
    let bytes = digest(seed, &format!("{DOMAIN_PREFIX}/uuid"));
    let mut uuid_bytes = [0u8; 16];
    uuid_bytes.copy_from_slice(&bytes[..16]);
    Uuid::from_bytes(uuid_bytes)
}

/// Deterministically derive an InvitationId (UUID) from a seed label.
///
/// Convenience alias for `uuid()` with clearer intent.
#[must_use]
#[inline]
pub fn invitation_id(seed: &str) -> Uuid {
    uuid(seed)
}

/// Deterministically derive a SessionId (UUID) from a seed label.
///
/// Convenience alias for `uuid()` with clearer intent.
#[must_use]
#[inline]
pub fn session_id(seed: &str) -> Uuid {
    // Use different domain for session IDs
    let bytes = digest(seed, &format!("{DOMAIN_PREFIX}/session"));
    let mut uuid_bytes = [0u8; 16];
    uuid_bytes.copy_from_slice(&bytes[..16]);
    Uuid::from_bytes(uuid_bytes)
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
    fn test_guardian_id_is_deterministic() {
        let id1 = guardian_id("carol-guardian");
        let id2 = guardian_id("carol-guardian");
        assert_eq!(id1, id2, "Same seed should produce same guardian ID");
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
    fn test_invitation_id_alias() {
        let u1 = invitation_id("invite-abc");
        let u2 = uuid("invite-abc");
        assert_eq!(u1, u2, "invitation_id should be alias for uuid");
    }

    #[test]
    fn test_session_id_differs_from_uuid() {
        let s = session_id("test");
        let u = uuid("test");
        assert_ne!(s, u, "session_id uses different domain than uuid");
    }

    #[test]
    fn test_different_id_types_are_independent() {
        // Same seed, different ID types should produce different values
        let auth = authority_id("test");
        let ctx = context_id("test");
        let dev = device_id("test");
        let guard = guardian_id("test");

        // The internal UUIDs should all be different due to domain separation
        assert_ne!(auth.0, dev.0, "Authority and device IDs should differ");
        assert_ne!(ctx.0, guard.0, "Context and guardian IDs should differ");
    }
}
