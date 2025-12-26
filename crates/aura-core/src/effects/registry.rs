//! Canonical operation mappings for effect types.

use super::EffectType;

/// Return the canonical list of operations for an effect type.
///
/// This is used by registry/adapters for capability discovery and validation.
pub fn operations_for(effect_type: EffectType) -> &'static [&'static str] {
    match effect_type {
        EffectType::Console => &[
            "log_info",
            "log_warn",
            "log_error",
            "log_debug",
        ],
        EffectType::Random => &[
            "random_bytes",
            "random_bytes_32",
            "random_u64",
            "random_range",
            "random_uuid",
        ],
        EffectType::Crypto => &[
            "hkdf_derive",
            "derive_key",
            "ed25519_generate_keypair",
            "ed25519_sign",
            "ed25519_verify",
            "ed25519_public_key",
            "generate_signing_keys",
            "sign_with_key",
            "verify_signature",
            "frost_generate_keys",
            "frost_generate_nonces",
            "frost_create_signing_package",
            "frost_sign_share",
            "frost_aggregate_signatures",
            "frost_verify",
            "aes_gcm_encrypt",
            "aes_gcm_decrypt",
            "chacha20_encrypt",
            "chacha20_decrypt",
            "frost_rotate_keys",
        ],
        EffectType::Network => &[
            "send_to_peer",
            "broadcast",
            "receive",
            "receive_from",
            "connected_peers",
            "is_peer_connected",
            "subscribe_to_peer_events",
            "open",
            "send",
            "close",
        ],
        EffectType::Storage => &[
            "store",
            "retrieve",
            "remove",
            "list_keys",
            "exists",
            "store_batch",
            "retrieve_batch",
            "clear_all",
            "stats",
        ],
        EffectType::Time => &[
            "physical_time",
            "sleep_ms",
            "logical_advance",
            "logical_now",
            "order_time",
        ],
        EffectType::System => &[
            "log",
            "log_with_context",
            "health_check",
            "get_system_info",
            "set_config",
            "get_config",
            "get_metrics",
            "restart_component",
            "shutdown",
        ],
        _ => &[],
    }
}

/// Check if an operation is supported for an effect type.
pub fn supports_operation(effect_type: EffectType, operation: &str) -> bool {
    operations_for(effect_type).contains(&operation)
}
