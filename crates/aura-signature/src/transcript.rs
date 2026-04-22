//! Canonical, domain-separated signing transcripts.

use crate::{AuthenticationError, Result};
use aura_core::effects::CryptoEffects;
use aura_core::threshold::{ApprovalContext, SignableOperation, SigningContext};
use aura_core::util::serialization;
use aura_core::AuthorityId;
use serde::Serialize;

/// Stable envelope wrapped around every security-critical signing payload.
///
/// The envelope keeps protocol domain and schema version outside the payload so
/// two protocols with byte-identical fields still produce different signature
/// inputs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TranscriptEnvelope<T> {
    /// Human-readable protocol/domain separator.
    pub domain_separator: String,
    /// Schema version for the signed payload shape.
    pub schema_version: u16,
    /// Protocol-specific payload.
    pub payload: T,
}

impl<T> TranscriptEnvelope<T> {
    /// Create a new transcript envelope.
    pub fn new(domain_separator: impl Into<String>, schema_version: u16, payload: T) -> Self {
        Self {
            domain_separator: domain_separator.into(),
            schema_version,
            payload,
        }
    }
}

/// A protocol message that can produce canonical bytes for signing.
pub trait SecurityTranscript {
    /// Protocol-specific payload shape serialized inside the shared envelope.
    type Payload: Serialize;

    /// Domain separator for this protocol's signing context.
    const DOMAIN_SEPARATOR: &'static str;

    /// Schema version for the transcript payload.
    const SCHEMA_VERSION: u16 = 1;

    /// Build the protocol-specific payload covered by the signature.
    fn transcript_payload(&self) -> Self::Payload;

    /// Encode this transcript using Aura's canonical DAG-CBOR serialization.
    fn transcript_bytes(&self) -> Result<Vec<u8>> {
        encode_transcript(
            Self::DOMAIN_SEPARATOR,
            Self::SCHEMA_VERSION,
            &self.transcript_payload(),
        )
    }
}

/// Encode a domain-separated transcript payload into canonical signing bytes.
pub fn encode_transcript<T: Serialize>(
    domain_separator: &'static str,
    schema_version: u16,
    payload: &T,
) -> Result<Vec<u8>> {
    if domain_separator.trim().is_empty() {
        return Err(AuthenticationError::TranscriptEncoding {
            details: "transcript domain separator must be non-empty".to_string(),
        });
    }

    serialization::to_vec(&TranscriptEnvelope::new(
        domain_separator,
        schema_version,
        payload,
    ))
    .map_err(|error| AuthenticationError::TranscriptEncoding {
        details: error.to_string(),
    })
}

#[derive(Debug, Clone, Serialize)]
struct ThresholdSigningContextTranscriptPayload {
    authority: AuthorityId,
    operation: SignableOperation,
    approval_context: ApprovalContext,
    epoch: u64,
}

struct ThresholdSigningContextTranscript<'a> {
    context: &'a SigningContext,
    epoch: u64,
}

impl SecurityTranscript for ThresholdSigningContextTranscript<'_> {
    type Payload = ThresholdSigningContextTranscriptPayload;

    const DOMAIN_SEPARATOR: &'static str = "aura.threshold.signing-context";

    fn transcript_payload(&self) -> Self::Payload {
        ThresholdSigningContextTranscriptPayload {
            authority: self.context.authority,
            operation: self.context.operation.clone(),
            approval_context: self.context.approval_context.clone(),
            epoch: self.epoch,
        }
    }
}

/// Encode a non-tree threshold signing context into canonical signing bytes.
///
/// Tree operations use their tree-specific binding message because tree
/// verification binds the group public key into the attested operation proof.
pub fn threshold_signing_context_transcript_bytes(
    context: &SigningContext,
    epoch: u64,
) -> Result<Vec<u8>> {
    ThresholdSigningContextTranscript { context, epoch }.transcript_bytes()
}

/// Sign a typed transcript with Ed25519.
pub async fn sign_ed25519_transcript<E, T>(
    crypto: &E,
    transcript: &T,
    private_key: &[u8],
) -> Result<Vec<u8>>
where
    E: CryptoEffects + Send + Sync + ?Sized,
    T: SecurityTranscript + ?Sized,
{
    let bytes = transcript.transcript_bytes()?;
    crypto
        .ed25519_sign(&bytes, private_key)
        .await
        .map_err(|error| AuthenticationError::CryptoError {
            details: format!("Ed25519 transcript signing failed: {error}"),
        })
}

/// Verify an Ed25519 signature over a typed transcript.
pub async fn verify_ed25519_transcript<E, T>(
    crypto: &E,
    transcript: &T,
    signature: &[u8],
    public_key: &[u8],
) -> Result<bool>
where
    E: CryptoEffects + Send + Sync + ?Sized,
    T: SecurityTranscript + ?Sized,
{
    let bytes = transcript.transcript_bytes()?;
    crypto
        .ed25519_verify(&bytes, signature, public_key)
        .await
        .map_err(|error| AuthenticationError::CryptoError {
            details: format!("Ed25519 transcript verification failed: {error}"),
        })
}

/// Verify a FROST aggregate signature over a typed transcript.
pub async fn verify_frost_transcript<E, T>(
    crypto: &E,
    transcript: &T,
    signature: &[u8],
    group_public_key: &[u8],
) -> Result<bool>
where
    E: CryptoEffects + Send + Sync + ?Sized,
    T: SecurityTranscript + ?Sized,
{
    let bytes = transcript.transcript_bytes()?;
    crypto
        .frost_verify(&bytes, signature, group_public_key)
        .await
        .map_err(|error| AuthenticationError::CryptoError {
            details: format!("FROST transcript verification failed: {error}"),
        })
}

/// Verify a FROST aggregate signature over a threshold signing context transcript.
pub async fn verify_threshold_signing_context_transcript<E>(
    crypto: &E,
    context: &SigningContext,
    epoch: u64,
    signature: &[u8],
    group_public_key: &[u8],
) -> Result<bool>
where
    E: CryptoEffects + Send + Sync + ?Sized,
{
    let bytes = threshold_signing_context_transcript_bytes(context, epoch)?;
    crypto
        .frost_verify(&bytes, signature, group_public_key)
        .await
        .map_err(|error| AuthenticationError::CryptoError {
            details: format!("FROST signing-context transcript verification failed: {error}"),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::threshold::{ApprovalContext, SignableOperation, SigningContext};
    use aura_core::AuthorityId;
    use serde::Serialize;

    #[derive(Debug, Clone, PartialEq, Eq, Serialize)]
    struct TestPayload {
        signer: String,
        context: String,
        nonce: Vec<u8>,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct TestTranscript {
        signer: String,
        context: String,
        nonce: Vec<u8>,
    }

    impl SecurityTranscript for TestTranscript {
        type Payload = TestPayload;

        const DOMAIN_SEPARATOR: &'static str = "aura.test.transcript";

        fn transcript_payload(&self) -> Self::Payload {
            TestPayload {
                signer: self.signer.clone(),
                context: self.context.clone(),
                nonce: self.nonce.clone(),
            }
        }
    }

    #[test]
    fn transcript_bytes_are_canonical() {
        let transcript = TestTranscript {
            signer: "alice".to_string(),
            context: "ctx".to_string(),
            nonce: vec![1, 2, 3],
        };

        assert_eq!(
            transcript.transcript_bytes().unwrap(),
            transcript.transcript_bytes().unwrap()
        );
    }

    #[test]
    fn domain_separator_changes_signature_input() {
        let payload = TestPayload {
            signer: "alice".to_string(),
            context: "ctx".to_string(),
            nonce: vec![1, 2, 3],
        };

        let left = encode_transcript("aura.left", 1, &payload).unwrap();
        let right = encode_transcript("aura.right", 1, &payload).unwrap();

        assert_ne!(left, right);
    }

    #[test]
    fn schema_version_changes_signature_input() {
        let payload = TestPayload {
            signer: "alice".to_string(),
            context: "ctx".to_string(),
            nonce: vec![1, 2, 3],
        };

        let v1 = encode_transcript("aura.test", 1, &payload).unwrap();
        let v2 = encode_transcript("aura.test", 2, &payload).unwrap();

        assert_ne!(v1, v2);
    }

    #[test]
    fn empty_domain_separator_fails_closed() {
        let payload = TestPayload {
            signer: "alice".to_string(),
            context: "ctx".to_string(),
            nonce: vec![1, 2, 3],
        };

        assert!(encode_transcript("", 1, &payload).is_err());
    }

    #[test]
    fn threshold_context_transcript_binds_epoch() {
        let context = SigningContext {
            authority: AuthorityId::new_from_entropy([7; 32]),
            operation: SignableOperation::Message {
                domain: "test".to_string(),
                payload: vec![1, 2, 3],
            },
            approval_context: ApprovalContext::SelfOperation,
        };

        let epoch_1 = threshold_signing_context_transcript_bytes(&context, 1).unwrap();
        let epoch_2 = threshold_signing_context_transcript_bytes(&context, 2).unwrap();

        assert_ne!(epoch_1, epoch_2);
    }
}
