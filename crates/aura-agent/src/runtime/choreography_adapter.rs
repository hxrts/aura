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
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_core::util::serialization::{from_slice, to_vec};
use aura_core::FlowCost;
use aura_guards::guards::journal::JournalCoupler;
use aura_guards::prelude::{GuardContextProvider, GuardEffects, SendGuardChain};
use aura_guards::LeakageBudget;
use aura_mpst::rumpsteak_aura_choreography::{LabelId, Message, RoleId};
use aura_mpst::ChoreographicAdapterExt;
use aura_protocol::effects::{
    ChoreographicEffects, ChoreographicRole, ChoreographyError, RoleIndex,
};
use std::any::Any;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
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
/// This adapter implements the `ChoreographicAdapter` trait from rumpsteak-aura,
/// bridging Aura's effect system with the choreographic programming model.
///
/// ## Role Family Support
///
/// For protocols with parameterized roles (e.g., `Witness[N]`), use `with_role_family()`
/// to register role instances that can be resolved during broadcast/collect operations.
type MessageProviderFn<R> =
    Box<dyn FnMut(MessageRequest<R>, &[ReceivedMessage]) -> Option<Box<dyn Any + Send>> + Send>;

type BranchDeciderFn = Box<dyn FnMut(&[ReceivedMessage]) -> Option<String> + Send>;

/// Runtime adapter used by generated choreography runners.
///
/// This adapter implements the `ChoreographicAdapter` trait from rumpsteak-aura,
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

    /// Get a reference to the journal coupler if configured.
    pub fn journal_coupler(&self) -> Option<&JournalCoupler> {
        self.journal_coupler.as_ref()
    }

    pub fn push_message<M: Message>(&mut self, message: M) {
        self.outbound.push_back(Box::new(message));
    }

    pub fn push_branch_choice(&mut self, label: R::Label) {
        self.branch_choices.push_back(label);
    }

    pub fn received_messages(&self) -> &[ReceivedMessage] {
        &self.received
    }

    pub async fn start_session(&mut self, session_id: Uuid) -> Result<(), ChoreographyError> {
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

    pub async fn end_session(&mut self) -> Result<(), ChoreographyError> {
        self.effects.end_session().await
    }

    fn map_role(&self, role: R) -> Result<ChoreographicRole, ChoreographyError> {
        let authority_id = if role == self.self_role {
            self.authority_id
        } else {
            *self
                .role_map
                .get(&role)
                .ok_or_else(|| ChoreographyError::RoleNotFound {
                    role: ChoreographicRole::new(
                        aura_core::DeviceId::from_uuid(self.authority_id.0),
                        RoleIndex::new(0).expect("role index"),
                    ),
                })?
        };

        let role_index = role.role_index().unwrap_or(0);
        let role_index =
            RoleIndex::new(role_index).ok_or_else(|| ChoreographyError::ProtocolViolation {
                message: format!("invalid role index: {role_index}"),
            })?;

        Ok(ChoreographicRole::new(
            aura_core::DeviceId::from_uuid(authority_id.0),
            role_index,
        ))
    }
}

#[async_trait]
impl<E, R> aura_mpst::rumpsteak_aura_choreography::ChoreographicAdapter
    for AuraProtocolAdapter<E, R>
where
    E: ChoreographicEffects
        + GuardEffects
        + GuardContextProvider
        + aura_core::PhysicalTimeEffects
        + aura_core::TimeEffects,
    R: RoleId,
{
    type Error = ChoreographyError;
    type Role = R;

    async fn send<M: Message>(&mut self, to: Self::Role, msg: M) -> Result<(), Self::Error> {
        let role = self.map_role(to)?;
        let payload = to_vec(&msg).map_err(|err| ChoreographyError::SerializationFailed {
            reason: err.to_string(),
        })?;

        // Track receipt from guard evaluation for journal coupling
        let mut guard_receipt: Option<aura_core::Receipt> = None;
        let type_name = std::any::type_name::<M>();

        // Evaluate guard chain if configured
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
                    ChoreographyError::AuthorizationFailed {
                        reason: format!("Guard chain evaluation failed: {e}"),
                    }
                })?;

                if !result.authorized {
                    return Err(ChoreographyError::AuthorizationFailed {
                        reason: result
                            .denial_reason
                            .unwrap_or_else(|| "Guard chain denied send".to_string()),
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

        // Send the message
        self.effects.send_to_role_bytes(role, payload).await?;

        // Couple journal operations after successful send
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
                    ChoreographyError::InternalError {
                        message: format!("Journal coupling failed: {e}"),
                    }
                })?;

            if coupling_result.operations_applied > 0 {
                debug!(
                    message_type = type_name,
                    operations_applied = coupling_result.operations_applied,
                    "Journal coupling completed successfully"
                );
            }
        }

        Ok(())
    }

    async fn recv<M: Message>(&mut self, from: Self::Role) -> Result<M, Self::Error> {
        let role = self.map_role(from)?;
        let payload = self.effects.receive_from_role_bytes(role).await?;
        self.received.push(ReceivedMessage {
            type_name: std::any::type_name::<M>(),
            bytes: payload.clone(),
        });
        from_slice(&payload).map_err(|err| ChoreographyError::DeserializationFailed {
            reason: err.to_string(),
        })
    }

    /// Resolve all instances of a parameterized role family.
    ///
    /// For a choreography with `roles Coordinator, Witness[N]`, calling
    /// `resolve_family("Witness")` returns all registered witness roles.
    fn resolve_family(&self, family: &str) -> Result<Vec<Self::Role>, Self::Error> {
        let roles = self.role_families.get(family).ok_or_else(|| {
            ChoreographyError::RoleFamilyNotFound {
                family: family.to_string(),
            }
        })?;

        if roles.is_empty() {
            return Err(ChoreographyError::EmptyRoleFamily {
                family: family.to_string(),
            });
        }

        Ok(roles.clone())
    }

    /// Resolve a range of role instances [start, end).
    ///
    /// For a choreography with `Witness[0..3]`, this returns witnesses at indices 0, 1, 2.
    fn resolve_range(
        &self,
        family: &str,
        start: u32,
        end: u32,
    ) -> Result<Vec<Self::Role>, Self::Error> {
        let all_roles = self.role_families.get(family).ok_or_else(|| {
            ChoreographyError::RoleFamilyNotFound {
                family: family.to_string(),
            }
        })?;

        let start_idx = start as usize;
        let end_idx = end as usize;

        if start_idx >= all_roles.len() || end_idx > all_roles.len() || start_idx >= end_idx {
            return Err(ChoreographyError::InvalidRoleFamilyRange {
                family: family.to_string(),
                start,
                end,
            });
        }

        let roles: Vec<Self::Role> = all_roles[start_idx..end_idx].to_vec();

        if roles.is_empty() {
            return Err(ChoreographyError::EmptyRoleFamily {
                family: family.to_string(),
            });
        }

        Ok(roles)
    }
}

#[async_trait]
impl<E, R> ChoreographicAdapterExt for AuraProtocolAdapter<E, R>
where
    E: ChoreographicEffects
        + GuardEffects
        + GuardContextProvider
        + aura_core::PhysicalTimeEffects
        + aura_core::TimeEffects,
    R: RoleId,
{
    async fn provide_message<M: Message>(&mut self, to: Self::Role) -> Result<M, Self::Error> {
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
                    .ok_or_else(|| ChoreographyError::ProtocolViolation {
                        message: "message provider returned None".to_string(),
                    })?
                } else {
                    return Err(ChoreographyError::ProtocolViolation {
                        message: "no queued message for provide_message".to_string(),
                    });
                }
            }
        };

        boxed
            .downcast::<M>()
            .map(|msg| *msg)
            .map_err(|_| ChoreographyError::ProtocolViolation {
                message: format!(
                    "queued message type mismatch (expected {})",
                    std::any::type_name::<M>()
                ),
            })
    }

    async fn select_branch<L: LabelId>(&mut self, choices: &[L]) -> Result<L, Self::Error> {
        let choice = match self.branch_choices.pop_front() {
            Some(choice) => choice,
            None => {
                if let Some(decider) = self.branch_decider.as_mut() {
                    let label = decider(&self.received).ok_or_else(|| {
                        ChoreographyError::ProtocolViolation {
                            message: "branch decider returned None".to_string(),
                        }
                    })?;
                    let selected = choices
                        .iter()
                        .copied()
                        .find(|choice| choice.as_str() == label);
                    return selected.ok_or_else(|| ChoreographyError::ProtocolViolation {
                        message: "branch decider returned invalid label".to_string(),
                    });
                }
                return Err(ChoreographyError::ProtocolViolation {
                    message: "no queued branch choice for select_branch".to_string(),
                });
            }
        };

        let selected = choices
            .iter()
            .copied()
            .find(|label| label.as_str() == choice.as_str());

        selected.ok_or_else(|| ChoreographyError::ProtocolViolation {
            message: "queued branch choice is not valid for this choice".to_string(),
        })
    }
}

/// Public API alias for the choreography adapter.
pub type ChoreographyAdapter = AuraHandlerAdapter;
