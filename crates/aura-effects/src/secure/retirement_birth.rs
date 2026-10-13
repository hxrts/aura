//! Private authenticated payload for a protected retirement birth.
//!
//! The descriptor and ordinary value share one encrypted record publication.
//! Existing protected records without this birth format cannot gain deletion
//! authority later, even when their plaintext happens to match a proposal.

use aura_core::effects::SecureStorageLocation;
use aura_core::time::PhysicalTime;
use aura_core::AuraError;
use serde::{Deserialize, Serialize};

pub(super) const MAX_BIRTH_DESCRIPTOR_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProtectedBirthDescriptor {
    pub(super) profile_digest: [u8; 32],
    pub(super) location: SecureStorageLocation,
    /// Original authenticated registration, including its signature proof.
    pub(super) signed_statement: Vec<u8>,
    pub(super) expiry: PhysicalTime,
}

pub(super) fn encode_payload(
    descriptor: &ProtectedBirthDescriptor,
    plaintext: &[u8],
) -> Result<Vec<u8>, AuraError> {
    if descriptor.signed_statement.len() > MAX_BIRTH_DESCRIPTOR_BYTES
        || descriptor.location.full_path().len() > MAX_BIRTH_DESCRIPTOR_BYTES
        || descriptor.expiry.uncertainty.is_some()
    {
        return Err(AuraError::invalid(
            "protected retirement descriptor exceeds bound",
        ));
    }
    let header = serde_json::to_vec(descriptor).map_err(|source| AuraError::Serialization {
        message: "encode protected retirement birth".into(),
        source: Some(std::sync::Arc::new(source)),
    })?;
    if header.len() > MAX_BIRTH_DESCRIPTOR_BYTES {
        return Err(AuraError::invalid(
            "protected retirement descriptor exceeds bound",
        ));
    }
    let length = u32::try_from(header.len())
        .map_err(|_| AuraError::invalid("protected retirement descriptor length overflow"))?;
    let capacity = 4usize
        .checked_add(header.len())
        .and_then(|size| size.checked_add(plaintext.len()))
        .ok_or_else(|| AuraError::invalid("protected retirement payload length overflow"))?;
    let mut payload = Vec::with_capacity(capacity);
    payload.extend_from_slice(&length.to_le_bytes());
    payload.extend_from_slice(&header);
    payload.extend_from_slice(plaintext);
    Ok(payload)
}

pub(super) fn decode_payload(
    location: &SecureStorageLocation,
    payload: &[u8],
) -> Result<(ProtectedBirthDescriptor, Vec<u8>), AuraError> {
    let encoded_len: [u8; 4] = payload
        .get(..4)
        .ok_or_else(|| AuraError::invalid("protected retirement birth is truncated"))?
        .try_into()
        .map_err(|_| AuraError::invalid("protected retirement birth length is invalid"))?;
    let length = u32::from_le_bytes(encoded_len) as usize;
    if length > MAX_BIRTH_DESCRIPTOR_BYTES {
        return Err(AuraError::invalid(
            "protected retirement descriptor exceeds bound",
        ));
    }
    let end = 4usize
        .checked_add(length)
        .ok_or_else(|| AuraError::invalid("protected retirement descriptor length overflow"))?;
    let encoded = payload
        .get(4..end)
        .ok_or_else(|| AuraError::invalid("protected retirement descriptor is truncated"))?;
    let descriptor: ProtectedBirthDescriptor =
        serde_json::from_slice(encoded).map_err(|source| AuraError::Serialization {
            message: "decode protected retirement birth".into(),
            source: Some(std::sync::Arc::new(source)),
        })?;
    if descriptor.location != *location
        || descriptor.signed_statement.is_empty()
        || descriptor.expiry.uncertainty.is_some()
    {
        return Err(AuraError::permission_denied(
            "protected retirement birth lacks original location or signed statement",
        ));
    }
    Ok((descriptor, payload[end..].to_vec()))
}

pub(super) fn admit_replay(
    proposed: &ProtectedBirthDescriptor,
    proposed_bytes: &[u8],
    original: Option<(&ProtectedBirthDescriptor, &[u8])>,
    legacy_record_present: bool,
) -> Result<bool, AuraError> {
    if legacy_record_present {
        return Err(AuraError::permission_denied(
            "protected retirement metadata cannot be added after publication",
        ));
    }
    let Some((descriptor, bytes)) = original else {
        return Ok(true);
    };
    if descriptor != proposed || bytes != proposed_bytes {
        return Err(AuraError::permission_denied(
            "protected retirement birth differs from original exact record",
        ));
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor(profile: u8, statement: &[u8]) -> ProtectedBirthDescriptor {
        ProtectedBirthDescriptor {
            profile_digest: [profile; 32],
            location: SecureStorageLocation::new("retirement-test", "original"),
            signed_statement: statement.to_vec(),
            expiry: PhysicalTime::exact(500),
        }
    }

    #[test]
    fn birth_codec_preserves_ordinary_bytes_and_exact_signed_binding() {
        let original = descriptor(1, b"signed original");
        let encoded = encode_payload(&original, b"secret bytes").unwrap();
        let (decoded, plaintext) = decode_payload(&original.location, &encoded).unwrap();
        assert_eq!(decoded, original);
        assert_eq!(plaintext, b"secret bytes");
        assert!(
            decode_payload(&SecureStorageLocation::new("foreign", "original"), &encoded).is_err()
        );
        assert!(decode_payload(&original.location, &encoded[..3]).is_err());
    }

    #[test]
    fn birth_replay_rejects_late_descriptor_foreign_profile_and_changed_bytes() {
        let original = descriptor(1, b"signed original");
        assert!(admit_replay(&original, b"secret", None, false).unwrap());
        assert!(!admit_replay(&original, b"secret", Some((&original, b"secret")), false).unwrap());
        assert!(admit_replay(&original, b"secret", None, true).is_err());
        assert!(admit_replay(
            &descriptor(2, b"signed original"),
            b"secret",
            Some((&original, b"secret")),
            false
        )
        .is_err());
        assert!(admit_replay(
            &descriptor(1, b"forged"),
            b"secret",
            Some((&original, b"secret")),
            false
        )
        .is_err());
        assert!(admit_replay(
            &original,
            b"replacement",
            Some((&original, b"secret")),
            false
        )
        .is_err());
    }
}
