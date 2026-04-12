#![allow(dead_code, missing_docs)]

use aura_core::time::PhysicalTime;
use aura_core::DeviceId;
use aura_sync::core::{SessionManager, SyncConfig};

pub const TEST_TIMESTAMP_MS: u64 = 1_700_000_000_000;

pub fn test_time(ts_ms: u64) -> PhysicalTime {
    PhysicalTime {
        ts_ms,
        uncertainty: None,
    }
}

pub fn default_test_time() -> PhysicalTime {
    test_time(TEST_TIMESTAMP_MS)
}

#[allow(clippy::unwrap_used)]
pub fn test_device_id(seed: &[u8]) -> DeviceId {
    use aura_core::hash::hash;
    let hash_bytes = hash(seed);
    let uuid_bytes: [u8; 16] = hash_bytes[..16].try_into().unwrap();
    DeviceId(uuid::Uuid::from_bytes(uuid_bytes))
}

pub fn device(seed: u8) -> DeviceId {
    DeviceId::new_from_entropy([seed; 32])
}

pub fn test_sync_config() -> SyncConfig {
    SyncConfig::for_testing()
}

pub fn test_session_manager() -> SessionManager<()> {
    let config = aura_sync::core::session::SessionConfig::default();
    SessionManager::new(config, default_test_time())
}

pub fn device_labels(device_count: usize) -> Vec<String> {
    (0..device_count).map(|i| format!("device_{i}")).collect()
}
