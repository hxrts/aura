//! Pre-adoption mailbox upkeep under the actual admitted enrollment owner.
//! The broker only holds opaque guarded VM frames; its receipt is never a
//! substitute for local runtime intake or the final VM verifier.

use super::AuraAgent;
use crate::core::{AgentError, AgentResult};
use crate::runtime::services::bootstrap_broker::BootstrapPendingEnrollmentMailboxRegistration;
use aura_core::effects::transport::TransportEnvelope;
use aura_core::effects::{
    PhysicalTimeEffects, RandomCoreEffects, SecureStorageCapability, SecureStorageEffects,
    SecureStorageLocation,
};
use aura_core::hash::hash;
use aura_core::AuraError;
use aura_invitation::enrollment_mailbox::PendingEnrollmentMailboxRegistration;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

const FRAME_NAMESPACE: &str = "pending_enrollment_mailbox_frame_v1";
const MAX_RETAINED_FRAMES_PER_ADMISSION: usize = 64;
const MAX_RETAINED_FRAMES_TOTAL: usize = 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RetainedMailboxFrame {
    manifest_digest: [u8; 32],
    message_id: String,
    envelope: TransportEnvelope,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RetainedMailboxCredential {
    pub(crate) credential: String,
    registration: PendingEnrollmentMailboxRegistration,
}

pub(crate) fn retained_location(
    admitted: &crate::handlers::invitation::enrollment_manifest_admission::AdmittedEnrollmentManifest,
) -> SecureStorageLocation {
    let manifest = admitted.manifest();
    let digest = admitted.manifest_digest();
    let suffix = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    SecureStorageLocation::new(
        "pending_enrollment_mailbox_credential_v1",
        format!(
            "{}:{}:{}",
            manifest.invitee_authority, manifest.invitation, suffix
        ),
    )
}

fn boundary<E: std::error::Error + Send + Sync + 'static>(source: E) -> AgentError {
    AgentError::from(AuraError::Internal {
        message: "pending enrollment mailbox owner failed".into(),
        source: Some(Arc::new(source)),
    })
}

fn frame_prefix(digest: [u8; 32]) -> String {
    format!("{}:", hex::encode(digest))
}

pub(crate) fn frame_location(digest: [u8; 32], message_id: &str) -> SecureStorageLocation {
    SecureStorageLocation::new(
        FRAME_NAMESPACE,
        format!("{}{}", frame_prefix(digest), message_id),
    )
}

fn verify_frame(
    admitted: &crate::handlers::invitation::enrollment_manifest_admission::AdmittedEnrollmentManifest,
    record: &RetainedMailboxFrame,
) -> AgentResult<()> {
    let manifest = admitted.manifest();
    let exact_session = record
        .envelope
        .metadata
        .get("session-id")
        .and_then(|raw| uuid::Uuid::parse_str(raw).ok())
        .is_some_and(|session| {
            aura_invitation::enrollment_mailbox_session::is_original_enrollment_mailbox_session(
                &manifest.invitation,
                &admitted.manifest_digest(),
                session,
            )
        });
    if record.manifest_digest != admitted.manifest_digest()
        || uuid::Uuid::parse_str(&record.message_id).is_err()
        || record.envelope.source != manifest.subject
        || record.envelope.destination != manifest.subject
        || record.envelope.metadata.get("aura-destination-device-id")
            != Some(&manifest.invitee_device.to_string())
        || record.envelope.metadata.get("aura-message-id") != Some(&record.message_id)
        || record
            .envelope
            .metadata
            .get("content-type")
            .is_none_or(|kind| kind != "application/aura-choreography")
        || !exact_session
        || record.envelope.receipt.as_ref().is_none_or(|receipt| {
            receipt.src != record.envelope.source
                || receipt.dst != record.envelope.destination
                || receipt.context != record.envelope.context
        })
    {
        return Err(AgentError::from(AuraError::invalid(
            "retained pending mailbox frame differs from protected original admission",
        )));
    }
    Ok(())
}

/// Check exact encrypted delivery custody after the receiver has retained its
/// verified terminal and acknowledged the original required task. The terminal
/// stops replay; physical retirement waits for the provider-sealed expiry.
pub(crate) async fn verify_retained_mailbox_frames_after_terminal(
    effects: &crate::runtime::AuraEffectSystem,
    admitted: &crate::handlers::invitation::enrollment_manifest_admission::AdmittedEnrollmentManifest,
) -> AgentResult<()> {
    let prefix = frame_prefix(admitted.manifest_digest());
    let keys = effects
        .secure_list_keys(FRAME_NAMESPACE, &[SecureStorageCapability::List])
        .await
        .map_err(boundary)?;
    if keys.len() > MAX_RETAINED_FRAMES_TOTAL {
        return Err(AgentError::from(AuraError::invalid(
            "retained pending mailbox frame inventory exceeded before terminal cleanup",
        )));
    }
    for key in keys.into_iter().filter(|key| key.starts_with(&prefix)) {
        let location = SecureStorageLocation::new(FRAME_NAMESPACE, &key);
        let bytes = effects
            .secure_retrieve(&location, &[SecureStorageCapability::Read])
            .await
            .map_err(boundary)?;
        let record: RetainedMailboxFrame =
            aura_core::util::serialization::from_slice(&bytes).map_err(boundary)?;
        if frame_location(record.manifest_digest, &record.message_id).key != key {
            return Err(AgentError::from(AuraError::invalid(
                "terminal mailbox custody key changed",
            )));
        }
        verify_frame(admitted, &record)?;
    }
    Ok(())
}

impl AuraAgent {
    fn queue_retained_mailbox_frame_once(
        &self,
        key: &str,
        envelope: TransportEnvelope,
    ) -> AgentResult<bool> {
        let mut seen = self.pending_mailbox_seen.lock();
        if seen.contains(key) {
            return Ok(false);
        }
        if !matches!(
            self.runtime().effects().queue_runtime_envelope(envelope),
            crate::runtime::subsystems::transport::QueueEnvelopeOutcome::Queued
        ) {
            return Err(AgentError::from(AuraError::invalid(
                "pending enrollment runtime intake refused original frame",
            )));
        }
        seen.insert(key.to_owned());
        Ok(true)
    }

    async fn replay_retained_mailbox_frames(
        &self,
        admitted: &crate::handlers::invitation::enrollment_manifest_admission::AdmittedEnrollmentManifest,
        keys: &[String],
    ) -> AgentResult<usize> {
        let effects = self.runtime().effects();
        let prefix = frame_prefix(admitted.manifest_digest());
        let scoped: Vec<_> = keys.iter().filter(|key| key.starts_with(&prefix)).collect();
        if scoped.len() > MAX_RETAINED_FRAMES_PER_ADMISSION {
            return Err(AgentError::from(AuraError::invalid(
                "retained pending mailbox frame capacity exceeded",
            )));
        }
        let mut replayed = 0;
        for key in scoped {
            let bytes = effects
                .secure_retrieve(
                    &SecureStorageLocation::new(FRAME_NAMESPACE, key),
                    &[SecureStorageCapability::Read],
                )
                .await
                .map_err(boundary)?;
            let record: RetainedMailboxFrame =
                aura_core::util::serialization::from_slice(&bytes).map_err(boundary)?;
            if frame_location(record.manifest_digest, &record.message_id).key != key.as_str() {
                return Err(AgentError::from(AuraError::invalid(
                    "retained pending mailbox frame key differs from original message",
                )));
            }
            verify_frame(admitted, &record)?;
            replayed += usize::from(self.queue_retained_mailbox_frame_once(key, record.envelope)?);
        }
        Ok(replayed)
    }

    /// Register an exact setup-key mailbox and admit its frames into the
    /// existing runtime queue. Immutable encrypted custody precedes broker ACK;
    /// a same-profile reopen replays that custody before using the broker.
    pub async fn drain_pending_enrollment_vm_mailbox(&self) -> AgentResult<usize> {
        let effects = self.runtime().effects();
        effects.retain_original_return_signer(
            self.threshold_signing().downgrade_for_original_return(),
        );
        let manager = effects
            .rendezvous_manager()
            .filter(|manager| manager.config().bootstrap_broker.enabled);
        let admissions = crate::handlers::invitation::enrollment_manifest_admission::load_recoverable_negative_admissions(effects.as_ref())
            .await.map_err(boundary)?;
        let mut retained_keys = effects
            .secure_list_keys(FRAME_NAMESPACE, &[SecureStorageCapability::List])
            .await
            .map_err(boundary)?;
        if retained_keys.len() > MAX_RETAINED_FRAMES_TOTAL {
            return Err(AgentError::from(AuraError::invalid(
                "retained pending mailbox frame inventory exceeded",
            )));
        }
        let mut admitted_frames = 0;
        for admitted in admissions {
            let now_ms = effects.physical_time().await.map_err(boundary)?.ts_ms;
            if now_ms >= admitted.manifest().expires_at_ms {
                continue;
            }
            admitted_frames += self
                .replay_retained_mailbox_frames(&admitted, &retained_keys)
                .await?;
            let Some(manager) = manager.as_ref() else {
                continue;
            };
            let location = retained_location(&admitted);
            let retained = if effects.secure_exists(&location).await.map_err(boundary)? {
                let bytes = effects
                    .secure_retrieve(&location, &[SecureStorageCapability::Read])
                    .await
                    .map_err(boundary)?;
                aura_core::util::serialization::from_slice::<RetainedMailboxCredential>(&bytes)
                    .map_err(boundary)?
            } else {
                let secret = effects.random_bytes(32).await;
                if secret.len() != 32 {
                    return Err(AgentError::from(AuraError::invalid(
                        "pending mailbox credential source returned invalid length",
                    )));
                }
                let credential = secret
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>();
                let registration = self
                    .threshold_signing()
                    .sign_pending_enrollment_mailbox_registration(
                        admitted.baseline(),
                        admitted.local_setup(),
                        hash(credential.as_bytes()),
                    )
                    .await
                    .map_err(boundary)?;
                let candidate = RetainedMailboxCredential {
                    credential,
                    registration,
                };
                let bytes = aura_core::util::serialization::to_vec(&candidate).map_err(boundary)?;
                effects
                    .secure_store_immutable(
                        &location,
                        &bytes,
                        &[
                            SecureStorageCapability::Read,
                            SecureStorageCapability::Write,
                            SecureStorageCapability::Delete,
                        ],
                    )
                    .await
                    .map_err(boundary)?;
                let retained_bytes = effects
                    .secure_retrieve(&location, &[SecureStorageCapability::Read])
                    .await
                    .map_err(boundary)?;
                aura_core::util::serialization::from_slice::<RetainedMailboxCredential>(
                    &retained_bytes,
                )
                .map_err(boundary)?
            };
            retained
                .registration
                .clone()
                .verify_for_admitted_original(
                    effects.as_ref(),
                    admitted.baseline(),
                    admitted.local_setup(),
                    now_ms,
                )
                .await
                .map_err(boundary)?;
            if retained
                .registration
                .statement
                .retrieval_credential_commitment()
                != hash(retained.credential.as_bytes())
            {
                return Err(AgentError::from(AuraError::invalid(
                    "retained mailbox credential changed",
                )));
            }
            manager
                .register_original_pending_mailbox(BootstrapPendingEnrollmentMailboxRegistration {
                    registration: retained.registration,
                    retrieval_credential: retained.credential.clone(),
                })
                .await
                .map_err(boundary)?;
            let manifest = admitted.manifest();
            let deliveries = manager
                .take_original_pending_mailbox(
                    manifest.subject,
                    manifest.invitee_device,
                    admitted.manifest_digest(),
                    &retained.credential,
                )
                .await
                .map_err(boundary)?;
            for delivery in deliveries {
                let record = RetainedMailboxFrame {
                    manifest_digest: admitted.manifest_digest(),
                    message_id: delivery.message_id.clone(),
                    envelope: delivery.envelope,
                };
                verify_frame(&admitted, &record)?;
                let location = frame_location(record.manifest_digest, &record.message_id);
                let is_new = !retained_keys.iter().any(|key| key == &location.key);
                let count = retained_keys
                    .iter()
                    .filter(|key| key.starts_with(&frame_prefix(record.manifest_digest)))
                    .count();
                if is_new
                    && (count >= MAX_RETAINED_FRAMES_PER_ADMISSION
                        || retained_keys.len() >= MAX_RETAINED_FRAMES_TOTAL)
                {
                    return Err(AgentError::from(AuraError::invalid(
                        "retained pending mailbox frame capacity exceeded",
                    )));
                }
                let bytes = aura_core::util::serialization::to_vec(&record).map_err(boundary)?;
                effects
                    .secure_store_immutable(
                        &location,
                        &bytes,
                        &[
                            SecureStorageCapability::Read,
                            SecureStorageCapability::Write,
                        ],
                    )
                    .await
                    .map_err(boundary)?;
                let retained_bytes = effects
                    .secure_retrieve(&location, &[SecureStorageCapability::Read])
                    .await
                    .map_err(boundary)?;
                if retained_bytes != bytes {
                    return Err(AgentError::from(AuraError::invalid(
                        "broker redelivery changed an immutable original frame",
                    )));
                }
                if is_new {
                    retained_keys.push(location.key.clone());
                }
                admitted_frames += usize::from(
                    self.queue_retained_mailbox_frame_once(&location.key, record.envelope)?,
                );
                manager
                    .acknowledge_original_pending_mailbox(
                        manifest.subject,
                        manifest.invitee_device,
                        admitted.manifest_digest(),
                        &retained.credential,
                        &delivery.message_id,
                    )
                    .await
                    .map_err(boundary)?;
            }
        }
        Ok(admitted_frames)
    }
}
