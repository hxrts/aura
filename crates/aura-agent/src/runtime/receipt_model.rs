//! Runtime-owned flow and transport receipt signing.

use aura_core::effects::transport::{
    TransportEnvelope, TransportError, TransportReceipt, MAX_TRANSPORT_SIGNATURE_BYTES,
};
use aura_core::{AuraError, Ed25519Signature, Ed25519SigningKey, Ed25519VerifyingKey, Hash32};

const RECEIPT_SIGNATURE_MAGIC: &[u8] = b"AURA-RECEIPT-V1\0";
const FLOW_SCOPE: u8 = 1;
const TRANSPORT_SCOPE: u8 = 2;
const PUBLIC_KEY_BYTES: usize = 32;
const SIGNATURE_BYTES: usize = 64;
const SIGNATURE_BLOB_BYTES: usize =
    RECEIPT_SIGNATURE_MAGIC.len() + 1 + PUBLIC_KEY_BYTES + SIGNATURE_BYTES;

pub(crate) fn sign_flow_receipt(
    receipt: &mut aura_core::Receipt,
    signing_key: &Ed25519SigningKey,
) -> Result<(), AuraError> {
    let mut transcript = flow_receipt_transcript(
        &receipt.ctx.to_bytes(),
        &receipt.src.to_bytes(),
        &receipt.dst.to_bytes(),
        receipt.epoch.value(),
        receipt.cost.value(),
        receipt.nonce.value(),
        &receipt.prev.0,
    );
    append_scope(&mut transcript, FLOW_SCOPE);
    receipt.sig = aura_core::ReceiptSig::new(sign_blob(FLOW_SCOPE, signing_key, &transcript)?)?;
    Ok(())
}

#[cfg(test)]
pub(crate) fn sign_transport_flow_receipt(
    receipt: &mut TransportReceipt,
    signing_key: &Ed25519SigningKey,
) -> Result<(), TransportError> {
    let transcript = transport_receipt_transcript(receipt, FLOW_SCOPE, None);
    receipt.sig =
        sign_blob(FLOW_SCOPE, signing_key, &transcript).map_err(transport_crypto_error)?;
    Ok(())
}

pub(crate) fn verify_transport_flow_receipt(
    receipt: &TransportReceipt,
) -> Result<(), TransportError> {
    let transcript = transport_receipt_transcript(receipt, FLOW_SCOPE, None);
    verify_blob(FLOW_SCOPE, &receipt.sig, &transcript)
}

pub(crate) fn sign_transport_receipt_for_envelope(
    receipt: &mut TransportReceipt,
    envelope: &TransportEnvelope,
    signing_key: &Ed25519SigningKey,
) -> Result<(), TransportError> {
    let transcript = transport_receipt_transcript(receipt, TRANSPORT_SCOPE, Some(envelope));
    receipt.sig =
        sign_blob(TRANSPORT_SCOPE, signing_key, &transcript).map_err(transport_crypto_error)?;
    Ok(())
}

pub(crate) fn verify_transport_receipt_for_envelope(
    receipt: &TransportReceipt,
    envelope: &TransportEnvelope,
) -> Result<(), TransportError> {
    if receipt.context != envelope.context
        || receipt.src != envelope.source
        || receipt.dst != envelope.destination
    {
        return Err(TransportError::ReceiptValidationFailed {
            reason: "receipt route does not match envelope route".to_string(),
        });
    }
    let transcript = transport_receipt_transcript(receipt, TRANSPORT_SCOPE, Some(envelope));
    verify_blob(TRANSPORT_SCOPE, &receipt.sig, &transcript)
}

fn sign_blob(
    scope: u8,
    signing_key: &Ed25519SigningKey,
    transcript: &[u8],
) -> Result<Vec<u8>, AuraError> {
    let verifying_key = signing_key.verifying_key()?;
    let signature = signing_key.sign(transcript)?;
    let mut blob = Vec::with_capacity(SIGNATURE_BLOB_BYTES);
    blob.extend_from_slice(RECEIPT_SIGNATURE_MAGIC);
    blob.push(scope);
    blob.extend_from_slice(verifying_key.as_bytes());
    blob.extend_from_slice(signature.as_bytes());
    Ok(blob)
}

fn verify_blob(scope: u8, blob: &[u8], transcript: &[u8]) -> Result<(), TransportError> {
    if blob.len() > MAX_TRANSPORT_SIGNATURE_BYTES {
        return Err(TransportError::ReceiptValidationFailed {
            reason: "receipt signature exceeds transport limit".to_string(),
        });
    }
    if blob.len() != SIGNATURE_BLOB_BYTES {
        return Err(TransportError::ReceiptValidationFailed {
            reason: "receipt signature has invalid format".to_string(),
        });
    }
    if !blob.starts_with(RECEIPT_SIGNATURE_MAGIC) {
        return Err(TransportError::ReceiptValidationFailed {
            reason: "receipt signature magic mismatch".to_string(),
        });
    }
    if blob[RECEIPT_SIGNATURE_MAGIC.len()] != scope {
        return Err(TransportError::ReceiptValidationFailed {
            reason: "receipt signature scope mismatch".to_string(),
        });
    }

    let key_start = RECEIPT_SIGNATURE_MAGIC.len() + 1;
    let sig_start = key_start + PUBLIC_KEY_BYTES;
    let verifying_key = Ed25519VerifyingKey::try_from_slice(&blob[key_start..sig_start])
        .map_err(transport_crypto_error)?;
    let signature =
        Ed25519Signature::try_from_slice(&blob[sig_start..]).map_err(transport_crypto_error)?;
    verifying_key
        .verify(transcript, &signature)
        .map_err(transport_crypto_error)
}

fn flow_receipt_transcript(
    context: &[u8; 16],
    src: &[u8; 16],
    dst: &[u8; 16],
    epoch: u64,
    cost: u32,
    nonce: u64,
    prev: &[u8; 32],
) -> Vec<u8> {
    let mut transcript = Vec::with_capacity(128);
    transcript.extend_from_slice(b"aura:flow-receipt:v1");
    transcript.extend_from_slice(context);
    transcript.extend_from_slice(src);
    transcript.extend_from_slice(dst);
    transcript.extend_from_slice(&epoch.to_be_bytes());
    transcript.extend_from_slice(&cost.to_be_bytes());
    transcript.extend_from_slice(&nonce.to_be_bytes());
    transcript.extend_from_slice(prev);
    transcript
}

fn transport_receipt_transcript(
    receipt: &TransportReceipt,
    scope: u8,
    envelope: Option<&TransportEnvelope>,
) -> Vec<u8> {
    let mut transcript = flow_receipt_transcript(
        &receipt.context.to_bytes(),
        &receipt.src.to_bytes(),
        &receipt.dst.to_bytes(),
        receipt.epoch,
        receipt.cost,
        receipt.nonce,
        &receipt.prev,
    );
    append_scope(&mut transcript, scope);
    if let Some(envelope) = envelope {
        transcript.extend_from_slice(b":payload:");
        transcript.extend_from_slice(Hash32::from_bytes(&envelope.payload).as_bytes());
        append_metadata_value(&mut transcript, envelope, "content-type");
        append_metadata_value(&mut transcript, envelope, "wire-format-version");
    }
    transcript
}

fn append_scope(transcript: &mut Vec<u8>, scope: u8) {
    transcript.extend_from_slice(b":scope:");
    transcript.push(scope);
}

fn append_metadata_value(
    transcript: &mut Vec<u8>,
    envelope: &TransportEnvelope,
    key: &'static str,
) {
    transcript.extend_from_slice(key.as_bytes());
    match envelope.metadata.get(key) {
        Some(value) => {
            transcript.extend_from_slice(&(value.len() as u32).to_be_bytes());
            transcript.extend_from_slice(value.as_bytes());
        }
        None => transcript.extend_from_slice(&0u32.to_be_bytes()),
    }
}

fn transport_crypto_error(error: AuraError) -> TransportError {
    TransportError::ReceiptValidationFailed {
        reason: format!("receipt signature verification failed: {error}"),
    }
}

#[cfg(test)]
pub(crate) fn test_receipt_signing_key() -> Ed25519SigningKey {
    Ed25519SigningKey::from_bytes([42u8; 32])
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::{AuthorityId, ContextId};
    use std::collections::HashMap;

    fn authority(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    fn context(seed: u8) -> ContextId {
        ContextId::new_from_entropy([seed; 32])
    }

    fn envelope() -> TransportEnvelope {
        let mut metadata = HashMap::new();
        metadata.insert(
            "content-type".to_string(),
            "application/aura-test".to_string(),
        );
        TransportEnvelope {
            destination: authority(2),
            source: authority(1),
            context: context(3),
            payload: b"payload".to_vec(),
            metadata,
            receipt: None,
        }
    }

    fn receipt_for(envelope: &TransportEnvelope) -> TransportReceipt {
        TransportReceipt {
            context: envelope.context,
            src: envelope.source,
            dst: envelope.destination,
            epoch: 1,
            cost: 1,
            nonce: 9,
            prev: [0; 32],
            sig: Vec::new(),
        }
    }

    #[test]
    fn envelope_receipt_signature_verifies() {
        let envelope = envelope();
        let mut receipt = receipt_for(&envelope);

        sign_transport_receipt_for_envelope(&mut receipt, &envelope, &test_receipt_signing_key())
            .unwrap();

        assert!(verify_transport_receipt_for_envelope(&receipt, &envelope).is_ok());
    }

    #[test]
    fn envelope_receipt_rejects_tampered_signature() {
        let envelope = envelope();
        let mut receipt = receipt_for(&envelope);
        sign_transport_receipt_for_envelope(&mut receipt, &envelope, &test_receipt_signing_key())
            .unwrap();
        let last = receipt.sig.len() - 1;
        receipt.sig[last] ^= 0x55;

        assert!(verify_transport_receipt_for_envelope(&receipt, &envelope).is_err());
    }

    #[test]
    fn envelope_receipt_rejects_tampered_payload() {
        let mut envelope = envelope();
        let mut receipt = receipt_for(&envelope);
        sign_transport_receipt_for_envelope(&mut receipt, &envelope, &test_receipt_signing_key())
            .unwrap();
        envelope.payload.extend_from_slice(b"-modified");

        assert!(verify_transport_receipt_for_envelope(&receipt, &envelope).is_err());
    }

    #[test]
    fn envelope_receipt_rejects_route_mismatch() {
        let mut envelope = envelope();
        let mut receipt = receipt_for(&envelope);
        sign_transport_receipt_for_envelope(&mut receipt, &envelope, &test_receipt_signing_key())
            .unwrap();
        envelope.destination = authority(9);

        assert!(verify_transport_receipt_for_envelope(&receipt, &envelope).is_err());
    }
}
