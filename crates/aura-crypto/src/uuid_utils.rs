//! UUID utilities and abstractions
//!
//! Provides unified interfaces for UUID generation and manipulation used throughout Aura.

use uuid::Uuid;

/// Generate a new random UUID v4
///
/// Note: This function uses Uuid::new_v4() directly and should only be used in tests.
/// Production code should use the Effects system for deterministic UUID generation.
#[allow(clippy::disallowed_methods)]
pub fn generate_uuid() -> Uuid {
    Uuid::new_v4()
}

/// Parse a UUID from a string
pub fn parse_uuid(s: &str) -> Result<Uuid, uuid::Error> {
    Uuid::parse_str(s)
}

/// Convert a UUID to a string
pub fn uuid_to_string(uuid: &Uuid) -> String {
    uuid.to_string()
}

/// Convert a UUID to bytes
pub fn uuid_to_bytes(uuid: &Uuid) -> [u8; 16] {
    *uuid.as_bytes()
}

/// Create a UUID from bytes
pub fn uuid_from_bytes(bytes: &[u8; 16]) -> Uuid {
    Uuid::from_bytes(*bytes)
}

/// Create a UUID from a slice of bytes
pub fn uuid_from_slice(bytes: &[u8]) -> Result<Uuid, uuid::Error> {
    Uuid::from_slice(bytes)
}

/// Generate a deterministic UUID from input data using namespace and name
pub fn uuid_from_name(namespace: &Uuid, name: &[u8]) -> Uuid {
    Uuid::new_v5(namespace, name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uuid_generation() {
        let uuid1 = generate_uuid();
        let uuid2 = generate_uuid();

        // UUIDs should be different
        assert_ne!(uuid1, uuid2);

        // Should be version 4
        assert_eq!(uuid1.get_version_num(), 4);
    }

    #[test]
    fn test_uuid_serialization() {
        let uuid = generate_uuid();

        let string = uuid_to_string(&uuid);
        let parsed = parse_uuid(&string).unwrap();
        assert_eq!(uuid, parsed);

        let bytes = uuid_to_bytes(&uuid);
        let from_bytes = uuid_from_bytes(&bytes);
        assert_eq!(uuid, from_bytes);
    }

    #[test]
    fn test_uuid_from_name() {
        let namespace = Uuid::NAMESPACE_OID;
        let name = b"test name";

        let uuid1 = uuid_from_name(&namespace, name);
        let uuid2 = uuid_from_name(&namespace, name);

        // Deterministic - should be the same
        assert_eq!(uuid1, uuid2);

        // Different name should produce different UUID
        let uuid3 = uuid_from_name(&namespace, b"different name");
        assert_ne!(uuid1, uuid3);
    }
}
