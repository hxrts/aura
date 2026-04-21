//! Shared deterministic test builders for common identifier and timestamp types.
//!
//! These helpers are intentionally small and seed-based so Layer 4-7 crates can
//! reuse the same canonical test data shapes instead of open-coding repeated
//! `new_from_entropy` fixtures in local `test_support` modules.

use aura_core::time::{PhysicalTime, TimeStamp};
use aura_core::types::identifiers::{AuthorityId, ChannelId, ContextId};
use aura_core::DeviceId;

/// Create a deterministic authority id from a single-byte seed.
pub fn authority_id(seed: u8) -> AuthorityId {
    AuthorityId::new_from_entropy([seed; 32])
}

/// Create a deterministic device id from a single-byte seed.
pub fn device_id(seed: u8) -> DeviceId {
    DeviceId::new_from_entropy([seed; 32])
}

/// Create a deterministic context id from a single-byte seed.
pub fn context_id(seed: u8) -> ContextId {
    ContextId::new_from_entropy([seed; 32])
}

/// Create a deterministic channel id from a single-byte seed.
pub fn channel_id(seed: u8) -> ChannelId {
    ChannelId::from_bytes([seed; 32])
}

/// Create a deterministic physical timestamp from a millisecond value.
pub fn timestamp_ms(ts_ms: u64) -> TimeStamp {
    TimeStamp::PhysicalClock(PhysicalTime {
        ts_ms,
        uncertainty: None,
    })
}
