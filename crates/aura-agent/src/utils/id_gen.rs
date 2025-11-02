//! ID generation utilities
//!
//! Centralized ID generation for data, capabilities, and other entities.

/// Generate a new data ID
pub fn new_data_id() -> String {
    format!("data:{}", aura_crypto::generate_uuid())
}

/// Generate a new encrypted data ID
pub fn new_encrypted_data_id() -> String {
    format!("encrypted:{}", aura_crypto::generate_uuid())
}

/// Generate a new capability ID
pub fn new_capability_id() -> String {
    format!("cap:{}", aura_crypto::generate_uuid())
}

/// Generate a capability ID for specific data and grantee
pub fn new_capability_id_for(data_id: &str, grantee_device: uuid::Uuid) -> String {
    format!("cap_{}_{}", data_id, grantee_device)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_data_id() {
        let id = new_data_id();
        assert!(id.starts_with("data:"));
        assert!(uuid::Uuid::parse_str(&id[5..]).is_ok());
    }

    #[test]
    fn test_new_encrypted_data_id() {
        let id = new_encrypted_data_id();
        assert!(id.starts_with("encrypted:"));
        assert!(uuid::Uuid::parse_str(&id[10..]).is_ok());
    }

    #[test]
    fn test_new_capability_id() {
        let id = new_capability_id();
        assert!(id.starts_with("cap:"));
        assert!(uuid::Uuid::parse_str(&id[4..]).is_ok());
    }

    #[test]
    fn test_new_capability_id_for() {
        let data_id = "data-123";
        let device_uuid = uuid::Uuid::new_v4();
        let id = new_capability_id_for(data_id, device_uuid);
        assert_eq!(id, format!("cap_data-123_{}", device_uuid));
    }
}
