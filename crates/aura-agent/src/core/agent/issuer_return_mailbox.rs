//! Alice's protected custody for the temporary Bob-to-Alice VM mailbox.
//! Broker bytes are untrusted until the original issued artifact, setup proof,
//! return registration, and exact per-frame signature are checked locally.

use super::AuraAgent;
use crate::core::{AgentError, AgentResult};
use crate::handlers::invitation::enrollment_trust::RequiredIssuedEnrollmentSelectorCapability;
use crate::runtime::services::bootstrap_broker::IssuerReturnMailboxDelivery;
use aura_core::effects::{
    CryptoExtendedEffects, PhysicalTimeEffects, SecureStorageCapability, SecureStorageEffects,
    SecureStorageLocation,
};
use aura_core::AuraError;
use aura_invitation::enrollment_mailbox::{
    IssuerEnrollmentReturnRegistration, IssuerEnrollmentReturnStatement,
};
use aura_invitation::enrollment_setup::VerifiedEnrollmentSetupPossession;
use aura_signature::SecurityTranscript;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

const REGISTRATION_NAMESPACE: &str = "issuer_return_mailbox_registration_v1";
const CREDENTIAL_NAMESPACE: &str = "issuer_return_mailbox_credential_v1";
const SETUP_NAMESPACE: &str = "issuer_return_mailbox_setup_v1";
const FRAME_NAMESPACE: &str = "issuer_return_mailbox_frame_v1";
const RETIREMENT_ROOT_NAMESPACE: &str = "issuer_return_mailbox_retirement_root_v1";
const RETIREMENT_LEAF_NAMESPACE: &str = "issuer_return_mailbox_retirement_leaf_v1";
const MAX_REGISTRATIONS: usize = 1024;
const MAX_RETAINED_FRAMES: usize = 1024;
const MAX_FRAMES_PER_REGISTRATION: usize = 64;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RetainedReturnFrame {
    manifest_digest: [u8; 32],
    delivery: IssuerReturnMailboxDelivery,
}

/// Immutable inventory for a future profile-owned retirement operation. This
/// record cannot retire anything: it only fixes the original protected bytes
/// and signed expiry before the broker route can be published.
#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct IssuerReturnRetirementRoot {
    statement: IssuerEnrollmentReturnStatement,
    registration_digest: [u8; 32],
    credential_digest: [u8; 32],
    setup_record_digest: [u8; 32],
}

impl IssuerReturnRetirementRoot {
    fn new(
        statement: &IssuerEnrollmentReturnStatement,
        registration: &[u8],
        credential: &[u8],
        setup: &[u8],
    ) -> Self {
        Self {
            statement: statement.clone(),
            registration_digest: aura_core::hash::hash(registration),
            credential_digest: aura_core::hash::hash(credential),
            setup_record_digest: aura_core::hash::hash(setup),
        }
    }

    pub(crate) fn verifies(
        &self,
        statement: &IssuerEnrollmentReturnStatement,
        registration: &[u8],
        credential: &[u8],
        setup: &[u8],
    ) -> bool {
        self.statement == *statement
            && self.registration_digest == aura_core::hash::hash(registration)
            && self.credential_digest == aura_core::hash::hash(credential)
            && self.setup_record_digest == aura_core::hash::hash(setup)
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct IssuerReturnRetirementLeaf {
    root_digest: [u8; 32],
    frame_key: String,
    frame_digest: [u8; 32],
}

impl IssuerReturnRetirementLeaf {
    pub(crate) fn verifies(&self, root_digest: [u8; 32], frame_key: &str, frame: &[u8]) -> bool {
        self.root_digest == root_digest
            && self.frame_key == frame_key
            && self.frame_digest == aura_core::hash::hash(frame)
    }
}

pub(crate) fn retirement_root_location(
    statement: &IssuerEnrollmentReturnStatement,
) -> SecureStorageLocation {
    owner_location(RETIREMENT_ROOT_NAMESPACE, statement)
}

pub(crate) fn retirement_leaf_location(frame_key: &str) -> SecureStorageLocation {
    SecureStorageLocation::new(RETIREMENT_LEAF_NAMESPACE, frame_key)
}

pub(crate) async fn retain_original_return_retirement_root(
    effects: &crate::runtime::AuraEffectSystem,
    statement: &IssuerEnrollmentReturnStatement,
    registration: &[u8],
    credential: &[u8],
    setup: &[u8],
) -> Result<(), AuraError> {
    let root = IssuerReturnRetirementRoot::new(statement, registration, credential, setup);
    let encoded = aura_core::util::serialization::to_vec(&root)?;
    let location = retirement_root_location(statement);
    effects
        .secure_store_immutable(
            &location,
            &encoded,
            &[
                SecureStorageCapability::Read,
                SecureStorageCapability::Write,
            ],
        )
        .await?;
    let original = effects
        .secure_retrieve(&location, &[SecureStorageCapability::Read])
        .await?;
    if original != encoded || !root.verifies(statement, registration, credential, setup) {
        return Err(AuraError::permission_denied(
            "original return retirement inventory differs from protected publication",
        ));
    }
    Ok(())
}

async fn load_retirement_root(
    effects: &crate::runtime::AuraEffectSystem,
    statement: &IssuerEnrollmentReturnStatement,
    registration: &[u8],
    credential: &[u8],
    setup: &[u8],
) -> AgentResult<[u8; 32]> {
    let bytes = effects
        .secure_retrieve(
            &retirement_root_location(statement),
            &[SecureStorageCapability::Read],
        )
        .await
        .map_err(boundary)?;
    let root: IssuerReturnRetirementRoot =
        aura_core::util::serialization::from_slice(&bytes).map_err(boundary)?;
    if !root.verifies(statement, registration, credential, setup) {
        return Err(AgentError::from(AuraError::permission_denied(
            "original return retirement root differs from protected records",
        )));
    }
    Ok(aura_core::hash::hash(&bytes))
}

async fn retain_retirement_leaf(
    effects: &crate::runtime::AuraEffectSystem,
    root_digest: [u8; 32],
    frame_key: &str,
    frame: &[u8],
) -> AgentResult<()> {
    let leaf = IssuerReturnRetirementLeaf {
        root_digest,
        frame_key: frame_key.to_owned(),
        frame_digest: aura_core::hash::hash(frame),
    };
    let encoded = aura_core::util::serialization::to_vec(&leaf).map_err(boundary)?;
    let location = retirement_leaf_location(frame_key);
    effects
        .secure_store_immutable(
            &location,
            &encoded,
            &[
                SecureStorageCapability::Read,
                SecureStorageCapability::Write,
            ],
        )
        .await
        .map_err(boundary)?;
    let original = effects
        .secure_retrieve(&location, &[SecureStorageCapability::Read])
        .await
        .map_err(boundary)?;
    if original != encoded || !leaf.verifies(root_digest, frame_key, frame) {
        return Err(AgentError::from(AuraError::permission_denied(
            "original return frame retirement leaf differs from protected frame",
        )));
    }
    Ok(())
}

fn boundary(source: impl std::error::Error + Send + Sync + 'static) -> AgentError {
    AgentError::from(AuraError::Internal {
        message: "issuer return mailbox owner failed".into(),
        source: Some(Arc::new(source)),
    })
}

fn owner_location(
    namespace: &str,
    statement: &IssuerEnrollmentReturnStatement,
) -> SecureStorageLocation {
    let record = format!(
        "{}:{}",
        statement.invitation(),
        hex::encode(statement.manifest_digest())
    );
    if namespace == REGISTRATION_NAMESPACE {
        SecureStorageLocation::new(namespace, format!("{}:{record}", statement.subject()))
    } else {
        SecureStorageLocation::with_sub_key(namespace, statement.subject().to_string(), record)
    }
}

fn frame_prefix(digest: [u8; 32]) -> String {
    format!("{}:", hex::encode(digest))
}

fn frame_location(digest: [u8; 32], message_id: &str) -> SecureStorageLocation {
    SecureStorageLocation::new(
        FRAME_NAMESPACE,
        format!("{}{}", frame_prefix(digest), message_id),
    )
}

async fn verify_original_owner(
    effects: Arc<crate::runtime::AuraEffectSystem>,
    registration: &IssuerEnrollmentReturnRegistration,
) -> AgentResult<(
    RequiredIssuedEnrollmentSelectorCapability,
    VerifiedEnrollmentSetupPossession,
)> {
    let statement = &registration.statement;
    let selected =
        RequiredIssuedEnrollmentSelectorCapability::load(effects.clone(), statement.ceremony())
            .await?;
    let manifest = selected.manifest();
    let expected = match registration.mode {
        aura_core::crypto::single_signer::SigningMode::SingleSigner => {
            IssuerEnrollmentReturnStatement::for_owned_single_signer_manifest(
                manifest,
                statement.retrieval_credential_commitment(),
            )
        }
        aura_core::crypto::single_signer::SigningMode::Threshold => {
            IssuerEnrollmentReturnStatement::for_approved_manifest(
                manifest,
                statement.retrieval_credential_commitment(),
            )
        }
    }
    .map_err(boundary)?;
    if statement != &expected
        || selected.digest() != statement.manifest_digest()
        || manifest.subject != effects.runtime_authority_id()
        || manifest.initiator_device != effects.device_id()
    {
        return Err(AgentError::from(AuraError::permission_denied(
            "issuer return registration differs from protected original artifact",
        )));
    }
    let policy = manifest
        .final_inventory()
        .map_err(boundary)?
        .get(usize::from(statement.issuer_policy_index()))
        .cloned()
        .ok_or_else(|| {
            AgentError::from(AuraError::permission_denied(
                "original issuer root policy is absent",
            ))
        })?;
    let proof = &registration.proof;
    if policy.signing_node != aura_core::tree::NodeIndex(0)
        || policy.mode != registration.mode
        || policy.epoch != proof.epoch
        || policy.public_key_package != proof.public_key_package
        || match registration.mode {
            aura_core::crypto::single_signer::SigningMode::SingleSigner => {
                !proof.is_single_signer()
            }
            aura_core::crypto::single_signer::SigningMode::Threshold => {
                policy.threshold < 2
                    || usize::from(proof.signer_count) != policy.participants.len()
                    || proof.signers != (1..=proof.signer_count).collect::<Vec<_>>()
            }
        }
    {
        return Err(AgentError::from(AuraError::permission_denied(
            "issuer return proof differs from original root policy",
        )));
    }
    let proof_bytes = statement.required_transcript_bytes().map_err(boundary)?;
    if !effects
        .verify_signature(
            &proof_bytes,
            &proof.signature,
            &policy.public_key_package,
            policy.mode,
        )
        .await
        .map_err(boundary)?
    {
        return Err(AgentError::from(AuraError::permission_denied(
            "issuer return proof signature is invalid",
        )));
    }
    let setup_bytes = effects
        .secure_retrieve(
            &owner_location(SETUP_NAMESPACE, statement),
            &[SecureStorageCapability::Read],
        )
        .await
        .map_err(boundary)?;
    let code = std::str::from_utf8(&setup_bytes).map_err(boundary)?;
    let setup = aura_invitation::enrollment_setup::DeviceEnrollmentSetupRequest::decode(code)
        .map_err(boundary)?
        .verify_possession(
            effects.as_ref(),
            effects.physical_time().await.map_err(boundary)?.ts_ms,
        )
        .await
        .map_err(boundary)?;
    if setup.digest() != statement.setup_digest()
        || setup.statement().authority != manifest.invitee_authority
        || setup.statement().device != manifest.invitee_device
    {
        return Err(AgentError::from(AuraError::permission_denied(
            "retained setup proof differs from original issuer transfer",
        )));
    }
    Ok((selected, setup))
}

async fn verify_delivery(
    effects: &crate::runtime::AuraEffectSystem,
    registration: &IssuerEnrollmentReturnRegistration,
    setup: &VerifiedEnrollmentSetupPossession,
    delivery: &IssuerReturnMailboxDelivery,
) -> AgentResult<()> {
    if delivery.message_id
        != delivery
            .envelope
            .metadata
            .get("aura-message-id")
            .cloned()
            .unwrap_or_default()
        || delivery.envelope.metadata.get("session-id")
            != Some(&registration.statement.session().to_string())
        || delivery.envelope.metadata.get("aura-destination-device-id")
            != Some(&registration.statement.issuer_device().to_string())
        || delivery.envelope.source != registration.statement.provisional()
        || delivery.envelope.destination != registration.statement.subject()
        || delivery.envelope.metadata.get("aura-source-device-id")
            != Some(&registration.statement.invitee_device().to_string())
    {
        return Err(AgentError::from(AuraError::permission_denied(
            "return VM frame differs from original route",
        )));
    }
    let now = effects.physical_time().await.map_err(boundary)?.ts_ms;
    let candidate = delivery
        .setup_request
        .clone()
        .verify_possession(effects, now)
        .await
        .map_err(boundary)?;
    if candidate.digest() != setup.digest() || candidate.statement() != setup.statement() {
        return Err(AgentError::from(AuraError::permission_denied(
            "return frame setup key differs from retained original selection",
        )));
    }
    delivery
        .authorization
        .verify_untrusted_broker_admission(
            effects,
            effects,
            registration,
            &delivery.setup_request,
            &delivery.envelope,
        )
        .await
        .map_err(boundary)
}

impl AuraAgent {
    fn queue_return_frame_once(
        &self,
        key: &str,
        envelope: aura_core::effects::transport::TransportEnvelope,
    ) -> AgentResult<bool> {
        let mut seen = self.issuer_return_seen.lock();
        if seen.contains(key) {
            return Ok(false);
        }
        if !matches!(
            self.runtime().effects().queue_runtime_envelope(envelope),
            crate::runtime::subsystems::transport::QueueEnvelopeOutcome::Queued,
        ) {
            return Err(AgentError::from(AuraError::invalid(
                "issuer runtime intake refused original return frame",
            )));
        }
        seen.insert(key.to_owned());
        Ok(true)
    }

    /// Replay exact encrypted return frames, then ACK new broker deliveries
    /// only after they are durably retained and queued for the original VM.
    pub async fn drain_issuer_return_vm_mailbox(&self) -> AgentResult<usize> {
        let effects = self.runtime().effects();
        let registration_keys = effects
            .secure_list_keys(REGISTRATION_NAMESPACE, &[SecureStorageCapability::List])
            .await
            .map_err(boundary)?;
        if registration_keys.len() > MAX_REGISTRATIONS {
            return Err(AgentError::from(AuraError::invalid(
                "issuer return registration inventory exceeded",
            )));
        }
        let mut frame_keys = effects
            .secure_list_keys(FRAME_NAMESPACE, &[SecureStorageCapability::List])
            .await
            .map_err(boundary)?;
        if frame_keys.len() > MAX_RETAINED_FRAMES {
            return Err(AgentError::from(AuraError::invalid(
                "issuer return frame inventory exceeded",
            )));
        }
        let manager = effects.rendezvous_manager();
        let mut admitted_frames = 0;
        for key in registration_keys {
            let bytes = effects
                .secure_retrieve(
                    &SecureStorageLocation::new(REGISTRATION_NAMESPACE, &key),
                    &[SecureStorageCapability::Read],
                )
                .await
                .map_err(boundary)?;
            let registration: IssuerEnrollmentReturnRegistration =
                aura_core::util::serialization::from_slice(&bytes).map_err(boundary)?;
            let statement = &registration.statement;
            if owner_location(REGISTRATION_NAMESPACE, statement).key != key {
                return Err(AgentError::from(AuraError::permission_denied(
                    "issuer return registration storage key changed",
                )));
            }
            let now = effects.physical_time().await.map_err(boundary)?.ts_ms;
            if now >= statement.expires_at_ms() {
                // The protected record remains immutable until the selected
                // provider can retire its sealed expiry. An expired route
                // grants no replay or broker admission.
                continue;
            }
            let (_selected, setup) = verify_original_owner(effects.clone(), &registration).await?;
            let committed = matches!(
                self.runtime()
                    .ceremony_runner()
                    .terminal_outcome(statement.ceremony())
                    .await
                    .map_err(boundary)?,
                Some(aura_app::runtime_bridge::CeremonyTerminalOutcome::Committed)
            );
            if committed {
                let (receipt, _) = crate::handlers::invitation::enrollment_trust::recover_verified_response_receipt(
                    effects.as_ref(), statement.ceremony(),
                ).await?;
                if &receipt.invitation_id() != statement.invitation()
                    || receipt.subject() != statement.subject()
                    || receipt.device_id() != statement.invitee_device()
                    || receipt.setup_digest() != statement.setup_digest()
                {
                    return Err(AgentError::from(AuraError::permission_denied(
                        "committed return retirement differs from verified original response",
                    )));
                }
            }
            let credential_bytes = effects
                .secure_retrieve(
                    &owner_location(CREDENTIAL_NAMESPACE, statement),
                    &[SecureStorageCapability::Read],
                )
                .await
                .map_err(boundary)?;
            let credential = std::str::from_utf8(&credential_bytes).map_err(boundary)?;
            if credential.len() != 64
                || aura_core::hash::hash(credential.as_bytes())
                    != statement.retrieval_credential_commitment()
            {
                return Err(AgentError::from(AuraError::permission_denied(
                    "issuer return retrieval credential changed",
                )));
            }
            let setup_bytes = effects
                .secure_retrieve(
                    &owner_location(SETUP_NAMESPACE, statement),
                    &[SecureStorageCapability::Read],
                )
                .await
                .map_err(boundary)?;
            let root_digest = load_retirement_root(
                effects.as_ref(),
                statement,
                &bytes,
                &credential_bytes,
                &setup_bytes,
            )
            .await?;
            let prefix = frame_prefix(statement.manifest_digest());
            let scoped: Vec<_> = frame_keys
                .iter()
                .filter(|key| key.starts_with(&prefix))
                .cloned()
                .collect();
            let mut scoped_count = scoped.len();
            if scoped_count > MAX_FRAMES_PER_REGISTRATION {
                return Err(AgentError::from(AuraError::invalid(
                    "issuer return frame scope exceeded",
                )));
            }
            for key in scoped {
                let bytes = effects
                    .secure_retrieve(
                        &SecureStorageLocation::new(FRAME_NAMESPACE, &key),
                        &[SecureStorageCapability::Read],
                    )
                    .await
                    .map_err(boundary)?;
                let record: RetainedReturnFrame =
                    aura_core::util::serialization::from_slice(&bytes).map_err(boundary)?;
                if record.manifest_digest != statement.manifest_digest()
                    || frame_location(record.manifest_digest, &record.delivery.message_id).key
                        != key.as_str()
                {
                    return Err(AgentError::from(AuraError::permission_denied(
                        "retained return frame identity changed",
                    )));
                }
                verify_delivery(effects.as_ref(), &registration, &setup, &record.delivery).await?;
                retain_retirement_leaf(effects.as_ref(), root_digest, &key, &bytes).await?;
                if !committed {
                    admitted_frames +=
                        usize::from(self.queue_return_frame_once(&key, record.delivery.envelope)?);
                }
            }
            let Some(manager) = manager
                .as_ref()
                .filter(|manager| manager.config().bootstrap_broker.enabled)
            else {
                continue;
            };
            manager.register_original_issuer_return_mailbox(
                crate::runtime::services::bootstrap_broker::BootstrapIssuerReturnMailboxRegistration {
                    registration: registration.clone(),
                    retrieval_credential: credential.to_owned(),
                },
            ).await.map_err(boundary)?;
            let deliveries = manager
                .take_original_issuer_return_vm_envelopes(
                    statement.subject(),
                    statement.issuer_device(),
                    statement.manifest_digest(),
                    credential,
                )
                .await
                .map_err(boundary)?;
            for delivery in deliveries {
                verify_delivery(effects.as_ref(), &registration, &setup, &delivery).await?;
                let location = frame_location(statement.manifest_digest(), &delivery.message_id);
                let already_retained = frame_keys.iter().any(|key| *key == location.key);
                if !already_retained
                    && (frame_keys.len() >= MAX_RETAINED_FRAMES
                        || scoped_count >= MAX_FRAMES_PER_REGISTRATION)
                {
                    return Err(AgentError::from(AuraError::invalid(
                        "issuer return frame retention capacity exceeded",
                    )));
                }
                let record = RetainedReturnFrame {
                    manifest_digest: statement.manifest_digest(),
                    delivery,
                };
                let bytes = aura_core::util::serialization::to_vec(&record).map_err(boundary)?;
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
                let original = effects
                    .secure_retrieve(&location, &[SecureStorageCapability::Read])
                    .await
                    .map_err(boundary)?;
                if original != bytes {
                    return Err(AgentError::from(AuraError::permission_denied(
                        "broker redelivery changed immutable return frame",
                    )));
                }
                retain_retirement_leaf(effects.as_ref(), root_digest, &location.key, &bytes)
                    .await?;
                if !already_retained {
                    frame_keys.push(location.key.clone());
                    scoped_count += 1;
                }
                if !committed {
                    admitted_frames += usize::from(
                        self.queue_return_frame_once(&location.key, record.delivery.envelope)?,
                    );
                }
                manager
                    .acknowledge_original_issuer_return_vm_envelope(
                        statement.subject(),
                        statement.issuer_device(),
                        statement.manifest_digest(),
                        credential,
                        &record.delivery.message_id,
                    )
                    .await
                    .map_err(boundary)?;
            }
        }
        Ok(admitted_frames)
    }
}
