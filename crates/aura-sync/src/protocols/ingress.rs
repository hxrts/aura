//! Sync-local helpers for routing peer data through the guard-owned ingress typestate.

use aura_core::{util::serialization, AuraError, ContextId, DeviceId, Hash32};
use aura_guards::{
    DecodedIngress, IngressSource, IngressVerificationEvidence, VerifiedIngress,
    VerifiedIngressMetadata,
};
use serde::Serialize;

/// Create verified ingress evidence for an authority-scoped sync payload after
/// the protocol-specific caller has performed its current peer and namespace
/// checks.
pub(crate) fn verified_authority_payload<T: Serialize>(
    source: aura_core::AuthorityId,
    context: ContextId,
    schema_version: u16,
    payload: T,
) -> Result<VerifiedIngress<T>, AuraError> {
    verified_payload(
        IngressSource::Authority(source),
        context,
        schema_version,
        payload,
    )
}

#[cfg(test)]
pub(crate) fn verified_authority_payload_with_hash<T>(
    source: aura_core::AuthorityId,
    context: ContextId,
    schema_version: u16,
    payload_hash: Hash32,
    payload: T,
) -> Result<VerifiedIngress<T>, AuraError> {
    verified_payload_with_hash(
        IngressSource::Authority(source),
        context,
        schema_version,
        payload_hash,
        payload,
    )
}

/// Create verified ingress evidence for a device-scoped sync payload after the
/// protocol-specific caller has performed its current peer and namespace checks.
pub(crate) fn verified_device_payload<T: Serialize>(
    source: DeviceId,
    context: ContextId,
    schema_version: u16,
    payload: T,
) -> Result<VerifiedIngress<T>, AuraError> {
    verified_payload(
        IngressSource::Device(source),
        context,
        schema_version,
        payload,
    )
}

fn verified_payload<T: Serialize>(
    source: IngressSource,
    context: ContextId,
    schema_version: u16,
    payload: T,
) -> Result<VerifiedIngress<T>, AuraError> {
    let bytes = serialization::to_vec(&payload)
        .map_err(|err| AuraError::invalid(format!("serialize sync ingress payload: {err}")))?;
    verified_payload_with_hash(
        source,
        context,
        schema_version,
        Hash32::from_bytes(&bytes),
        payload,
    )
}

fn verified_payload_with_hash<T>(
    source: IngressSource,
    context: ContextId,
    schema_version: u16,
    payload_hash: Hash32,
    payload: T,
) -> Result<VerifiedIngress<T>, AuraError> {
    let metadata =
        VerifiedIngressMetadata::new(source, context, None, payload_hash, schema_version);
    let evidence = IngressVerificationEvidence::builder(metadata)
        .peer_identity(true, "sync caller supplied authenticated peer source")
        .and_then(|builder| {
            builder.envelope_authenticity(
                payload_hash != Hash32::zero(),
                "canonical payload hash must be non-empty",
            )
        })
        .and_then(|builder| {
            builder.capability_authorization(true, "sync caller completed guard authorization")
        })
        .and_then(|builder| builder.namespace_scope(true, "sync caller supplied scoped context"))
        .and_then(|builder| builder.schema_version(schema_version == 1, "unsupported sync schema"))
        .and_then(|builder| builder.replay_freshness(true, "sync caller supplied fresh peer batch"))
        .and_then(|builder| {
            builder.signer_membership(true, "sync reducers validate signer membership per fact")
        })
        .and_then(|builder| {
            builder.proof_evidence(true, "sync reducers validate fact and receipt proofs")
        })
        .and_then(|builder| builder.build())
        .map_err(|err| AuraError::invalid(format!("build sync ingress evidence: {err}")))?;

    DecodedIngress::new(payload, evidence.metadata().clone())
        .verify(evidence)
        .map_err(|err| AuraError::invalid(format!("promote sync ingress: {err}")))
}
