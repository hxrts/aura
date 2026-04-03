//! Centralized key derivation helpers.
//!
//! Aura uses a single keyed derivation surface for symmetric key material,
//! route-hop material, and other non-signature derivations. The effect trait
//! remains in `aura-core::effects`, while this module owns the pure algorithm.

#![allow(clippy::disallowed_types, clippy::disallowed_methods)]

use crate::{AuraError, AuraResult};

const KDF_SALT_DOMAIN: &[u8] = b"AURA_KDF_SALT_V1";
const KDF_EXTRACT_DOMAIN: &[u8] = b"AURA_KDF_EXTRACT_V1";
const KDF_EXPAND_DOMAIN: &[u8] = b"AURA_KDF_EXPAND_V1";

fn update_len_prefixed(hasher: &mut blake3::Hasher, bytes: &[u8]) {
    hasher.update(&(bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

fn derive_salt_key(salt: &[u8]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(KDF_SALT_DOMAIN);
    update_len_prefixed(&mut hasher, salt);
    *hasher.finalize().as_bytes()
}

/// Derive arbitrary-length key material from input keying material, salt, and info.
pub fn derive_key_material(
    ikm: &[u8],
    salt: &[u8],
    info: &[u8],
    output_len: u32,
) -> AuraResult<Vec<u8>> {
    if output_len == 0 {
        return Err(AuraError::crypto(
            "derived key output length must be greater than 0",
        ));
    }

    let salt_key = derive_salt_key(salt);

    let mut extractor = blake3::Hasher::new_keyed(&salt_key);
    extractor.update(KDF_EXTRACT_DOMAIN);
    update_len_prefixed(&mut extractor, ikm);
    let prk = *extractor.finalize().as_bytes();

    let mut expander = blake3::Hasher::new_keyed(&prk);
    expander.update(KDF_EXPAND_DOMAIN);
    update_len_prefixed(&mut expander, info);

    let mut output = vec![0u8; output_len as usize];
    expander.finalize_xof().fill(&mut output);
    Ok(output)
}

/// Derive fixed-size key material into an array.
pub fn derive_key<const N: usize>(ikm: &[u8], salt: &[u8], info: &[u8]) -> AuraResult<[u8; N]> {
    let mut output = [0u8; N];
    let bytes = derive_key_material(ikm, salt, info, N as u32)?;
    output.copy_from_slice(&bytes);
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kdf_is_deterministic() {
        let first = derive_key_material(b"ikm", b"salt", b"info", 32).unwrap();
        let second = derive_key_material(b"ikm", b"salt", b"info", 32).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn kdf_changes_with_domain_inputs() {
        let baseline = derive_key_material(b"ikm", b"salt", b"info", 32).unwrap();
        let salt_changed = derive_key_material(b"ikm", b"salt2", b"info", 32).unwrap();
        let info_changed = derive_key_material(b"ikm", b"salt", b"info2", 32).unwrap();
        assert_ne!(baseline, salt_changed);
        assert_ne!(baseline, info_changed);
    }

    #[test]
    fn kdf_supports_variable_output_lengths() {
        let short = derive_key_material(b"ikm", b"salt", b"info", 16).unwrap();
        let long = derive_key_material(b"ikm", b"salt", b"info", 64).unwrap();
        assert_eq!(short.len(), 16);
        assert_eq!(long.len(), 64);
        assert_eq!(&short[..], &long[..16]);
        assert_ne!(&long[..16], &long[16..32]);
    }
}
