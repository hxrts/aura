//! Choreography adapter implementation
//!
//! Adapter for integrating with the choreographic programming
//! system from aura-protocol in the authority-centric runtime.
//!
//! ## Role Family Resolution
//!
//! The adapter supports parameterized role families like `Witness[N]` from
//! choreography definitions. Use `with_role_family()` to register role instances:
//!
//! ```ignore
//! let adapter = AuraProtocolAdapter::new(effects, authority_id, self_role, role_map)
//!     .with_role_family("Witness", witness_roles.clone());
//! ```
//!
//! ## Guard Chain Enforcement
//!
//! The adapter supports guard chain enforcement for choreography sends. Configure
//! guards using `with_guard_config()`:
//!
//! ```ignore
//! let guard_config = GuardConfig::new(context_id)
//!     .with_message_guard::<MyMessage>("cap:my_capability", 100);
//!
//! let adapter = AuraProtocolAdapter::new(effects, authority_id, self_role, role_map)
//!     .with_guard_config(guard_config);
//! ```

use async_trait::async_trait;
use aura_core::effects::{AdmissionError, CapabilityKey, RuntimeCapabilityEffects};
use aura_core::hash::hash;
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_core::util::serialization::{from_slice, to_vec};
use aura_core::FlowCost;
use aura_guards::guards::journal::JournalCoupler;
use aura_guards::prelude::{GuardContextProvider, GuardEffects, SendGuardChain};
use aura_guards::LeakageBudget;
use aura_mpst::telltale_choreography::{
    ChoreoHandler, ChoreoHandlerExt, ChoreoResult, ChoreographyError as TelltaleChoreographyError,
    LabelId, RoleId,
};
use aura_mpst::ChoreographicAdapterExt;
use aura_protocol::effects::{
    ChoreographicEffects, ChoreographicRole, ChoreographyError as AuraChoreographyError, RoleIndex,
};
use std::any::Any;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, warn};
use uuid::Uuid;

/// Guard requirements for a specific message type.
#[derive(Debug, Clone)]
pub struct MessageGuardRequirements {
    /// Required capability for sending this message (e.g., "cap:amp_send")
    pub capability: String,
    /// Flow cost for sending this message
    pub flow_cost: FlowCost,
    /// Optional leakage budget for this message
    pub leakage_budget: Option<LeakageBudget>,
    /// Journal facts to record after successful send (from choreography annotation)
    pub journal_facts: Option<String>,
    /// Whether to merge journal after this message (from choreography annotation)
    pub journal_merge: bool,
}

impl MessageGuardRequirements {
    /// Create guard requirements with capability and flow cost.
    pub fn new(capability: impl Into<String>, flow_cost: impl Into<FlowCost>) -> Self {
        Self {
            capability: capability.into(),
            flow_cost: flow_cost.into(),
            leakage_budget: None,
            journal_facts: None,
            journal_merge: false,
        }
    }

    /// Add leakage budget to the guard requirements.
    pub fn with_leakage_budget(mut self, budget: LeakageBudget) -> Self {
        self.leakage_budget = Some(budget);
        self
    }

    /// Set journal facts to record after successful send.
    pub fn with_journal_facts(mut self, facts: impl Into<String>) -> Self {
        self.journal_facts = Some(facts.into());
        self
    }

    /// Enable journal merge after this message.
    pub fn with_journal_merge(mut self, merge: bool) -> Self {
        self.journal_merge = merge;
        self
    }
}

/// Configuration for guard chain enforcement in choreography execution.
///
/// Maps message type names to their guard requirements (capability + flow cost).
#[derive(Debug, Clone, Default)]
pub struct GuardConfig {
    /// Context ID for guard evaluation
    pub context_id: Option<ContextId>,
    /// Map of message type name -> guard requirements
    guards: HashMap<String, MessageGuardRequirements>,
}

impl GuardConfig {
    /// Create a new guard config with the given context ID.
    pub fn new(context_id: ContextId) -> Self {
        Self {
            context_id: Some(context_id),
            guards: HashMap::new(),
        }
    }

    /// Create an empty guard config (no guard enforcement).
    pub fn none() -> Self {
        Self::default()
    }

    /// Add guard requirements for a specific message type.
    ///
    /// The type name is derived from `std::any::type_name::<M>()`.
    pub fn with_message_guard<M: 'static>(
        mut self,
        capability: impl Into<String>,
        flow_cost: impl Into<FlowCost>,
    ) -> Self {
        let type_name = std::any::type_name::<M>().to_string();
        self.guards.insert(
            type_name,
            MessageGuardRequirements::new(capability, flow_cost),
        );
        self
    }

    /// Add guard requirements for a message type by name.
    pub fn with_named_guard(
        mut self,
        type_name: impl Into<String>,
        requirements: MessageGuardRequirements,
    ) -> Self {
        self.guards.insert(type_name.into(), requirements);
        self
    }

    /// Get guard requirements for a message type.
    pub fn get_guard(&self, type_name: &str) -> Option<&MessageGuardRequirements> {
        self.guards.get(type_name)
    }

    /// Check if guard enforcement is enabled (context_id is set).
    pub fn is_enabled(&self) -> bool {
        self.context_id.is_some()
    }
}

/// Adapter for choreography integration
#[derive(Debug)]
pub struct AuraHandlerAdapter {
    authority_id: AuthorityId,
}

impl AuraHandlerAdapter {
    pub fn new(authority_id: AuthorityId) -> Self {
        Self { authority_id }
    }

    pub fn authority_id(&self) -> AuthorityId {
        self.authority_id
    }
}

/// Metadata captured for received choreography messages.
#[derive(Debug, Clone)]
pub struct ReceivedMessage {
    pub type_name: &'static str,
    pub bytes: Vec<u8>,
}

/// Request passed to dynamic message providers.
#[derive(Debug)]
pub struct MessageRequest<R: RoleId> {
    #[allow(dead_code)]
    pub to: R,
    pub type_name: &'static str,
}

/// Runtime adapter used by generated choreography runners.
///
/// This adapter implements the `ChoreographicAdapter` trait from telltale-choreography,
/// bridging Aura's effect system with the choreographic programming model.
///
/// ## Role Family Support
///
/// For protocols with parameterized roles (e.g., `Witness[N]`), use `with_role_family()`
/// to register role instances that can be resolved during broadcast/collect operations.
type MessageProviderFn<R> =
    Box<dyn FnMut(MessageRequest<R>, &[ReceivedMessage]) -> Option<Box<dyn Any + Send>> + Send>;

type BranchDeciderFn = Box<dyn FnMut(&[ReceivedMessage]) -> Option<String> + Send>;

struct RuntimeAdmissionConfig {
    capability_effects: Arc<dyn RuntimeCapabilityEffects>,
    required_capabilities: Vec<CapabilityKey>,
    admitted: bool,
}

/// Runtime adapter used by generated choreography runners.
///
/// This adapter implements the `ChoreographicAdapter` trait from telltale-choreography,
/// bridging Aura's effect system with the choreographic programming model.
///
/// ## Features
///
/// - **Role Family Support**: For protocols with parameterized roles (e.g., `Witness[N]`),
///   use `with_role_family()` to register role instances.
/// - **Guard Chain Enforcement**: When configured via `with_guard_config()`, evaluates
///   CapGuard and FlowGuard before each send operation.
/// - **Journal Coupling**: When configured via `with_journal_coupler()`, records journal
///   facts after successful sends.
#[allow(dead_code)]
pub struct AuraProtocolAdapter<E, R>
where
    E: ChoreographicEffects
        + GuardEffects
        + GuardContextProvider
        + aura_core::PhysicalTimeEffects
        + aura_core::TimeEffects,
    R: RoleId,
{
    effects: Arc<E>,
    authority_id: AuthorityId,
    self_role: R,
    role_map: HashMap<R, AuthorityId>,
    /// Role family registry: maps family names (e.g., "Witness") to role instances
    role_families: HashMap<String, Vec<R>>,
    outbound: VecDeque<Box<dyn Any + Send>>,
    branch_choices: VecDeque<R::Label>,
    received: Vec<ReceivedMessage>,
    message_provider: Option<MessageProviderFn<R>>,
    branch_decider: Option<BranchDeciderFn>,
    /// Guard chain configuration for send operations
    guard_config: GuardConfig,
    /// Journal coupler for fact recording after successful sends
    journal_coupler: Option<JournalCoupler>,
    /// Optional theorem-pack/runtime capability admission gate.
    runtime_admission: Option<RuntimeAdmissionConfig>,
}

#[allow(dead_code)]
impl<E, R> AuraProtocolAdapter<E, R>
where
    E: ChoreographicEffects
        + GuardEffects
        + GuardContextProvider
        + aura_core::PhysicalTimeEffects
        + aura_core::TimeEffects,
    R: RoleId,
{
    /// Create a new protocol adapter.
    ///
    /// # Arguments
    ///
    /// * `effects` - The effect system implementation
    /// * `authority_id` - The local authority's ID
    /// * `self_role` - The role this adapter plays in the protocol
    /// * `role_map` - Mapping from roles to authority IDs
    pub fn new(
        effects: Arc<E>,
        authority_id: AuthorityId,
        self_role: R,
        role_map: HashMap<R, AuthorityId>,
    ) -> Self {
        Self {
            effects,
            authority_id,
            self_role,
            role_map,
            role_families: HashMap::new(),
            outbound: VecDeque::new(),
            branch_choices: VecDeque::new(),
            received: Vec::new(),
            message_provider: None,
            branch_decider: None,
            guard_config: GuardConfig::default(),
            journal_coupler: None,
            runtime_admission: None,
        }
    }

    /// Register a role family for broadcast/collect operations.
    ///
    /// This is used for protocols with parameterized roles like `Witness[N]`.
    /// The family name should match the role name in the choreography definition.
    ///
    /// # Example
    ///
    /// ```ignore
    /// // For a choreography with `roles Coordinator, Witness[N]`
    /// let adapter = AuraProtocolAdapter::new(effects, auth_id, role, role_map)
    ///     .with_role_family("Witness", vec![witness0, witness1, witness2]);
    /// ```
    pub fn with_role_family(mut self, family: impl Into<String>, roles: Vec<R>) -> Self {
        self.role_families.insert(family.into(), roles);
        self
    }

    /// Register multiple role families at once.
    pub fn with_role_families(
        mut self,
        families: impl IntoIterator<Item = (String, Vec<R>)>,
    ) -> Self {
        for (family, roles) in families {
            self.role_families.insert(family, roles);
        }
        self
    }

    /// Provide outbound messages dynamically when the queue is empty.
    pub fn with_message_provider(
        mut self,
        provider: impl FnMut(MessageRequest<R>, &[ReceivedMessage]) -> Option<Box<dyn Any + Send>>
            + Send
            + 'static,
    ) -> Self {
        self.message_provider = Some(Box::new(provider));
        self
    }

    /// Provide branch decisions dynamically based on received messages.
    pub fn with_branch_decider(
        mut self,
        decider: impl FnMut(&[ReceivedMessage]) -> Option<String> + Send + 'static,
    ) -> Self {
        self.branch_decider = Some(Box::new(decider));
        self
    }

    /// Configure guard chain enforcement for send operations.
    ///
    /// When configured, the adapter will evaluate guard chain requirements
    /// (capability checks and flow budget charges) before each send operation.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let guard_config = GuardConfig::new(context_id)
    ///     .with_message_guard::<MyMessage>("cap:my_capability", 100);
    ///
    /// let adapter = AuraProtocolAdapter::new(effects, auth_id, role, role_map)
    ///     .with_guard_config(guard_config);
    /// ```
    pub fn with_guard_config(mut self, config: GuardConfig) -> Self {
        self.guard_config = config;
        self
    }

    /// Get the current guard configuration.
    pub fn guard_config(&self) -> &GuardConfig {
        &self.guard_config
    }

    /// Configure journal coupler for fact recording after successful sends.
    ///
    /// When configured, the adapter will call `couple_with_send` after each
    /// successful send operation to record journal facts per choreography annotations.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use aura_guards::guards::journal::{JournalCoupler, JournalCouplerBuilder};
    /// use aura_mpst::journal::{JournalAnnotation, JournalOpType};
    ///
    /// let coupler = JournalCouplerBuilder::new()
    ///     .with_annotation(
    ///         "send_coupling",
    ///         JournalAnnotation::add_facts("Protocol message sent"),
    ///     )
    ///     .build();
    ///
    /// let adapter = AuraProtocolAdapter::new(effects, auth_id, role, role_map)
    ///     .with_journal_coupler(coupler);
    /// ```
    pub fn with_journal_coupler(mut self, coupler: JournalCoupler) -> Self {
        self.journal_coupler = Some(coupler);
        self
    }

    /// Configure runtime capability admission checks for this choreography execution.
    pub fn with_runtime_capability_admission(
        mut self,
        capability_effects: Arc<dyn RuntimeCapabilityEffects>,
        required_capabilities: Vec<CapabilityKey>,
    ) -> Self {
        self.runtime_admission = Some(RuntimeAdmissionConfig {
            capability_effects,
            required_capabilities,
            admitted: false,
        });
        self
    }

    /// Get a reference to the journal coupler if configured.
    pub fn journal_coupler(&self) -> Option<&JournalCoupler> {
        self.journal_coupler.as_ref()
    }

    pub fn push_message<M: Send + 'static>(&mut self, message: M) {
        self.outbound.push_back(Box::new(message));
    }

    pub fn push_branch_choice(&mut self, label: R::Label) {
        self.branch_choices.push_back(label);
    }

    pub fn received_messages(&self) -> &[ReceivedMessage] {
        &self.received
    }

    async fn ensure_runtime_admission(&mut self) -> Result<(), AuraChoreographyError> {
        let Some(admission) = self.runtime_admission.as_mut() else {
            return Ok(());
        };

        if admission.admitted {
            return Ok(());
        }

        let inventory = admission
            .capability_effects
            .capability_inventory()
            .await
            .map_err(|_| AuraChoreographyError::AuthorizationFailed {
                reason: "TheoremPackAdmission inventory unavailable".to_string(),
            })?;
        let _inventory_size = inventory.len();

        if let Err(error) = admission
            .capability_effects
            .require_capabilities(&admission.required_capabilities)
            .await
        {
            let reason = match &error {
                AdmissionError::MissingCapability { capability } => format!(
                    "TheoremPackAdmission failed: missing runtime capability ref={}",
                    capability_key_ref(capability.as_str())
                ),
                AdmissionError::MissingRuntimeContracts => {
                    "TheoremPackAdmission failed: missing runtime contracts".to_string()
                }
                AdmissionError::InventoryUnavailable { .. } => {
                    "TheoremPackAdmission failed: inventory unavailable".to_string()
                }
                AdmissionError::Internal { .. } => "TheoremPackAdmission failed".to_string(),
            };
            return Err(AuraChoreographyError::AuthorizationFailed { reason });
        }
        admission.admitted = true;
        Ok(())
    }

    pub async fn start_session(&mut self, session_id: Uuid) -> Result<(), AuraChoreographyError> {
        self.ensure_runtime_admission()
            .await
            .map_err(|error| TelltaleChoreographyError::ExecutionError(error.to_string()))?;
        let mut roles = Vec::new();
        let self_role = self.map_role(self.self_role)?;
        roles.push(self_role);

        for role in self.role_map.keys() {
            let mapped = self.map_role(*role)?;
            if mapped != self_role {
                roles.push(mapped);
            }
        }

        self.effects.start_session(session_id, roles).await
    }

    pub async fn end_session(&mut self) -> Result<(), AuraChoreographyError> {
        self.effects.end_session().await
    }

    async fn send_value<M: serde::Serialize + Send + Sync>(
        &mut self,
        to: R,
        msg: &M,
    ) -> Result<(), AuraChoreographyError> {
        self.ensure_runtime_admission().await?;
        let role = self.map_role(to)?;
        let payload = to_vec(msg).map_err(|err| AuraChoreographyError::SerializationFailed {
            reason: err.to_string(),
        })?;

        let mut guard_receipt: Option<aura_core::Receipt> = None;
        let type_name = std::any::type_name::<M>();

        if let Some(context_id) = self.guard_config.context_id {
            if let Some(guard_req) = self.guard_config.get_guard(type_name) {
                let peer = self.role_map.get(&to).copied().unwrap_or(self.authority_id);

                debug!(
                    message_type = type_name,
                    capability = %guard_req.capability,
                    flow_cost = ?guard_req.flow_cost,
                    context = ?context_id,
                    peer = ?peer,
                    "Evaluating guard chain for choreography send"
                );

                let mut guard = SendGuardChain::new(
                    aura_guards::guards::CapabilityId::from(guard_req.capability.as_str()),
                    context_id,
                    peer,
                    guard_req.flow_cost,
                );

                if let Some(ref leakage) = guard_req.leakage_budget {
                    guard = guard.with_leakage_budget(leakage.clone());
                }

                let result = guard.evaluate(&*self.effects).await.map_err(|e| {
                    AuraChoreographyError::InternalError {
                        message: format!("guard chain evaluation failed: {e}"),
                    }
                })?;

                if !result.authorized {
                    return Err(AuraChoreographyError::ProtocolViolation {
                        message: result
                            .denial_reason
                            .unwrap_or_else(|| "guard chain denied send".to_string()),
                    });
                }

                debug!(
                    message_type = type_name,
                    receipt = ?result.receipt,
                    "Guard chain authorized choreography send"
                );

                guard_receipt = result.receipt;
            }
        }

        self.effects.send_to_role_bytes(role, payload).await?;

        if let Some(ref coupler) = self.journal_coupler {
            debug!(
                message_type = type_name,
                "Coupling journal operations after choreography send"
            );

            let coupling_result = coupler
                .couple_with_send(&*self.effects, &guard_receipt)
                .await
                .map_err(|e| {
                    warn!(
                        message_type = type_name,
                        error = %e,
                        "Journal coupling failed after send (message was sent)"
                    );
                    AuraChoreographyError::InternalError {
                        message: format!("journal coupling failed: {e}"),
                    }
                })?;

            if coupling_result.operations_applied > 0usize {
                debug!(
                    message_type = type_name,
                    operations_applied = coupling_result.operations_applied,
                    "Journal coupling completed successfully"
                );
            }
        }

        Ok(())
    }

    async fn recv_value<M: serde::de::DeserializeOwned + Send>(
        &mut self,
        from: R,
    ) -> Result<M, AuraChoreographyError> {
        self.ensure_runtime_admission().await?;
        let role = self.map_role(from)?;
        let payload = self.effects.receive_from_role_bytes(role).await?;
        self.received.push(ReceivedMessage {
            type_name: std::any::type_name::<M>(),
            bytes: payload.clone(),
        });
        from_slice(&payload).map_err(|err| AuraChoreographyError::DeserializationFailed {
            reason: err.to_string(),
        })
    }

    fn map_role(&self, role: R) -> Result<ChoreographicRole, AuraChoreographyError> {
        let authority_id = if role == self.self_role {
            self.authority_id
        } else {
            *self
                .role_map
                .get(&role)
                .ok_or_else(|| AuraChoreographyError::RoleNotFound {
                    role: ChoreographicRole::new(
                        aura_core::DeviceId::new_from_entropy([0u8; 32]),
                        self.authority_id,
                        RoleIndex::new(0).expect("role index"),
                    ),
                })?
        };

        let role_index = role.role_index().unwrap_or(0);
        let role_index =
            RoleIndex::new(role_index).ok_or_else(|| AuraChoreographyError::ProtocolViolation {
                message: format!("invalid role index: {role_index}"),
            })?;

        Ok(ChoreographicRole::for_authority(authority_id, role_index))
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[async_trait]
impl<E, R> ChoreoHandler for AuraProtocolAdapter<E, R>
where
    E: ChoreographicEffects
        + GuardEffects
        + GuardContextProvider
        + aura_core::PhysicalTimeEffects
        + aura_core::TimeEffects,
    R: RoleId,
{
    type Role = R;
    type Endpoint = ();

    async fn send<M: serde::Serialize + Send + Sync>(
        &mut self,
        _ep: &mut Self::Endpoint,
        to: Self::Role,
        msg: &M,
    ) -> ChoreoResult<()> {
        self.send_value(to, msg).await.map_err(map_runtime_error)
    }

    async fn recv<M: serde::de::DeserializeOwned + Send>(
        &mut self,
        _ep: &mut Self::Endpoint,
        from: Self::Role,
    ) -> ChoreoResult<M> {
        self.recv_value(from).await.map_err(map_runtime_error)
    }

    async fn choose(
        &mut self,
        ep: &mut Self::Endpoint,
        who: Self::Role,
        label: <Self::Role as RoleId>::Label,
    ) -> ChoreoResult<()> {
        self.send(ep, who, &label.as_str().to_string()).await
    }

    async fn offer(
        &mut self,
        ep: &mut Self::Endpoint,
        from: Self::Role,
    ) -> ChoreoResult<<Self::Role as RoleId>::Label> {
        let label: String = self.recv(ep, from).await?;
        <Self::Role as RoleId>::Label::from_str(&label).ok_or_else(|| {
            TelltaleChoreographyError::InvalidChoice {
                expected: Vec::new(),
                actual: label,
            }
        })
    }

    async fn with_timeout<F, T>(
        &mut self,
        _ep: &mut Self::Endpoint,
        _at: Self::Role,
        dur: Duration,
        body: F,
    ) -> ChoreoResult<T>
    where
        F: std::future::Future<Output = ChoreoResult<T>> + Send,
    {
        tokio::time::timeout(dur, body)
            .await
            .map_err(|_| TelltaleChoreographyError::Timeout(dur))?
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[async_trait]
impl<E, R> ChoreoHandlerExt for AuraProtocolAdapter<E, R>
where
    E: ChoreographicEffects
        + GuardEffects
        + GuardContextProvider
        + aura_core::PhysicalTimeEffects
        + aura_core::TimeEffects,
    R: RoleId,
{
    async fn setup(&mut self, _role: Self::Role) -> ChoreoResult<Self::Endpoint> {
        Ok(())
    }

    async fn teardown(&mut self, _ep: Self::Endpoint) -> ChoreoResult<()> {
        Ok(())
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl<E, R> aura_mpst::ChoreographicAdapter for AuraProtocolAdapter<E, R>
where
    E: ChoreographicEffects
        + GuardEffects
        + GuardContextProvider
        + aura_core::PhysicalTimeEffects
        + aura_core::TimeEffects,
    R: RoleId,
{
    type Error = AuraChoreographyError;
    type Role = R;

    async fn send<M: serde::Serialize + Send + Sync + 'static>(
        &mut self,
        to: Self::Role,
        msg: M,
    ) -> Result<(), Self::Error> {
        self.send_value(to, &msg).await
    }

    async fn recv<M: serde::de::DeserializeOwned + Send + 'static>(
        &mut self,
        from: Self::Role,
    ) -> Result<M, Self::Error> {
        self.recv_value(from).await
    }

    fn resolve_family(&self, family: &str) -> Result<Vec<Self::Role>, Self::Error> {
        self.role_families
            .get(family)
            .cloned()
            .ok_or_else(|| AuraChoreographyError::RoleFamilyNotFound {
                family: family.to_string(),
            })
            .and_then(|roles| {
                if roles.is_empty() {
                    Err(AuraChoreographyError::EmptyRoleFamily {
                        family: family.to_string(),
                    })
                } else {
                    Ok(roles)
                }
            })
    }

    fn resolve_range(
        &self,
        family: &str,
        start: u32,
        end: u32,
    ) -> Result<Vec<Self::Role>, Self::Error> {
        let roles = self.resolve_family(family)?;
        let start_idx = start as usize;
        let end_idx = end as usize;
        if start_idx >= roles.len() || end_idx > roles.len() || start_idx >= end_idx {
            return Err(AuraChoreographyError::InvalidRoleFamilyRange {
                family: family.to_string(),
                start,
                end,
            });
        }
        Ok(roles[start_idx..end_idx].to_vec())
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl<E, R> ChoreographicAdapterExt for AuraProtocolAdapter<E, R>
where
    E: ChoreographicEffects
        + GuardEffects
        + GuardContextProvider
        + aura_core::PhysicalTimeEffects
        + aura_core::TimeEffects,
    R: RoleId,
{
    async fn setup(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    async fn teardown(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    async fn provide_message<M: Send + 'static>(
        &mut self,
        to: Self::Role,
    ) -> Result<M, Self::Error> {
        self.ensure_runtime_admission().await?;
        let boxed = match self.outbound.pop_front() {
            Some(boxed) => boxed,
            None => {
                if let Some(provider) = self.message_provider.as_mut() {
                    provider(
                        MessageRequest {
                            to,
                            type_name: std::any::type_name::<M>(),
                        },
                        &self.received,
                    )
                    .ok_or_else(|| {
                        AuraChoreographyError::ProtocolViolation {
                            message: "message provider returned None".to_string(),
                        }
                    })?
                } else {
                    return Err(AuraChoreographyError::ProtocolViolation {
                        message: "no queued message for provide_message".to_string(),
                    });
                }
            }
        };

        boxed.downcast::<M>().map(|msg| *msg).map_err(|_| {
            AuraChoreographyError::ProtocolViolation {
                message: format!(
                    "queued message type mismatch (expected {})",
                    std::any::type_name::<M>()
                ),
            }
        })
    }

    async fn select_branch<L: LabelId>(&mut self, choices: &[L]) -> Result<L, Self::Error> {
        self.ensure_runtime_admission().await?;
        let choice = match self.branch_choices.pop_front() {
            Some(choice) => choice,
            None => {
                if let Some(decider) = self.branch_decider.as_mut() {
                    let label = decider(&self.received).ok_or_else(|| {
                        AuraChoreographyError::ProtocolViolation {
                            message: "branch decider returned None".to_string(),
                        }
                    })?;
                    let selected = choices
                        .iter()
                        .copied()
                        .find(|choice| choice.as_str() == label);
                    return selected.ok_or_else(|| AuraChoreographyError::ProtocolViolation {
                        message: format!("branch decider returned invalid label: {label}"),
                    });
                }
                return Err(AuraChoreographyError::ProtocolViolation {
                    message: "no queued branch choice for select_branch".to_string(),
                });
            }
        };

        let selected = choices
            .iter()
            .copied()
            .find(|label| label.as_str() == choice.as_str());

        selected.ok_or_else(|| AuraChoreographyError::ProtocolViolation {
            message: "queued branch choice is not valid for this choice".to_string(),
        })
    }
}

/// Public API alias for the choreography adapter.
pub use AuraHandlerAdapter as ChoreographyAdapter;

fn map_runtime_role(
    result: Result<ChoreographicRole, AuraChoreographyError>,
) -> ChoreoResult<ChoreographicRole> {
    result.map_err(|error| TelltaleChoreographyError::ExecutionError(error.to_string()))
}

fn map_runtime_error(error: AuraChoreographyError) -> TelltaleChoreographyError {
    TelltaleChoreographyError::ExecutionError(error.to_string())
}

fn map_telltale_error(error: TelltaleChoreographyError) -> AuraChoreographyError {
    aura_protocol::effects::choreographic::map_telltale_choreography_error(error)
}

fn capability_key_ref(key: &str) -> String {
    let digest = hash(key.as_bytes());
    hex::encode(&digest[..8])
}
