//! Effect-backed identifier generation helpers.
//!
//! These helpers ensure production ID generation uses RandomEffects rather than
//! ambient entropy. Tests should prefer deterministic seeds in aura-core test_utils.

use aura_core::effects::RandomEffects;
use aura_core::identifiers::{
    AccountId, AuthorityId, ContextId, DeviceId, EventId, GuardianId, OperationId, SessionId,
};

/// Generate a new AuthorityId using RandomEffects.
pub async fn new_authority_id<R: RandomEffects + ?Sized>(effects: &R) -> AuthorityId {
    AuthorityId::new_from_entropy(effects.random_bytes_32().await)
}

/// Generate a new ContextId using RandomEffects.
pub async fn new_context_id<R: RandomEffects + ?Sized>(effects: &R) -> ContextId {
    ContextId::new_from_entropy(effects.random_bytes_32().await)
}

/// Generate a new DeviceId using RandomEffects.
pub async fn new_device_id<R: RandomEffects + ?Sized>(effects: &R) -> DeviceId {
    DeviceId::new_from_entropy(effects.random_bytes_32().await)
}

/// Generate a new AccountId using RandomEffects.
pub async fn new_account_id<R: RandomEffects + ?Sized>(effects: &R) -> AccountId {
    AccountId::new_from_entropy(effects.random_bytes_32().await)
}

/// Generate a new GuardianId using RandomEffects.
pub async fn new_guardian_id<R: RandomEffects + ?Sized>(effects: &R) -> GuardianId {
    GuardianId::new_from_entropy(effects.random_bytes_32().await)
}

/// Generate a new SessionId using RandomEffects.
pub async fn new_session_id<R: RandomEffects + ?Sized>(effects: &R) -> SessionId {
    SessionId::new_from_entropy(effects.random_bytes_32().await)
}

/// Generate a new EventId using RandomEffects.
pub async fn new_event_id<R: RandomEffects + ?Sized>(effects: &R) -> EventId {
    EventId::new_from_entropy(effects.random_bytes_32().await)
}

/// Generate a new OperationId using RandomEffects.
pub async fn new_operation_id<R: RandomEffects + ?Sized>(effects: &R) -> OperationId {
    OperationId::new_from_entropy(effects.random_bytes_32().await)
}
