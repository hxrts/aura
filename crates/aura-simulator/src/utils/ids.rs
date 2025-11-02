//! ID generation utilities

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use uuid::Uuid;

/// Generate a deterministic UUID as a string
#[allow(clippy::disallowed_methods)]
pub fn generate_random_uuid() -> String {
    // Use current time nanoseconds to create different UUIDs
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let hash_input = format!("random_uuid_{}", timestamp);
    let hash_bytes = blake3::hash(hash_input.as_bytes());
    // SAFETY: blake3 hash is always 32 bytes, slice conversion to [u8; 16] always succeeds
    #[allow(clippy::unwrap_used)]
    Uuid::from_bytes(hash_bytes.as_bytes()[..16].try_into().unwrap()).to_string()
}

/// Generate a deterministic UUID
#[allow(clippy::disallowed_methods)]
pub fn generate_random_uuid_raw() -> Uuid {
    // Use current time nanoseconds to create different UUIDs
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let hash_input = format!("random_uuid_{}", timestamp);
    let hash_bytes = blake3::hash(hash_input.as_bytes());
    // SAFETY: blake3 hash is always 32 bytes, slice conversion to [u8; 16] always succeeds
    #[allow(clippy::unwrap_used)]
    Uuid::from_bytes(hash_bytes.as_bytes()[..16].try_into().unwrap())
}

/// Generate a deterministic UUID based on input seed
pub fn generate_deterministic_uuid(seed: u64) -> Uuid {
    let mut bytes = [0u8; 16];
    let seed_bytes = seed.to_le_bytes();

    // Fill the UUID bytes with a pattern based on the seed
    for i in 0..16 {
        bytes[i] = seed_bytes[i % 8];
    }

    // Ensure this is a valid UUID v4 by setting the version and variant bits
    bytes[6] = (bytes[6] & 0x0f) | 0x40; // Version 4
    bytes[8] = (bytes[8] & 0x3f) | 0x80; // Variant 10

    Uuid::from_bytes(bytes)
}

/// Generate a deterministic UUID as a string
pub fn generate_deterministic_uuid_string(seed: u64) -> String {
    generate_deterministic_uuid(seed).to_string()
}

/// Generate a checkpoint ID
pub fn generate_checkpoint_id() -> String {
    format!("checkpoint_{}", generate_random_uuid())
}

/// Generate a snapshot ID
pub fn generate_snapshot_id() -> String {
    format!("snapshot_{}", generate_random_uuid())
}

/// Generate an analysis ID based on context
pub fn generate_analysis_id(property_name: &str, detected_at: u64) -> String {
    format!("analysis_{}_{}", property_name, detected_at)
}

/// Generate a session ID
pub fn generate_session_id() -> String {
    format!("session_{}", generate_random_uuid())
}

/// Generate a trace ID
pub fn generate_trace_id() -> String {
    format!("trace_{}", generate_random_uuid())
}

/// Generate a test variation ID
pub fn generate_test_variation_id(base_name: &str, variation_index: usize) -> String {
    format!(
        "{}_{:04}_{}",
        base_name,
        variation_index,
        &generate_random_uuid()[..8]
    )
}

/// Generate a hash-based ID from input string
pub fn generate_hash_id(input: &str) -> String {
    let mut hasher = DefaultHasher::new();
    input.hash(&mut hasher);
    let hash = hasher.finish();
    format!("hash_{:016x}", hash)
}

/// Generate a short random ID (8 characters)
pub fn generate_short_id() -> String {
    generate_random_uuid()[..8].to_string()
}

/// Generate a timestamped ID with random suffix
pub fn generate_timestamped_id(prefix: &str) -> String {
    let timestamp = crate::utils::time::current_unix_timestamp_millis();
    let random_suffix = generate_short_id();
    format!("{}_{}__{}", prefix, timestamp, random_suffix)
}

/// Validate that a string is a valid UUID format
pub fn is_valid_uuid(id: &str) -> bool {
    Uuid::parse_str(id).is_ok()
}

/// Extract timestamp from a timestamped ID (if possible)
pub fn extract_timestamp_from_id(id: &str) -> Option<u64> {
    let parts: Vec<&str> = id.split('_').collect();
    if parts.len() >= 2 {
        parts[1].parse().ok()
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_random_uuid_generation() {
        let uuid1 = generate_random_uuid();
        let uuid2 = generate_random_uuid();

        assert_ne!(uuid1, uuid2);
        assert!(is_valid_uuid(&uuid1));
        assert!(is_valid_uuid(&uuid2));
    }

    #[test]
    fn test_deterministic_uuid_generation() {
        let uuid1 = generate_deterministic_uuid_string(42);
        let uuid2 = generate_deterministic_uuid_string(42);
        let uuid3 = generate_deterministic_uuid_string(43);

        assert_eq!(uuid1, uuid2); // Same seed = same UUID
        assert_ne!(uuid1, uuid3); // Different seed = different UUID
        assert!(is_valid_uuid(&uuid1));
        assert!(is_valid_uuid(&uuid3));
    }

    #[test]
    fn test_specialized_id_generation() {
        let checkpoint_id = generate_checkpoint_id();
        let snapshot_id = generate_snapshot_id();
        let session_id = generate_session_id();

        assert!(checkpoint_id.starts_with("checkpoint_"));
        assert!(snapshot_id.starts_with("snapshot_"));
        assert!(session_id.starts_with("session_"));
    }

    #[test]
    fn test_analysis_id_generation() {
        let id = generate_analysis_id("test_property", 12345);
        assert_eq!(id, "analysis_test_property_12345");
    }

    #[test]
    fn test_test_variation_id() {
        let id = generate_test_variation_id("test_scenario", 42);
        assert!(id.starts_with("test_scenario_0042_"));
        assert_eq!(id.len(), "test_scenario_0042_".len() + 8);
    }

    #[test]
    fn test_hash_id_generation() {
        let id1 = generate_hash_id("test_input");
        let id2 = generate_hash_id("test_input");
        let id3 = generate_hash_id("different_input");

        assert_eq!(id1, id2); // Same input = same hash
        assert_ne!(id1, id3); // Different input = different hash
        assert!(id1.starts_with("hash_"));
    }

    #[test]
    fn test_short_id_generation() {
        let id = generate_short_id();
        assert_eq!(id.len(), 8);
    }

    #[test]
    fn test_timestamped_id() {
        let id = generate_timestamped_id("test");
        assert!(id.starts_with("test_"));

        let timestamp = extract_timestamp_from_id(&id);
        assert!(timestamp.is_some());
        assert!(timestamp.unwrap() > 0);
    }

    #[test]
    fn test_uuid_validation() {
        assert!(is_valid_uuid("550e8400-e29b-41d4-a716-446655440000"));
        assert!(!is_valid_uuid("invalid-uuid"));
        assert!(!is_valid_uuid(""));
    }
}
