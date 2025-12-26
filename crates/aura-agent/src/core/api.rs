//! Public Agent API
//!
//! Minimal public API surface for the agent runtime.

use super::{AgentConfig, AgentError, AgentResult, AuthorityContext};
use crate::handlers::{
    AuthService, ChatService, InvitationService, RecoveryService, SessionService,
};
use crate::runtime::services::SyncManagerConfig;
use crate::runtime::services::ThresholdSigningService;
use crate::runtime::system::RuntimeSystem;
use crate::runtime::{EffectContext, EffectSystemBuilder};
use aura_core::hash::hash;
use aura_core::identifiers::{AuthorityId, ContextId};

/// Main agent interface - thin facade delegating to runtime
///
/// Services are created on-demand as lightweight wrappers around effects.
/// No lazy initialization needed since services are stateless.
pub struct AuraAgent {
    /// The runtime system handling all operations
    runtime: RuntimeSystem,

    /// Authority context for this agent (includes cached account_id)
    context: AuthorityContext,
}

impl AuraAgent {
    /// Create a new agent with the given runtime system
    pub(crate) fn new(runtime: RuntimeSystem, authority_id: AuthorityId) -> Self {
        Self {
            context: AuthorityContext::new_with_device(authority_id, runtime.device_id()),
            runtime,
        }
    }

    /// Get the authority ID for this agent
    pub fn authority_id(&self) -> AuthorityId {
        self.context.authority_id
    }

    /// Get the authority context (read-only)
    pub fn context(&self) -> &AuthorityContext {
        &self.context
    }

    /// Access the runtime system (for advanced operations)
    pub fn runtime(&self) -> &RuntimeSystem {
        &self.runtime
    }

    /// Get the session management service
    ///
    /// Provides access to session creation, management, and lifecycle operations.
    /// Returns a new lightweight service instance (services are stateless wrappers).
    pub fn sessions(&self) -> SessionService {
        SessionService::new(
            self.runtime.effects(),
            self.context.clone(),
            self.context.account_id,
        )
    }

    /// Get the authentication service
    ///
    /// Provides access to authentication operations including challenge-response
    /// flows and device key verification.
    /// Returns a new lightweight service instance (services are stateless wrappers).
    pub fn auth(&self) -> AgentResult<AuthService> {
        AuthService::new(
            self.runtime.effects(),
            self.context.clone(),
            self.context.account_id,
        )
    }

    /// Get the chat service
    ///
    /// Provides access to chat operations including group creation, messaging,
    /// and message history retrieval.
    pub fn chat(&self) -> ChatService {
        ChatService::new(self.runtime.effects())
    }

    /// Get the invitation service
    ///
    /// Provides access to invitation operations including creating, accepting,
    /// and declining invitations for channels, guardians, and contacts.
    /// Returns a new lightweight service instance (services are stateless wrappers).
    pub fn invitations(&self) -> AgentResult<InvitationService> {
        InvitationService::new(self.runtime.effects(), self.context.clone())
    }

    /// Get the recovery service
    ///
    /// Provides access to guardian-based recovery operations including device
    /// addition/removal, tree replacement, and guardian set updates.
    /// Returns a new lightweight service instance (services are stateless wrappers).
    pub fn recovery(&self) -> AgentResult<RecoveryService> {
        RecoveryService::new(self.runtime.effects(), self.context.clone())
    }

    /// Get the threshold signing service
    ///
    /// Provides access to unified threshold signing operations including:
    /// - Multi-device signing (your devices)
    /// - Guardian recovery approvals (cross-authority)
    /// - Group operation approvals (shared authority)
    ///
    /// Returns a new lightweight service instance (services are stateless wrappers).
    pub fn threshold_signing(&self) -> ThresholdSigningService {
        ThresholdSigningService::new(self.runtime.effects())
    }

    /// Get the ceremony tracker for guardian ceremony coordination
    ///
    /// The ceremony tracker manages state for in-progress guardian ceremonies,
    /// including tracking which guardians have accepted invitations and whether
    /// the threshold has been reached.
    ///
    /// # Returns
    /// A cloneable reference to the ceremony tracker service
    pub async fn ceremony_tracker(&self) -> crate::runtime::services::CeremonyTracker {
        self.runtime.ceremony_tracker().clone()
    }

    /// Process guardian ceremony acceptances and auto-complete when threshold is reached
    ///
    /// This method should be called periodically (e.g., in a background task) to:
    /// 1. Poll for incoming guardian acceptance messages via transport
    /// 2. Update the ceremony tracker with each acceptance
    /// 3. Automatically commit ceremonies when threshold is reached
    ///
    /// # Returns
    /// Number of acceptances processed and number of ceremonies completed
    pub async fn process_ceremony_acceptances(&self) -> AgentResult<(usize, usize)> {
        use aura_core::effects::{ThresholdSigningEffects, TransportEffects};
        use aura_protocol::effects::TreeEffects;

        let ceremony_tracker = self.ceremony_tracker().await;
        let authority_id = self.authority_id();
        let effects = self.runtime.effects();

        let mut acceptance_count = 0usize;
        let mut completed_count = 0usize;

        loop {
            let envelope = match effects.receive_envelope().await {
                Ok(env) => env,
                Err(aura_core::effects::TransportError::NoMessage) => break,
                Err(e) => {
                    tracing::warn!("Error receiving ceremony envelope: {}", e);
                    break;
                }
            };

            let Some(content_type) = envelope.metadata.get("content-type").cloned() else {
                effects.requeue_envelope(envelope);
                break;
            };

            match content_type.as_str() {
                "application/aura-guardian-acceptance" => {
                    let (Some(ceremony_id), Some(guardian_id)) = (
                        envelope.metadata.get("ceremony-id"),
                        envelope.metadata.get("guardian-id"),
                    ) else {
                        continue;
                    };

                    acceptance_count += 1;
                    let guardian_authority: AuthorityId = match guardian_id.parse() {
                        Ok(id) => id,
                        Err(e) => {
                            tracing::warn!(
                                ceremony_id = %ceremony_id,
                                guardian_id = %guardian_id,
                                error = %e,
                                "Invalid guardian authority id in acceptance"
                            );
                            let _ = ceremony_tracker
                                .mark_failed(
                                    ceremony_id,
                                    Some(format!(
                                        "Invalid guardian id in acceptance: {guardian_id}"
                                    )),
                                )
                                .await;
                            continue;
                        }
                    };

                    let threshold_reached = match ceremony_tracker
                        .mark_accepted(
                            ceremony_id,
                            aura_core::threshold::ParticipantIdentity::guardian(guardian_authority),
                        )
                        .await
                    {
                        Ok(reached) => reached,
                        Err(e) => {
                            tracing::warn!(
                                ceremony_id = %ceremony_id,
                                guardian_id = %guardian_id,
                                error = %e,
                                "Failed to mark guardian as accepted"
                            );
                            continue;
                        }
                    };

                    if !threshold_reached {
                        continue;
                    }

                    let ceremony_state = match ceremony_tracker.get(ceremony_id).await {
                        Ok(s) => s,
                        Err(e) => {
                            tracing::error!(
                                ceremony_id = %ceremony_id,
                                error = %e,
                                "Failed to retrieve ceremony state for commit"
                            );
                            continue;
                        }
                    };

                    if ceremony_state.is_committed {
                        continue;
                    }

                    let new_epoch = ceremony_state.new_epoch;
                    if let Err(e) = effects.commit_key_rotation(&authority_id, new_epoch).await {
                        tracing::error!(
                            ceremony_id = %ceremony_id,
                            new_epoch,
                            error = %e,
                            "Failed to commit guardian key rotation"
                        );
                        let _ = ceremony_tracker
                            .mark_failed(ceremony_id, Some(format!("Commit failed: {e}")))
                            .await;
                        continue;
                    }

                    let mut bindings = Vec::new();
                    for participant in &ceremony_state.participants {
                        let aura_core::threshold::ParticipantIdentity::Guardian(guardian_id) =
                            participant
                        else {
                            continue;
                        };

                        let binding_hash = aura_core::Hash32(aura_core::hash::hash(
                            format!(
                                "guardian-binding:{}:{}:{}:{}",
                                ceremony_id, authority_id, guardian_id, new_epoch
                            )
                            .as_bytes(),
                        ));

                        bindings.push(aura_journal::fact::RelationalFact::GuardianBinding {
                            account_id: authority_id,
                            guardian_id: *guardian_id,
                            binding_hash,
                        });
                    }

                    if !bindings.is_empty() {
                        if let Err(e) = effects.commit_relational_facts(bindings).await {
                            tracing::error!(
                                ceremony_id = %ceremony_id,
                                error = %e,
                                "Failed to commit GuardianBinding facts"
                            );
                            let _ = ceremony_tracker
                                .mark_failed(
                                    ceremony_id,
                                    Some(format!("Failed to commit guardian bindings: {e}")),
                                )
                                .await;
                            continue;
                        }
                    }

                    let _ = ceremony_tracker.mark_committed(ceremony_id).await;
                    completed_count += 1;
                }
                "application/aura-device-enrollment-key-package" => {
                    use aura_core::effects::{
                        SecureStorageCapability, SecureStorageEffects, SecureStorageLocation,
                    };

                    let (Some(ceremony_id), Some(pending_epoch_str), Some(initiator_device_id_str)) = (
                        envelope.metadata.get("ceremony-id"),
                        envelope.metadata.get("pending-epoch"),
                        envelope.metadata.get("initiator-device-id"),
                    ) else {
                        tracing::warn!("Malformed device enrollment key package envelope");
                        continue;
                    };

                    let pending_epoch: u64 = match pending_epoch_str.parse() {
                        Ok(v) => v,
                        Err(e) => {
                            tracing::warn!(
                                ceremony_id = %ceremony_id,
                                pending_epoch = %pending_epoch_str,
                                error = %e,
                                "Invalid pending epoch in device enrollment key package"
                            );
                            continue;
                        }
                    };

                    let initiator_device_id: aura_core::DeviceId =
                        match initiator_device_id_str.parse() {
                            Ok(v) => v,
                            Err(e) => {
                                tracing::warn!(
                                    ceremony_id = %ceremony_id,
                                    initiator_device_id = %initiator_device_id_str,
                                    error = %e,
                                    "Invalid initiator device id in device enrollment key package"
                                );
                                continue;
                            }
                        };

                    let self_device_id = self.context.device_id();
                    if let Some(participant_device_id) =
                        envelope.metadata.get("participant-device-id")
                    {
                        if participant_device_id != &self_device_id.to_string() {
                            tracing::warn!(
                                ceremony_id = %ceremony_id,
                                expected_device_id = %self_device_id,
                                got_device_id = %participant_device_id,
                                "Ignoring device enrollment key package for a different device"
                            );
                            continue;
                        }
                    }

                    let participant =
                        aura_core::threshold::ParticipantIdentity::device(self_device_id);
                    let location = SecureStorageLocation::with_sub_key(
                        "participant_shares",
                        format!("{}/{}", authority_id, pending_epoch),
                        participant.storage_key(),
                    );

                    if let Err(e) = effects
                        .secure_store(
                            &location,
                            &envelope.payload,
                            &[
                                SecureStorageCapability::Read,
                                SecureStorageCapability::Write,
                            ],
                        )
                        .await
                    {
                        tracing::warn!(
                            ceremony_id = %ceremony_id,
                            error = %e,
                            "Failed to store device enrollment key package"
                        );
                        continue;
                    }

                    // Acknowledge storage to the initiator device.
                    let context_entropy = {
                        let mut h = aura_core::hash::hasher();
                        h.update(b"DEVICE_ENROLLMENT_CONTEXT");
                        h.update(&authority_id.to_bytes());
                        h.update(ceremony_id.as_bytes());
                        h.finalize()
                    };
                    let ceremony_context =
                        aura_core::identifiers::ContextId::new_from_entropy(context_entropy);

                    let mut metadata = std::collections::HashMap::new();
                    metadata.insert(
                        "content-type".to_string(),
                        "application/aura-device-enrollment-acceptance".to_string(),
                    );
                    metadata.insert("ceremony-id".to_string(), ceremony_id.clone());
                    metadata.insert("acceptor-device-id".to_string(), self_device_id.to_string());
                    metadata.insert(
                        "aura-destination-device-id".to_string(),
                        initiator_device_id.to_string(),
                    );

                    let envelope = aura_core::effects::TransportEnvelope {
                        destination: authority_id,
                        source: authority_id,
                        context: ceremony_context,
                        payload: Vec::new(),
                        metadata,
                        receipt: None,
                    };

                    if let Err(e) = effects.send_envelope(envelope).await {
                        tracing::warn!(
                            ceremony_id = %ceremony_id,
                            error = %e,
                            "Failed to send device enrollment acceptance"
                        );
                    }
                }

                "application/aura-device-threshold-key-package" => {
                    use aura_core::effects::{
                        SecureStorageCapability, SecureStorageEffects, SecureStorageLocation,
                    };

                    let (Some(ceremony_id), Some(pending_epoch_str), Some(initiator_device_id_str)) = (
                        envelope.metadata.get("ceremony-id"),
                        envelope.metadata.get("pending-epoch"),
                        envelope.metadata.get("initiator-device-id"),
                    ) else {
                        tracing::warn!("Malformed device threshold key package envelope");
                        continue;
                    };

                    let pending_epoch: u64 = match pending_epoch_str.parse() {
                        Ok(v) => v,
                        Err(e) => {
                            tracing::warn!(
                                ceremony_id = %ceremony_id,
                                pending_epoch = %pending_epoch_str,
                                error = %e,
                                "Invalid pending epoch in device threshold key package"
                            );
                            continue;
                        }
                    };

                    let initiator_device_id: aura_core::DeviceId =
                        match initiator_device_id_str.parse() {
                            Ok(v) => v,
                            Err(e) => {
                                tracing::warn!(
                                    ceremony_id = %ceremony_id,
                                    initiator_device_id = %initiator_device_id_str,
                                    error = %e,
                                    "Invalid initiator device id in device threshold key package"
                                );
                                continue;
                            }
                        };

                    let self_device_id = self.context.device_id();
                    if let Some(participant_device_id) =
                        envelope.metadata.get("participant-device-id")
                    {
                        if participant_device_id != &self_device_id.to_string() {
                            tracing::warn!(
                                ceremony_id = %ceremony_id,
                                expected_device_id = %self_device_id,
                                got_device_id = %participant_device_id,
                                "Ignoring device threshold key package for a different device"
                            );
                            continue;
                        }
                    }

                    let participant =
                        aura_core::threshold::ParticipantIdentity::device(self_device_id);
                    let location = SecureStorageLocation::with_sub_key(
                        "participant_shares",
                        format!("{}/{}", authority_id, pending_epoch),
                        participant.storage_key(),
                    );

                    if let Err(e) = effects
                        .secure_store(
                            &location,
                            &envelope.payload,
                            &[
                                SecureStorageCapability::Read,
                                SecureStorageCapability::Write,
                            ],
                        )
                        .await
                    {
                        tracing::warn!(
                            ceremony_id = %ceremony_id,
                            error = %e,
                            "Failed to store device threshold key package"
                        );
                        continue;
                    }

                    // Acknowledge storage to the initiator device.
                    let context_entropy = {
                        let mut h = aura_core::hash::hasher();
                        h.update(b"DEVICE_THRESHOLD_CONTEXT");
                        h.update(&authority_id.to_bytes());
                        h.update(ceremony_id.as_bytes());
                        h.finalize()
                    };
                    let ceremony_context =
                        aura_core::identifiers::ContextId::new_from_entropy(context_entropy);

                    let mut metadata = std::collections::HashMap::new();
                    metadata.insert(
                        "content-type".to_string(),
                        "application/aura-device-threshold-acceptance".to_string(),
                    );
                    metadata.insert("ceremony-id".to_string(), ceremony_id.clone());
                    metadata.insert("acceptor-device-id".to_string(), self_device_id.to_string());
                    metadata.insert(
                        "aura-destination-device-id".to_string(),
                        initiator_device_id.to_string(),
                    );

                    let envelope = aura_core::effects::TransportEnvelope {
                        destination: authority_id,
                        source: authority_id,
                        context: ceremony_context,
                        payload: Vec::new(),
                        metadata,
                        receipt: None,
                    };

                    if let Err(e) = effects.send_envelope(envelope).await {
                        tracing::warn!(
                            ceremony_id = %ceremony_id,
                            error = %e,
                            "Failed to send device threshold acceptance"
                        );
                    }
                }

                "application/aura-device-enrollment-acceptance" => {
                    let (Some(ceremony_id), Some(device_id_str)) = (
                        envelope.metadata.get("ceremony-id"),
                        envelope.metadata.get("acceptor-device-id"),
                    ) else {
                        continue;
                    };

                    let acceptor_device_id: aura_core::DeviceId = match device_id_str.parse() {
                        Ok(d) => d,
                        Err(e) => {
                            tracing::warn!(
                                ceremony_id = %ceremony_id,
                                device_id = %device_id_str,
                                error = %e,
                                "Invalid device id in device enrollment acceptance"
                            );
                            continue;
                        }
                    };

                    acceptance_count += 1;

                    let threshold_reached = match ceremony_tracker
                        .mark_accepted(
                            ceremony_id,
                            aura_core::threshold::ParticipantIdentity::device(acceptor_device_id),
                        )
                        .await
                    {
                        Ok(reached) => reached,
                        Err(e) => {
                            tracing::warn!(
                                ceremony_id = %ceremony_id,
                                device_id = %device_id_str,
                                error = %e,
                                "Failed to mark device as accepted"
                            );
                            continue;
                        }
                    };

                    if !threshold_reached {
                        continue;
                    }

                    let ceremony_state = match ceremony_tracker.get(ceremony_id).await {
                        Ok(s) => s,
                        Err(e) => {
                            tracing::error!(
                                ceremony_id = %ceremony_id,
                                error = %e,
                                "Failed to retrieve ceremony state for commit"
                            );
                            continue;
                        }
                    };

                    let enrolled_device_id = ceremony_state
                        .enrollment_device_id
                        .unwrap_or(acceptor_device_id);

                    if ceremony_state.is_committed {
                        continue;
                    }

                    let new_epoch = ceremony_state.new_epoch;
                    if let Err(e) = effects.commit_key_rotation(&authority_id, new_epoch).await {
                        tracing::error!(
                            ceremony_id = %ceremony_id,
                            new_epoch,
                            error = %e,
                            "Failed to commit device enrollment key rotation"
                        );
                        let _ = ceremony_tracker
                            .mark_failed(ceremony_id, Some(format!("Commit failed: {e}")))
                            .await;
                        continue;
                    }

                    // Add a device leaf to the commitment tree so UI membership updates.
                    let tree_state = match effects.get_current_state().await {
                        Ok(s) => s,
                        Err(e) => {
                            tracing::error!(
                                ceremony_id = %ceremony_id,
                                error = %e,
                                "Failed to read tree state for device enrollment commit"
                            );
                            let _ = ceremony_tracker
                                .mark_failed(
                                    ceremony_id,
                                    Some(format!("Failed to read tree state: {e}")),
                                )
                                .await;
                            continue;
                        }
                    };

                    let next_leaf = tree_state
                        .leaves
                        .keys()
                        .map(|id| id.0)
                        .max()
                        .unwrap_or(0)
                        .saturating_add(1);

                    let leaf = aura_core::tree::LeafNode::new_device(
                        aura_core::tree::LeafId(next_leaf),
                        enrolled_device_id,
                        Vec::new(),
                    );

                    let op = aura_core::tree::TreeOp {
                        parent_epoch: tree_state.epoch,
                        parent_commitment: tree_state.root_commitment,
                        op: aura_core::tree::TreeOpKind::AddLeaf {
                            leaf,
                            under: aura_core::tree::NodeIndex(0),
                        },
                        version: 1,
                    };

                    let attested = aura_core::tree::AttestedOp {
                        op,
                        agg_sig: Vec::new(),
                        signer_count: 1,
                    };

                    if let Err(e) = effects.apply_attested_op(attested).await {
                        tracing::error!(
                            ceremony_id = %ceremony_id,
                            error = %e,
                            "Failed to apply tree op for device enrollment"
                        );
                        let _ = ceremony_tracker
                            .mark_failed(ceremony_id, Some(format!("Failed to apply tree op: {e}")))
                            .await;
                        continue;
                    }

                    let _ = ceremony_tracker.mark_committed(ceremony_id).await;
                    completed_count += 1;
                }

                "application/aura-device-threshold-acceptance" => {
                    let (Some(ceremony_id), Some(device_id_str)) = (
                        envelope.metadata.get("ceremony-id"),
                        envelope.metadata.get("acceptor-device-id"),
                    ) else {
                        continue;
                    };

                    let acceptor_device_id: aura_core::DeviceId = match device_id_str.parse() {
                        Ok(d) => d,
                        Err(e) => {
                            tracing::warn!(
                                ceremony_id = %ceremony_id,
                                device_id = %device_id_str,
                                error = %e,
                                "Invalid device id in device threshold acceptance"
                            );
                            continue;
                        }
                    };

                    acceptance_count += 1;

                    let threshold_reached = match ceremony_tracker
                        .mark_accepted(
                            ceremony_id,
                            aura_core::threshold::ParticipantIdentity::device(acceptor_device_id),
                        )
                        .await
                    {
                        Ok(reached) => reached,
                        Err(e) => {
                            tracing::warn!(
                                ceremony_id = %ceremony_id,
                                device_id = %device_id_str,
                                error = %e,
                                "Failed to mark device as accepted"
                            );
                            continue;
                        }
                    };

                    if !threshold_reached {
                        continue;
                    }

                    let ceremony_state = match ceremony_tracker.get(ceremony_id).await {
                        Ok(s) => s,
                        Err(e) => {
                            tracing::error!(
                                ceremony_id = %ceremony_id,
                                error = %e,
                                "Failed to retrieve ceremony state for commit"
                            );
                            continue;
                        }
                    };

                    if ceremony_state.is_committed {
                        continue;
                    }

                    let new_epoch = ceremony_state.new_epoch;
                    if let Err(e) = effects.commit_key_rotation(&authority_id, new_epoch).await {
                        tracing::error!(
                            ceremony_id = %ceremony_id,
                            new_epoch,
                            error = %e,
                            "Failed to commit device threshold key rotation"
                        );
                        let _ = ceremony_tracker
                            .mark_failed(ceremony_id, Some(format!("Commit failed: {e}")))
                            .await;
                        continue;
                    }

                    let _ = ceremony_tracker.mark_committed(ceremony_id).await;
                    completed_count += 1;
                }
                _ => {
                    effects.requeue_envelope(envelope);
                    break;
                }
            }
        }

        Ok((acceptance_count, completed_count))
    }

    /// Shutdown the agent
    pub async fn shutdown(self, ctx: &EffectContext) -> AgentResult<()> {
        self.runtime
            .shutdown(ctx)
            .await
            .map_err(AgentError::runtime)
    }
}

/// Builder for creating agents
pub struct AgentBuilder {
    config: AgentConfig,
    authority_id: Option<AuthorityId>,
    sync_config: Option<SyncManagerConfig>,
}

impl AgentBuilder {
    /// Create a new agent builder
    pub fn new() -> Self {
        Self {
            config: AgentConfig::default(),
            authority_id: None,
            sync_config: None,
        }
    }

    /// Set the authority ID
    pub fn with_authority(mut self, authority_id: AuthorityId) -> Self {
        self.authority_id = Some(authority_id);
        self
    }

    /// Enable the sync service with default configuration.
    pub fn with_sync(mut self) -> Self {
        self.sync_config = Some(SyncManagerConfig::default());
        self
    }

    /// Enable the sync service with a custom configuration.
    pub fn with_sync_config(mut self, config: SyncManagerConfig) -> Self {
        self.sync_config = Some(config);
        self
    }

    /// Set the configuration
    pub fn with_config(mut self, config: AgentConfig) -> Self {
        self.config = config;
        self
    }

    /// Build a production agent
    pub async fn build_production(self, _ctx: &EffectContext) -> AgentResult<AuraAgent> {
        let sync_config = self.sync_config.clone();
        let authority_id = self
            .authority_id
            .ok_or_else(|| AgentError::config("Authority ID required"))?;

        // Build-time context used only for effect wiring
        let context_entropy = hash(&authority_id.to_bytes());
        let temp_context = EffectContext::new(
            authority_id,
            ContextId::new_from_entropy(context_entropy),
            aura_core::effects::ExecutionMode::Production,
        );

        let mut builder = EffectSystemBuilder::production()
            .with_config(self.config)
            .with_authority(authority_id);
        if let Some(sync_config) = sync_config {
            builder = builder.with_sync_config(sync_config);
        }
        let runtime = builder
            .build(&temp_context)
            .await
            .map_err(AgentError::runtime)?;

        Ok(AuraAgent::new(runtime, authority_id))
    }

    /// Build a testing agent
    pub fn build_testing(self) -> AgentResult<AuraAgent> {
        let sync_config = self.sync_config.clone();
        let authority_id = self
            .authority_id
            .ok_or_else(|| AgentError::config("Authority ID required"))?;

        let mut builder = EffectSystemBuilder::testing()
            .with_config(self.config)
            .with_authority(authority_id);
        if let Some(sync_config) = sync_config {
            builder = builder.with_sync_config(sync_config);
        }
        let runtime = builder.build_sync().map_err(AgentError::runtime)?;

        Ok(AuraAgent::new(runtime, authority_id))
    }

    /// Build a testing agent using an existing async runtime
    pub async fn build_testing_async(self, ctx: &EffectContext) -> AgentResult<AuraAgent> {
        let sync_config = self.sync_config.clone();
        let authority_id = self
            .authority_id
            .ok_or_else(|| AgentError::config("Authority ID required"))?;

        let mut builder = EffectSystemBuilder::testing()
            .with_config(self.config)
            .with_authority(authority_id);
        if let Some(sync_config) = sync_config {
            builder = builder.with_sync_config(sync_config);
        }
        let runtime = builder.build(ctx).await.map_err(AgentError::runtime)?;

        Ok(AuraAgent::new(runtime, authority_id))
    }

    /// Build a simulation agent
    pub fn build_simulation(self, seed: u64) -> AgentResult<AuraAgent> {
        let sync_config = self.sync_config.clone();
        let authority_id = self
            .authority_id
            .ok_or_else(|| AgentError::config("Authority ID required"))?;

        let mut builder = EffectSystemBuilder::simulation(seed)
            .with_config(self.config)
            .with_authority(authority_id);
        if let Some(sync_config) = sync_config {
            builder = builder.with_sync_config(sync_config);
        }
        let runtime = builder.build_sync().map_err(AgentError::runtime)?;

        Ok(AuraAgent::new(runtime, authority_id))
    }

    /// Build a simulation agent using an existing async runtime
    pub async fn build_simulation_async(
        self,
        seed: u64,
        ctx: &EffectContext,
    ) -> AgentResult<AuraAgent> {
        let sync_config = self.sync_config.clone();
        let authority_id = self
            .authority_id
            .ok_or_else(|| AgentError::config("Authority ID required"))?;

        let mut builder = EffectSystemBuilder::simulation(seed)
            .with_config(self.config)
            .with_authority(authority_id);
        if let Some(sync_config) = sync_config {
            builder = builder.with_sync_config(sync_config);
        }
        let runtime = builder.build(ctx).await.map_err(AgentError::runtime)?;

        Ok(AuraAgent::new(runtime, authority_id))
    }

    /// Build a simulation agent with shared transport inbox for multi-agent scenarios
    ///
    /// This enables communication between multiple simulated agents (e.g., Bob, Alice, Carol)
    /// by providing a shared transport layer that routes messages based on destination authority.
    pub async fn build_simulation_async_with_shared_transport(
        self,
        seed: u64,
        ctx: &EffectContext,
        shared_transport: crate::SharedTransport,
    ) -> AgentResult<AuraAgent> {
        let sync_config = self.sync_config.clone();
        let authority_id = self
            .authority_id
            .ok_or_else(|| AgentError::config("Authority ID required"))?;

        let mut builder = EffectSystemBuilder::simulation(seed)
            .with_config(self.config)
            .with_authority(authority_id)
            .with_shared_transport(shared_transport);
        if let Some(sync_config) = sync_config {
            builder = builder.with_sync_config(sync_config);
        }
        let runtime = builder.build(ctx).await.map_err(AgentError::runtime)?;

        Ok(AuraAgent::new(runtime, authority_id))
    }
}

impl Default for AgentBuilder {
    fn default() -> Self {
        Self::new()
    }
}
