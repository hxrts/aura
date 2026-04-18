use crate::{
    effects::task::{CancellationToken, NeverCancel, TaskSpawner},
    effects::{CapabilityKey, CapabilityTokenRequest},
    AuraError, ProtocolErrorCode, TimeoutBudget,
};
use futures::future::{BoxFuture, LocalBoxFuture};
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::marker::PhantomData;
use std::sync::Arc;

/// Repo-wide ownership taxonomy for parity-critical surfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnershipCategory {
    Pure,
    MoveOwned,
    ActorOwned,
    Observed,
}

/// Declaration-time ownership boundary categories for proc-macro-enforced
/// parity-critical surfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoundaryDeclarationCategory {
    MoveOwned,
    ActorOwned,
    CapabilityGated,
}

/// Required frontend/app handoff policy for parity-critical semantic owners.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticOwnerHandoffPolicy {
    /// The frontend/local owner must settle the operation locally.
    FrontendSettlesLocally,
    /// The frontend/local owner must relinquish ownership before the first
    /// awaited app/runtime workflow step.
    HandoffBeforeFirstAwait,
}

/// Allowed await policy for parity-critical semantic-owner bodies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticOwnerAwaitPolicy {
    /// Only approved bounded-await helpers are allowed inside the owner body.
    BoundedOnly,
}

/// Required relationship between terminal publication and best-effort follow-up work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticOwnerBestEffortPolicy {
    /// Terminal publication must happen before best-effort follow-up that is
    /// allowed to fail.
    TerminalBeforeBestEffort,
}

/// Canonical protocol for parity-critical semantic owners.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SemanticOwnerProtocol {
    handoff_policy: SemanticOwnerHandoffPolicy,
    await_policy: SemanticOwnerAwaitPolicy,
    best_effort_policy: SemanticOwnerBestEffortPolicy,
}

impl SemanticOwnerProtocol {
    /// Canonical Aura semantic-owner protocol.
    pub const CANONICAL: Self = Self {
        handoff_policy: SemanticOwnerHandoffPolicy::HandoffBeforeFirstAwait,
        await_policy: SemanticOwnerAwaitPolicy::BoundedOnly,
        best_effort_policy: SemanticOwnerBestEffortPolicy::TerminalBeforeBestEffort,
    };

    pub const fn handoff_policy(self) -> SemanticOwnerHandoffPolicy {
        self.handoff_policy
    }

    pub const fn await_policy(self) -> SemanticOwnerAwaitPolicy {
        self.await_policy
    }

    pub const fn best_effort_policy(self) -> SemanticOwnerBestEffortPolicy {
        self.best_effort_policy
    }
}

/// Declared authoritative postcondition for a semantic owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SemanticOwnerPostcondition {
    name: &'static str,
}

impl SemanticOwnerPostcondition {
    pub const fn new(name: &'static str) -> Self {
        Self { name }
    }

    pub const fn name(self) -> &'static str {
        self.name
    }
}

/// Typed proof that a semantic owner's declared postcondition now holds.
///
/// Proofs are witnesses of state truth, not authority tokens. They should be
/// minted only by sanctioned capability-gated helpers after the authoritative
/// state transition has actually been established.
pub trait SemanticSuccessProof {
    fn declared_postcondition(&self) -> SemanticOwnerPostcondition;
}

/// Declared dependency edge for a semantic owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SemanticOwnerDependency {
    name: &'static str,
}

impl SemanticOwnerDependency {
    pub const fn new(name: &'static str) -> Self {
        Self { name }
    }

    pub const fn name(self) -> &'static str {
        self.name
    }
}

/// Declared authoritative input kind for a semantic owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SemanticOwnerAuthoritativeInput {
    name: &'static str,
}

impl SemanticOwnerAuthoritativeInput {
    pub const fn new(name: &'static str) -> Self {
        Self { name }
    }

    pub const fn name(self) -> &'static str {
        self.name
    }
}

/// Declared child-operation allowance for a semantic owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SemanticOwnerChildOperation {
    name: &'static str,
}

impl SemanticOwnerChildOperation {
    pub const fn new(name: &'static str) -> Self {
        Self { name }
    }

    pub const fn name(self) -> &'static str {
        self.name
    }
}

/// Sanctioned child-operation spawner for semantic owners that must delegate
/// required continuation work into an explicit child operation.
#[derive(Clone)]
pub struct ChildOperationSpawner {
    inner: OwnedTaskSpawner,
}

impl std::fmt::Debug for ChildOperationSpawner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChildOperationSpawner")
            .finish_non_exhaustive()
    }
}

impl ChildOperationSpawner {
    pub fn new(inner: OwnedTaskSpawner) -> Self {
        Self { inner }
    }

    pub fn shutdown_token(&self) -> &OwnedShutdownToken {
        self.inner.shutdown_token()
    }

    pub fn spawn_child_operation(
        &self,
        _child: SemanticOwnerChildOperation,
        fut: BoxFuture<'static, ()>,
    ) {
        self.inner.spawn(fut);
    }

    pub fn spawn_local_child_operation(
        &self,
        _child: SemanticOwnerChildOperation,
        fut: LocalBoxFuture<'static, ()>,
    ) {
        self.inner.spawn_local(fut);
    }
}

/// Canonical protocol for best-effort boundaries that are not allowed to own
/// primary terminal lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BestEffortBoundaryProtocol {
    terminal_relation: SemanticOwnerBestEffortPolicy,
}

impl BestEffortBoundaryProtocol {
    /// Canonical Aura best-effort protocol.
    pub const POST_TERMINAL_ONLY: Self = Self {
        terminal_relation: SemanticOwnerBestEffortPolicy::TerminalBeforeBestEffort,
    };

    pub const fn terminal_relation(self) -> SemanticOwnerBestEffortPolicy {
        self.terminal_relation
    }
}

/// Canonical collector for post-terminal best-effort work.
///
/// This helper is intentionally incapable of publishing primary terminal
/// lifecycle. It only records best-effort failures after the owner has already
/// published terminal success/failure through the canonical lifecycle path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostTerminalBestEffort<E> {
    protocol: BestEffortBoundaryProtocol,
    first_error: Option<E>,
}

impl<E> PostTerminalBestEffort<E> {
    #[must_use]
    pub const fn post_terminal_only() -> Self {
        Self {
            protocol: BestEffortBoundaryProtocol::POST_TERMINAL_ONLY,
            first_error: None,
        }
    }

    #[must_use]
    pub const fn protocol(&self) -> BestEffortBoundaryProtocol {
        self.protocol
    }

    pub fn record<T>(&mut self, result: Result<T, E>) -> Option<T> {
        match result {
            Ok(value) => Some(value),
            Err(error) => {
                if self.first_error.is_none() {
                    self.first_error = Some(error);
                }
                None
            }
        }
    }

    pub async fn capture<T, Fut>(&mut self, future: Fut) -> Option<T>
    where
        Fut: Future<Output = Result<T, E>>,
    {
        self.record(future.await)
    }

    #[must_use]
    pub fn first_error(&self) -> Option<&E> {
        self.first_error.as_ref()
    }

    pub fn finish(self) -> Result<(), E> {
        match self.first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

/// High-level typed error domain for ownership and lifecycle failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnershipErrorDomain {
    Ownership,
    Capability,
    Lifecycle,
    Timeout,
}

/// Typed ownership and lifecycle failures used by parity-critical boundaries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
pub enum OwnershipError {
    #[error("missing capability: {capability}")]
    MissingCapability { capability: String },
    #[error("stale owner token: {detail}")]
    StaleOwner { detail: String },
    #[error("invalid ownership transfer: {detail}")]
    InvalidTransfer { detail: String },
    #[error("owner dropped before terminal publication: {detail}")]
    OwnerDropped { detail: String },
    #[error("terminal lifecycle regression: {detail}")]
    TerminalRegression { detail: String },
    #[error("operation timed out: {detail}")]
    Timeout { detail: String },
}

impl OwnershipError {
    fn into_detail(detail: impl Into<String>) -> String {
        detail.into()
    }

    pub fn missing_capability(capability: impl Into<String>) -> Self {
        Self::MissingCapability {
            capability: capability.into(),
        }
    }

    pub fn stale_owner(detail: impl Into<String>) -> Self {
        Self::StaleOwner {
            detail: Self::into_detail(detail),
        }
    }

    pub fn invalid_transfer(detail: impl Into<String>) -> Self {
        Self::InvalidTransfer {
            detail: Self::into_detail(detail),
        }
    }

    pub fn owner_dropped(detail: impl Into<String>) -> Self {
        Self::OwnerDropped {
            detail: Self::into_detail(detail),
        }
    }

    pub fn terminal_regression(detail: impl Into<String>) -> Self {
        Self::TerminalRegression {
            detail: Self::into_detail(detail),
        }
    }

    pub fn timeout(detail: impl Into<String>) -> Self {
        Self::Timeout {
            detail: Self::into_detail(detail),
        }
    }

    pub fn domain(&self) -> OwnershipErrorDomain {
        match self {
            Self::MissingCapability { .. } => OwnershipErrorDomain::Capability,
            Self::StaleOwner { .. } | Self::InvalidTransfer { .. } => {
                OwnershipErrorDomain::Ownership
            }
            Self::OwnerDropped { .. } | Self::TerminalRegression { .. } => {
                OwnershipErrorDomain::Lifecycle
            }
            Self::Timeout { .. } => OwnershipErrorDomain::Timeout,
        }
    }
}

impl ProtocolErrorCode for OwnershipError {
    fn code(&self) -> &'static str {
        match self {
            Self::MissingCapability { .. } => "missing_capability",
            Self::StaleOwner { .. } => "stale_owner",
            Self::InvalidTransfer { .. } => "invalid_transfer",
            Self::OwnerDropped { .. } => "owner_dropped",
            Self::TerminalRegression { .. } => "terminal_regression",
            Self::Timeout { .. } => "timeout",
        }
    }
}

impl From<OwnershipError> for AuraError {
    fn from(value: OwnershipError) -> Self {
        match value {
            OwnershipError::MissingCapability { capability } => {
                AuraError::permission_denied(format!("missing_capability: {capability}"))
            }
            OwnershipError::StaleOwner { detail } => {
                AuraError::terminal(format!("stale_owner: {detail}"))
            }
            OwnershipError::InvalidTransfer { detail } => {
                AuraError::terminal(format!("invalid_transfer: {detail}"))
            }
            OwnershipError::OwnerDropped { detail } => {
                AuraError::terminal(format!("owner_dropped: {detail}"))
            }
            OwnershipError::TerminalRegression { detail } => {
                AuraError::internal(format!("terminal_regression: {detail}"))
            }
            OwnershipError::Timeout { detail } => AuraError::terminal(format!("timeout: {detail}")),
        }
    }
}

/// Standard result type for ownership/lifecycle boundaries.
pub type OwnershipResult<T> = std::result::Result<T, OwnershipError>;

pub trait OwnershipCapability {
    fn capability_key(&self) -> &CapabilityKey;
    fn into_capability_key(self) -> CapabilityKey
    where
        Self: Sized;

    fn biscuit_permission(&self) -> &str {
        self.capability_key().as_str()
    }
}

/// Build a standard capability-token request directly from typed ownership
/// capability wrappers of one wrapper family.
pub fn ownership_capability_token_request_for<C>(
    subject: impl Into<String>,
    capabilities: impl IntoIterator<Item = C>,
) -> CapabilityTokenRequest
where
    C: OwnershipCapability,
{
    let subject = subject.into();
    let permissions = capabilities
        .into_iter()
        .map(OwnershipCapability::into_capability_key)
        .map(|capability| capability.as_str().to_string())
        .collect::<Vec<_>>();
    CapabilityTokenRequest::standard(&subject, &permissions)
}

/// Typed owner epoch for move-owned operation boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OwnerEpoch(u64);

impl OwnerEpoch {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn value(self) -> u64 {
        self.0
    }
}

/// Typed publication sequence for exact-owner publication ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PublicationSequence(u64);

impl PublicationSequence {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn value(self) -> u64 {
        self.0
    }

    pub const fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }
}

/// Typed trace/span context for ownership-bearing operation boundaries.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct TraceContext {
    trace_id: Option<String>,
    span_id: Option<String>,
}

impl TraceContext {
    pub fn new(trace_id: Option<String>, span_id: Option<String>) -> Self {
        Self { trace_id, span_id }
    }

    pub const fn detached() -> Self {
        Self {
            trace_id: None,
            span_id: None,
        }
    }

    pub fn trace_id(&self) -> Option<&str> {
        self.trace_id.as_deref()
    }

    pub fn span_id(&self) -> Option<&str> {
        self.span_id.as_deref()
    }
}

/// Typed timeout budget surface for operation ownership contexts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationTimeoutBudget {
    Configured(TimeoutBudget),
    DeferredLocalPolicy,
}

impl OperationTimeoutBudget {
    pub fn configured(timeout_budget: TimeoutBudget) -> Self {
        Self::Configured(timeout_budget)
    }

    pub const fn deferred_local_policy() -> Self {
        Self::DeferredLocalPolicy
    }

    pub fn configured_budget(&self) -> Option<&TimeoutBudget> {
        match self {
            Self::Configured(timeout_budget) => Some(timeout_budget),
            Self::DeferredLocalPolicy => None,
        }
    }
}

/// Owned shutdown/cancellation token wrapper for parity-critical boundaries.
#[derive(Clone)]
pub struct OwnedShutdownToken {
    inner: Option<Arc<dyn CancellationToken>>,
}

impl std::fmt::Debug for OwnedShutdownToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OwnedShutdownToken")
            .field("attached", &self.inner.is_some())
            .finish()
    }
}

impl OwnedShutdownToken {
    pub fn attached(token: Arc<dyn CancellationToken>) -> Self {
        Self { inner: Some(token) }
    }

    pub const fn detached() -> Self {
        Self { inner: None }
    }

    pub async fn cancelled(&self) {
        match &self.inner {
            Some(token) => token.cancelled().await,
            None => NeverCancel.cancelled().await,
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner
            .as_ref()
            .is_some_and(|token| token.is_cancelled())
    }

    pub fn raw(&self) -> Option<&Arc<dyn CancellationToken>> {
        self.inner.as_ref()
    }
}

/// Owned task spawner wrapper for actor-owned or move-owned owners that may
/// create sanctioned background work.
#[derive(Clone)]
pub struct OwnedTaskSpawner {
    inner: Arc<dyn TaskSpawner>,
    shutdown: OwnedShutdownToken,
}

impl std::fmt::Debug for OwnedTaskSpawner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OwnedTaskSpawner").finish_non_exhaustive()
    }
}

impl OwnedTaskSpawner {
    pub fn new(inner: Arc<dyn TaskSpawner>, shutdown: OwnedShutdownToken) -> Self {
        Self { inner, shutdown }
    }

    pub fn shutdown_token(&self) -> &OwnedShutdownToken {
        &self.shutdown
    }

    pub fn spawn(&self, fut: BoxFuture<'static, ()>) {
        self.inner.spawn(fut);
    }

    pub fn spawn_cancellable(&self, fut: BoxFuture<'static, ()>) {
        let token = self
            .shutdown
            .raw()
            .cloned()
            .unwrap_or_else(|| Arc::new(NeverCancel));
        self.inner.spawn_cancellable(fut, token);
    }

    pub fn spawn_local(&self, fut: LocalBoxFuture<'static, ()>) {
        self.inner.spawn_local(fut);
    }

    pub fn spawn_local_cancellable(&self, fut: LocalBoxFuture<'static, ()>) {
        let token = self
            .shutdown
            .raw()
            .cloned()
            .unwrap_or_else(|| Arc::new(NeverCancel));
        self.inner.spawn_local_cancellable(fut, token);
    }
}

/// Opaque owned task handle metadata for parity-critical bookkeeping.
#[derive(Debug, Clone)]
pub struct OwnedTaskHandle<HandleId> {
    handle_id: HandleId,
    shutdown: OwnedShutdownToken,
}

impl<HandleId> OwnedTaskHandle<HandleId> {
    pub fn new(handle_id: HandleId, shutdown: OwnedShutdownToken) -> Self {
        Self {
            handle_id,
            shutdown,
        }
    }

    pub fn handle_id(&self) -> &HandleId {
        &self.handle_id
    }

    pub fn shutdown_token(&self) -> &OwnedShutdownToken {
        &self.shutdown
    }
}

/// Typed bounded actor-ingress/mailbox declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundedActorIngress<Domain, Message> {
    owner_name: &'static str,
    capacity: u32,
    _domain: PhantomData<fn() -> Domain>,
    _message: PhantomData<fn() -> Message>,
}

impl<Domain, Message> BoundedActorIngress<Domain, Message> {
    pub fn new(owner_name: &'static str, capacity: u32) -> Self {
        Self {
            owner_name,
            capacity,
            _domain: PhantomData,
            _message: PhantomData,
        }
    }

    pub fn owner_name(&self) -> &'static str {
        self.owner_name
    }

    pub fn capacity(&self) -> u32 {
        self.capacity
    }
}

/// Canonical declaration artifact for long-lived actor-owned domains.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActorDeclaration<Domain, Message> {
    category: BoundaryDeclarationCategory,
    owner_name: &'static str,
    domain_name: &'static str,
    ingress_gate: &'static str,
    ingress: BoundedActorIngress<Domain, Message>,
}

impl<Domain, Message> ActorDeclaration<Domain, Message> {
    pub fn new(
        owner_name: &'static str,
        domain_name: &'static str,
        ingress_gate: &'static str,
        capacity: u32,
    ) -> Self {
        Self {
            category: BoundaryDeclarationCategory::ActorOwned,
            owner_name,
            domain_name,
            ingress_gate,
            ingress: BoundedActorIngress::new(owner_name, capacity),
        }
    }

    pub fn category(&self) -> BoundaryDeclarationCategory {
        self.category
    }

    pub fn owner_name(&self) -> &'static str {
        self.owner_name
    }

    pub fn domain_name(&self) -> &'static str {
        self.domain_name
    }

    pub fn ingress_gate(&self) -> &'static str {
        self.ingress_gate
    }

    pub fn ingress(&self) -> &BoundedActorIngress<Domain, Message> {
        &self.ingress
    }

    pub fn into_ingress(self) -> BoundedActorIngress<Domain, Message> {
        self.ingress
    }

    pub fn register_supervision<HandleId>(
        self,
        handle_id: HandleId,
        shutdown: OwnedShutdownToken,
    ) -> SupervisionRegistration<Domain, Message, HandleId> {
        SupervisionRegistration {
            declaration: self,
            handle: OwnedTaskHandle::new(handle_id, shutdown),
        }
    }
}

/// Canonical declaration artifact for actor-root runtime services that own
/// lifecycle and supervision but do not expose a stable command-ingress type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActorRootDeclaration<Domain> {
    category: BoundaryDeclarationCategory,
    owner_name: &'static str,
    domain_name: &'static str,
    supervision_gate: &'static str,
    _domain: PhantomData<fn() -> Domain>,
}

impl<Domain> ActorRootDeclaration<Domain> {
    pub fn new(
        owner_name: &'static str,
        domain_name: &'static str,
        supervision_gate: &'static str,
    ) -> Self {
        Self {
            category: BoundaryDeclarationCategory::ActorOwned,
            owner_name,
            domain_name,
            supervision_gate,
            _domain: PhantomData,
        }
    }

    pub fn category(&self) -> BoundaryDeclarationCategory {
        self.category
    }

    pub fn owner_name(&self) -> &'static str {
        self.owner_name
    }

    pub fn domain_name(&self) -> &'static str {
        self.domain_name
    }

    pub fn supervision_gate(&self) -> &'static str {
        self.supervision_gate
    }

    pub fn register_supervision<HandleId>(
        self,
        handle_id: HandleId,
        shutdown: OwnedShutdownToken,
    ) -> ActorRootSupervisionRegistration<Domain, HandleId> {
        ActorRootSupervisionRegistration {
            declaration: self,
            handle: OwnedTaskHandle::new(handle_id, shutdown),
        }
    }
}

/// Typed link between an actor-root declaration and its supervised task handle.
#[derive(Debug, Clone)]
pub struct ActorRootSupervisionRegistration<Domain, HandleId> {
    declaration: ActorRootDeclaration<Domain>,
    handle: OwnedTaskHandle<HandleId>,
}

impl<Domain, HandleId> ActorRootSupervisionRegistration<Domain, HandleId> {
    pub fn declaration(&self) -> &ActorRootDeclaration<Domain> {
        &self.declaration
    }

    pub fn handle(&self) -> &OwnedTaskHandle<HandleId> {
        &self.handle
    }

    pub fn into_parts(self) -> (ActorRootDeclaration<Domain>, OwnedTaskHandle<HandleId>) {
        (self.declaration, self.handle)
    }
}

/// Typed link between an actor declaration and its supervised task handle.
#[derive(Debug, Clone)]
pub struct SupervisionRegistration<Domain, Message, HandleId> {
    declaration: ActorDeclaration<Domain, Message>,
    handle: OwnedTaskHandle<HandleId>,
}

impl<Domain, Message, HandleId> SupervisionRegistration<Domain, Message, HandleId> {
    pub fn declaration(&self) -> &ActorDeclaration<Domain, Message> {
        &self.declaration
    }

    pub fn handle(&self) -> &OwnedTaskHandle<HandleId> {
        &self.handle
    }

    pub fn into_parts(self) -> (ActorDeclaration<Domain, Message>, OwnedTaskHandle<HandleId>) {
        (self.declaration, self.handle)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PublicationMetadata<OperationId, InstanceId, Trace = TraceContext> {
    operation_id: OperationId,
    instance_id: InstanceId,
    owner_epoch: OwnerEpoch,
    publication_sequence: PublicationSequence,
    trace_context: Trace,
}

impl<OperationId, InstanceId, Trace> PublicationMetadata<OperationId, InstanceId, Trace> {
    fn new(
        operation_id: OperationId,
        instance_id: InstanceId,
        owner_epoch: OwnerEpoch,
        publication_sequence: PublicationSequence,
        trace_context: Trace,
    ) -> Self {
        Self {
            operation_id,
            instance_id,
            owner_epoch,
            publication_sequence,
            trace_context,
        }
    }
}

/// Non-terminal progress states for exact owner publication.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum OperationProgress<Phase> {
    Submitted,
    Progress { phase: Phase },
}

impl<Phase> OperationProgress<Phase> {
    pub const fn submitted() -> Self {
        Self::Submitted
    }

    pub fn progress(phase: Phase) -> Self {
        Self::Progress { phase }
    }
}

/// Terminal outcome for consumed terminal publication.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum TerminalOutcome<Output, Error> {
    Succeeded { output: Output },
    Failed { error: Error },
    Cancelled,
}

/// Capability-gated progress publication artifact.
#[derive(Debug, PartialEq, Eq)]
pub struct AuthorizedProgressPublication<OperationId, InstanceId, Trace, Phase> {
    capability: LifecyclePublicationCapability,
    metadata: PublicationMetadata<OperationId, InstanceId, Trace>,
    progress: OperationProgress<Phase>,
}

impl<OperationId, InstanceId, Trace, Phase>
    AuthorizedProgressPublication<OperationId, InstanceId, Trace, Phase>
{
    fn authorize(
        capability: &LifecyclePublicationCapability,
        metadata: PublicationMetadata<OperationId, InstanceId, Trace>,
        progress: OperationProgress<Phase>,
    ) -> Self {
        Self {
            capability: capability.clone(),
            metadata,
            progress,
        }
    }

    pub fn capability(&self) -> &CapabilityKey {
        self.capability.as_key()
    }

    pub fn operation_id(&self) -> &OperationId {
        &self.metadata.operation_id
    }

    pub fn instance_id(&self) -> &InstanceId {
        &self.metadata.instance_id
    }

    pub fn owner_epoch(&self) -> OwnerEpoch {
        self.metadata.owner_epoch
    }

    pub fn publication_sequence(&self) -> PublicationSequence {
        self.metadata.publication_sequence
    }

    pub fn trace_context(&self) -> &Trace {
        &self.metadata.trace_context
    }

    pub fn progress(&self) -> &OperationProgress<Phase> {
        &self.progress
    }

    pub fn into_parts(
        self,
    ) -> (
        LifecyclePublicationCapability,
        OperationId,
        InstanceId,
        OwnerEpoch,
        PublicationSequence,
        Trace,
        OperationProgress<Phase>,
    ) {
        (
            self.capability,
            self.metadata.operation_id,
            self.metadata.instance_id,
            self.metadata.owner_epoch,
            self.metadata.publication_sequence,
            self.metadata.trace_context,
            self.progress,
        )
    }
}

/// Consumed single-use terminal publisher.
#[derive(Debug, PartialEq, Eq)]
pub struct TerminalPublisher<OperationId, InstanceId, Trace, Output, Error> {
    capability: LifecyclePublicationCapability,
    metadata: PublicationMetadata<OperationId, InstanceId, Trace>,
    _output: PhantomData<fn() -> Output>,
    _error: PhantomData<fn() -> Error>,
}

impl<OperationId, InstanceId, Trace, Output, Error>
    TerminalPublisher<OperationId, InstanceId, Trace, Output, Error>
{
    fn new(
        capability: &LifecyclePublicationCapability,
        metadata: PublicationMetadata<OperationId, InstanceId, Trace>,
    ) -> Self {
        Self {
            capability: capability.clone(),
            metadata,
            _output: PhantomData,
            _error: PhantomData,
        }
    }

    pub fn succeed(
        self,
        output: Output,
    ) -> AuthorizedTerminalPublication<OperationId, InstanceId, Trace, Output, Error> {
        AuthorizedTerminalPublication {
            capability: self.capability,
            metadata: self.metadata,
            outcome: TerminalOutcome::Succeeded { output },
        }
    }

    pub fn fail(
        self,
        error: Error,
    ) -> AuthorizedTerminalPublication<OperationId, InstanceId, Trace, Output, Error> {
        AuthorizedTerminalPublication {
            capability: self.capability,
            metadata: self.metadata,
            outcome: TerminalOutcome::Failed { error },
        }
    }

    pub fn cancel(
        self,
    ) -> AuthorizedTerminalPublication<OperationId, InstanceId, Trace, Output, Error> {
        AuthorizedTerminalPublication {
            capability: self.capability,
            metadata: self.metadata,
            outcome: TerminalOutcome::Cancelled,
        }
    }
}

/// Capability-gated consumed terminal publication artifact.
#[derive(Debug, PartialEq, Eq)]
pub struct AuthorizedTerminalPublication<OperationId, InstanceId, Trace, Output, Error> {
    capability: LifecyclePublicationCapability,
    metadata: PublicationMetadata<OperationId, InstanceId, Trace>,
    outcome: TerminalOutcome<Output, Error>,
}

impl<OperationId, InstanceId, Trace, Output, Error>
    AuthorizedTerminalPublication<OperationId, InstanceId, Trace, Output, Error>
{
    pub fn capability(&self) -> &CapabilityKey {
        self.capability.as_key()
    }

    pub fn operation_id(&self) -> &OperationId {
        &self.metadata.operation_id
    }

    pub fn instance_id(&self) -> &InstanceId {
        &self.metadata.instance_id
    }

    pub fn owner_epoch(&self) -> OwnerEpoch {
        self.metadata.owner_epoch
    }

    pub fn publication_sequence(&self) -> PublicationSequence {
        self.metadata.publication_sequence
    }

    pub fn trace_context(&self) -> &Trace {
        &self.metadata.trace_context
    }

    pub fn outcome(&self) -> &TerminalOutcome<Output, Error> {
        &self.outcome
    }

    pub fn into_parts(
        self,
    ) -> (
        LifecyclePublicationCapability,
        OperationId,
        InstanceId,
        OwnerEpoch,
        PublicationSequence,
        Trace,
        TerminalOutcome<Output, Error>,
    ) {
        (
            self.capability,
            self.metadata.operation_id,
            self.metadata.instance_id,
            self.metadata.owner_epoch,
            self.metadata.publication_sequence,
            self.metadata.trace_context,
            self.outcome,
        )
    }
}

/// Opaque move-owned workflow context.
#[derive(Debug)]
pub struct OperationContext<OperationId, InstanceId, Trace = TraceContext> {
    operation_id: OperationId,
    instance_id: InstanceId,
    owner_epoch: OwnerEpoch,
    publication_sequence: PublicationSequence,
    timeout_budget: OperationTimeoutBudget,
    shutdown_token: OwnedShutdownToken,
    trace_context: Trace,
}

impl<OperationId, InstanceId, Trace> OperationContext<OperationId, InstanceId, Trace> {
    fn metadata(&self) -> PublicationMetadata<OperationId, InstanceId, Trace>
    where
        OperationId: Clone,
        InstanceId: Clone,
        Trace: Clone,
    {
        PublicationMetadata::new(
            self.operation_id.clone(),
            self.instance_id.clone(),
            self.owner_epoch,
            self.publication_sequence,
            self.trace_context.clone(),
        )
    }

    pub fn operation_id(&self) -> &OperationId {
        &self.operation_id
    }

    pub fn instance_id(&self) -> &InstanceId {
        &self.instance_id
    }

    pub fn owner_epoch(&self) -> OwnerEpoch {
        self.owner_epoch
    }

    pub fn publication_sequence(&self) -> PublicationSequence {
        self.publication_sequence
    }

    pub fn timeout_budget(&self) -> &OperationTimeoutBudget {
        &self.timeout_budget
    }

    pub fn shutdown_token(&self) -> &OwnedShutdownToken {
        &self.shutdown_token
    }

    pub fn trace_context(&self) -> &Trace {
        &self.trace_context
    }

    pub fn publish_update<Phase>(
        &mut self,
        capability: &LifecyclePublicationCapability,
        progress: OperationProgress<Phase>,
    ) -> AuthorizedProgressPublication<OperationId, InstanceId, Trace, Phase>
    where
        OperationId: Clone,
        InstanceId: Clone,
        Trace: Clone,
    {
        let publication =
            AuthorizedProgressPublication::authorize(capability, self.metadata(), progress);
        self.publication_sequence = self.publication_sequence.next();
        publication
    }

    pub fn publish_submitted(
        &mut self,
        capability: &LifecyclePublicationCapability,
    ) -> AuthorizedProgressPublication<OperationId, InstanceId, Trace, ()>
    where
        OperationId: Clone,
        InstanceId: Clone,
        Trace: Clone,
    {
        self.publish_update(capability, OperationProgress::submitted())
    }

    pub fn publish_progress<Phase>(
        &mut self,
        capability: &LifecyclePublicationCapability,
        phase: Phase,
    ) -> AuthorizedProgressPublication<OperationId, InstanceId, Trace, Phase>
    where
        OperationId: Clone,
        InstanceId: Clone,
        Trace: Clone,
    {
        self.publish_update(capability, OperationProgress::progress(phase))
    }

    pub fn begin_terminal<Output, Error>(
        self,
        capability: &LifecyclePublicationCapability,
    ) -> TerminalPublisher<OperationId, InstanceId, Trace, Output, Error> {
        TerminalPublisher::new(
            capability,
            PublicationMetadata::new(
                self.operation_id,
                self.instance_id,
                self.owner_epoch,
                self.publication_sequence,
                self.trace_context,
            ),
        )
    }
}

mod sealed {
    pub trait Sealed {}
}

/// Sealed trait for owner-controlled await/cancellation surfaces.
pub trait OwnerAwait: sealed::Sealed {
    fn owned_shutdown_token(&self) -> &OwnedShutdownToken;
}

/// Sealed trait for owner-controlled publication surfaces.
pub trait OwnerPublication: sealed::Sealed {
    type OperationId;
    type InstanceId;
    type Trace;

    fn operation_id(&self) -> &Self::OperationId;
    fn instance_id(&self) -> &Self::InstanceId;
    fn owner_epoch(&self) -> OwnerEpoch;
    fn publication_sequence(&self) -> PublicationSequence;
    fn trace_context(&self) -> &Self::Trace;
}

impl<OperationId, InstanceId, Trace> sealed::Sealed
    for OperationContext<OperationId, InstanceId, Trace>
{
}
impl<OperationId, InstanceId, Trace> OwnerAwait
    for OperationContext<OperationId, InstanceId, Trace>
{
    fn owned_shutdown_token(&self) -> &OwnedShutdownToken {
        self.shutdown_token()
    }
}

impl<OperationId, InstanceId, Trace, Phase> sealed::Sealed
    for AuthorizedProgressPublication<OperationId, InstanceId, Trace, Phase>
{
}

impl<OperationId, InstanceId, Trace, Phase> OwnerPublication
    for AuthorizedProgressPublication<OperationId, InstanceId, Trace, Phase>
{
    type OperationId = OperationId;
    type InstanceId = InstanceId;
    type Trace = Trace;

    fn operation_id(&self) -> &Self::OperationId {
        self.operation_id()
    }

    fn instance_id(&self) -> &Self::InstanceId {
        self.instance_id()
    }

    fn owner_epoch(&self) -> OwnerEpoch {
        self.owner_epoch()
    }

    fn publication_sequence(&self) -> PublicationSequence {
        self.publication_sequence()
    }

    fn trace_context(&self) -> &Self::Trace {
        self.trace_context()
    }
}

/// Explicit actor-owned ownership surface.
pub mod actor_owned {
    pub use super::{
        ActorDeclaration, BoundedActorIngress, OwnedShutdownToken, OwnedTaskHandle,
        OwnedTaskSpawner, SupervisionRegistration,
    };
}

/// Explicit move-owned ownership surface.
pub mod move_owned {
    pub use super::{
        issue_operation_handle, issue_owner_token, OpaqueOperationHandle, OperationContext,
        OperationProgress, OperationTimeoutBudget, OwnerAwait, OwnerEpoch, OwnerPublication,
        OwnerToken, OwnershipTransfer, PublicationSequence, TerminalOutcome, TerminalPublisher,
        Terminality, TraceContext,
    };
}

/// Explicit capability-gated minting/publication surface.
pub mod capability_gated {
    pub use super::{
        issue_operation_context, ownership_capability_token_request_for,
        ActorIngressMutationCapability, AuthorizedActorIngressMutation,
        AuthorizedProgressPublication, AuthorizedReadinessPublication,
        AuthorizedTerminalPublication, LifecyclePublicationCapability, OperationContextCapability,
        OwnershipCapability, OwnershipTransferCapability, ReadinessPublicationCapability,
    };
}

impl<OperationId, InstanceId, Trace, Output, Error> sealed::Sealed
    for AuthorizedTerminalPublication<OperationId, InstanceId, Trace, Output, Error>
{
}

impl<OperationId, InstanceId, Trace, Output, Error> OwnerPublication
    for AuthorizedTerminalPublication<OperationId, InstanceId, Trace, Output, Error>
{
    type OperationId = OperationId;
    type InstanceId = InstanceId;
    type Trace = Trace;

    fn operation_id(&self) -> &Self::OperationId {
        self.operation_id()
    }

    fn instance_id(&self) -> &Self::InstanceId {
        self.instance_id()
    }

    fn owner_epoch(&self) -> OwnerEpoch {
        self.owner_epoch()
    }

    fn publication_sequence(&self) -> PublicationSequence {
        self.publication_sequence()
    }

    fn trace_context(&self) -> &Self::Trace {
        self.trace_context()
    }
}

/// Capability-gated readiness publication artifact.
///
/// This keeps readiness updates on the same sanctioned authority path as
/// lifecycle updates instead of passing raw capability references through
/// higher-layer helper APIs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorizedReadinessPublication<Payload> {
    capability: ReadinessPublicationCapability,
    payload: Payload,
}

impl<Payload> AuthorizedReadinessPublication<Payload> {
    pub fn authorize(capability: &ReadinessPublicationCapability, payload: Payload) -> Self {
        Self {
            capability: capability.clone(),
            payload,
        }
    }

    pub fn capability(&self) -> &CapabilityKey {
        self.capability.as_key()
    }

    pub fn payload(&self) -> &Payload {
        &self.payload
    }

    pub fn into_parts(self) -> (ReadinessPublicationCapability, Payload) {
        (self.capability, self.payload)
    }
}

/// Capability-gated actor-ingress mutation artifact.
///
/// Higher layers can use this wrapper when they need to hand a mutation command
/// across an actor-owned ingress boundary without exposing raw capability refs
/// as parallel API surface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorizedActorIngressMutation<Mutation> {
    capability: ActorIngressMutationCapability,
    mutation: Mutation,
}

impl<Mutation> AuthorizedActorIngressMutation<Mutation> {
    pub fn authorize(capability: &ActorIngressMutationCapability, mutation: Mutation) -> Self {
        Self {
            capability: capability.clone(),
            mutation,
        }
    }

    pub fn capability(&self) -> &CapabilityKey {
        self.capability.as_key()
    }

    pub fn mutation(&self) -> &Mutation {
        &self.mutation
    }

    pub fn into_parts(self) -> (ActorIngressMutationCapability, Mutation) {
        (self.capability, self.mutation)
    }
}

macro_rules! ownership_capability_wrapper {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub struct $name(CapabilityKey);

        impl $name {
            pub fn new(key: impl Into<CapabilityKey>) -> Self {
                Self(key.into())
            }

            pub fn as_key(&self) -> &CapabilityKey {
                &self.0
            }

            pub fn into_key(self) -> CapabilityKey {
                self.0
            }
        }

        impl OwnershipCapability for $name {
            fn capability_key(&self) -> &CapabilityKey {
                self.as_key()
            }

            fn into_capability_key(self) -> CapabilityKey {
                self.into_key()
            }
        }

        impl From<CapabilityKey> for $name {
            fn from(value: CapabilityKey) -> Self {
                Self(value)
            }
        }

        impl From<$name> for CapabilityKey {
            fn from(value: $name) -> Self {
                value.into_key()
            }
        }
    };
}

ownership_capability_wrapper!(
    /// Capability required to publish authoritative semantic lifecycle updates.
    LifecyclePublicationCapability
);
ownership_capability_wrapper!(
    /// Capability required to mint opaque operation contexts.
    OperationContextCapability
);
ownership_capability_wrapper!(
    /// Capability required to publish authoritative readiness updates.
    ReadinessPublicationCapability
);
ownership_capability_wrapper!(
    /// Capability required to mint typed semantic postcondition proofs.
    PostconditionProofCapability
);
ownership_capability_wrapper!(
    /// Capability required to mutate actor-owned state through sanctioned ingress.
    ActorIngressMutationCapability
);
ownership_capability_wrapper!(
    /// Capability required to delegate or transfer ownership.
    OwnershipTransferCapability
);

/// Issue a typed operation handle through the sanctioned actor-ingress
/// capability path.
pub fn issue_operation_handle<Kind, HandleId, InstanceId>(
    _capability: &ActorIngressMutationCapability,
    handle_id: HandleId,
    instance_id: InstanceId,
) -> OpaqueOperationHandle<Kind, HandleId, InstanceId> {
    OpaqueOperationHandle {
        handle_id,
        instance_id,
        _kind: PhantomData,
    }
}

/// Issue an opaque move-owned operation context through the sanctioned context
/// capability path.
pub fn issue_operation_context<OperationId, InstanceId, Trace>(
    _capability: &OperationContextCapability,
    operation_id: OperationId,
    instance_id: InstanceId,
    owner_epoch: OwnerEpoch,
    publication_sequence: PublicationSequence,
    timeout_budget: OperationTimeoutBudget,
    shutdown_token: OwnedShutdownToken,
    trace_context: Trace,
) -> OperationContext<OperationId, InstanceId, Trace> {
    OperationContext {
        operation_id,
        instance_id,
        owner_epoch,
        publication_sequence,
        timeout_budget,
        shutdown_token,
        trace_context,
    }
}

/// Issue a move-owned authority token through the sanctioned transfer
/// capability path.
pub fn issue_owner_token<Scope, TokenId>(
    _capability: &OwnershipTransferCapability,
    token_id: TokenId,
    scope: Scope,
) -> OwnerToken<Scope, TokenId> {
    OwnerToken { token_id, scope }
}

/// Generic operation handle shape with private fields and typed accessors.
///
/// This is intentionally generic so higher layers can bind their own operation
/// and instance identifiers while sharing the same ownership vocabulary.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OpaqueOperationHandle<Kind, HandleId, InstanceId> {
    handle_id: HandleId,
    instance_id: InstanceId,
    #[serde(skip)]
    _kind: PhantomData<fn() -> Kind>,
}

impl<Kind, HandleId, InstanceId> OpaqueOperationHandle<Kind, HandleId, InstanceId> {
    pub fn handle_id(&self) -> &HandleId {
        &self.handle_id
    }

    pub fn instance_id(&self) -> &InstanceId {
        &self.instance_id
    }

    pub fn into_parts(self) -> (HandleId, InstanceId) {
        (self.handle_id, self.instance_id)
    }
}

/// Typed exclusive-ownership token for a scope.
///
/// Moving this token transfers the right to act. It is intentionally a value
/// type so stale holders are invalidated by ordinary Rust move semantics.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OwnerToken<Scope, TokenId> {
    token_id: TokenId,
    scope: Scope,
}

impl<Scope, TokenId> OwnerToken<Scope, TokenId> {
    pub fn token_id(&self) -> &TokenId {
        &self.token_id
    }

    pub fn scope(&self) -> &Scope {
        &self.scope
    }

    pub fn into_parts(self) -> (TokenId, Scope) {
        (self.token_id, self.scope)
    }

    pub fn handoff<Recipient>(
        self,
        recipient: Recipient,
    ) -> OwnershipTransfer<Scope, TokenId, Recipient> {
        OwnershipTransfer {
            token_id: self.token_id,
            scope: self.scope,
            recipient,
        }
    }
}

/// Consumed ownership-transfer record.
///
/// This is the typed artifact that represents authority moving from one owner
/// to the next. Higher layers can add proofs, audit records, or capability
/// material on top of this primitive.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OwnershipTransfer<Scope, TokenId, Recipient> {
    token_id: TokenId,
    scope: Scope,
    recipient: Recipient,
}

impl<Scope, TokenId, Recipient> OwnershipTransfer<Scope, TokenId, Recipient> {
    pub fn token_id(&self) -> &TokenId {
        &self.token_id
    }

    pub fn scope(&self) -> &Scope {
        &self.scope
    }

    pub fn recipient(&self) -> &Recipient {
        &self.recipient
    }

    pub fn into_parts(self) -> (TokenId, Scope, Recipient) {
        (self.token_id, self.scope, self.recipient)
    }

    pub fn retarget<NextRecipient>(
        self,
        recipient: NextRecipient,
    ) -> OwnershipTransfer<Scope, TokenId, NextRecipient> {
        OwnershipTransfer {
            token_id: self.token_id,
            scope: self.scope,
            recipient,
        }
    }
}

/// Terminality helper trait for lifecycle-like state machines.
pub trait Terminality {
    fn is_terminal(&self) -> bool;
    fn is_submitted(&self) -> bool;
    fn is_in_progress(&self) -> bool;
    fn is_succeeded(&self) -> bool;
    fn is_failed(&self) -> bool;
    fn is_cancelled(&self) -> bool;
}

impl<Phase> Terminality for OperationProgress<Phase> {
    fn is_terminal(&self) -> bool {
        false
    }

    fn is_submitted(&self) -> bool {
        matches!(self, Self::Submitted)
    }

    fn is_in_progress(&self) -> bool {
        matches!(self, Self::Progress { .. })
    }

    fn is_succeeded(&self) -> bool {
        false
    }

    fn is_failed(&self) -> bool {
        false
    }

    fn is_cancelled(&self) -> bool {
        false
    }
}

impl<Output, Error> Terminality for TerminalOutcome<Output, Error> {
    fn is_terminal(&self) -> bool {
        true
    }

    fn is_submitted(&self) -> bool {
        false
    }

    fn is_in_progress(&self) -> bool {
        false
    }

    fn is_succeeded(&self) -> bool {
        matches!(self, Self::Succeeded { .. })
    }

    fn is_failed(&self) -> bool {
        matches!(self, Self::Failed { .. })
    }

    fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled)
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::{
        issue_operation_context, issue_operation_handle, issue_owner_token,
        ActorIngressMutationCapability, AuthorizedActorIngressMutation,
        AuthorizedReadinessPublication, LifecyclePublicationCapability, OperationContextCapability,
        OperationProgress, OperationTimeoutBudget, OwnedShutdownToken, OwnerEpoch,
        OwnershipCapability, OwnershipError, OwnershipErrorDomain, OwnershipTransfer,
        OwnershipTransferCapability, PublicationSequence, ReadinessPublicationCapability,
        TerminalOutcome, Terminality, TraceContext,
    };
    use crate::{effects::CapabilityKey, AuraError, ProtocolErrorCode};

    // OwnershipCategory serialization roundtrip is in
    // tests/contracts/serialization_roundtrip.rs.

    #[test]
    fn opaque_operation_handle_preserves_ids() {
        struct Invite;
        let capability = ActorIngressMutationCapability::new("actor:ingress");

        let handle = issue_operation_handle::<Invite, _, _>(&capability, "invitation_create", 7u64);
        assert_eq!(handle.handle_id(), &"invitation_create");
        assert_eq!(handle.instance_id(), &7u64);
        let (handle_id, instance_id) = handle.into_parts();
        assert_eq!(handle_id, "invitation_create");
        assert_eq!(instance_id, 7u64);
    }

    #[test]
    fn owner_token_handoff_creates_consumed_transfer_record() {
        let capability = OwnershipTransferCapability::new("ownership:transfer");
        let token = issue_owner_token(&capability, "token-1", "session");
        let transfer: OwnershipTransfer<_, _, _> = token.handoff("owner-b");
        assert_eq!(transfer.token_id(), &"token-1");
        assert_eq!(transfer.scope(), &"session");
        assert_eq!(transfer.recipient(), &"owner-b");
    }

    #[test]
    fn progress_and_terminal_outcome_report_terminality() {
        let submitted = OperationProgress::<&'static str>::submitted();
        assert!(submitted.is_submitted());
        assert!(!submitted.is_terminal());

        let progress = OperationProgress::<&'static str>::progress("waiting");
        assert!(progress.is_in_progress());
        assert!(!progress.is_terminal());

        let success = TerminalOutcome::<&'static str, &'static str>::Succeeded { output: "done" };
        assert!(success.is_succeeded());
        assert!(success.is_terminal());

        let failed = TerminalOutcome::<(), &'static str>::Failed { error: "timeout" };
        assert!(failed.is_failed());
        assert!(failed.is_terminal());

        let cancelled = TerminalOutcome::<(), &'static str>::Cancelled;
        assert!(cancelled.is_cancelled());
        assert!(cancelled.is_terminal());
    }

    #[test]
    fn ownership_error_exposes_domain_code_and_aura_mapping() {
        let error = OwnershipError::missing_capability("invitation:send");
        assert_eq!(error.domain(), OwnershipErrorDomain::Capability);
        assert_eq!(error.code(), "missing_capability");

        let aura_error: AuraError = error.into();
        assert!(matches!(aura_error, AuraError::PermissionDenied { .. }));

        let timeout = OwnershipError::timeout("invitation_create");
        assert_eq!(timeout.domain(), OwnershipErrorDomain::Timeout);
        assert_eq!(timeout.code(), "timeout");
        let aura_timeout: AuraError = timeout.into();
        assert!(matches!(aura_timeout, AuraError::Terminal(_)));
    }

    #[test]
    fn ownership_capability_wrappers_round_trip_runtime_keys() {
        let lifecycle = LifecyclePublicationCapability::new("semantic:lifecycle");
        let readiness = ReadinessPublicationCapability::new("semantic:readiness");
        let actor = ActorIngressMutationCapability::new("actor:ingress");
        let transfer = OwnershipTransferCapability::new("ownership:transfer");

        assert_eq!(lifecycle.capability_key().as_str(), "semantic:lifecycle");
        assert_eq!(readiness.capability_key().as_str(), "semantic:readiness");
        assert_eq!(actor.capability_key().as_str(), "actor:ingress");
        assert_eq!(transfer.capability_key().as_str(), "ownership:transfer");

        let raw: CapabilityKey = transfer.into_capability_key();
        assert_eq!(raw.as_str(), "ownership:transfer");
    }

    #[test]
    fn ownership_capabilities_use_existing_capability_token_request_shape() {
        let request = super::ownership_capability_token_request_for(
            "owner-a",
            [
                LifecyclePublicationCapability::new("semantic:lifecycle"),
                LifecyclePublicationCapability::new("semantic:lifecycle:secondary"),
            ],
        );

        assert_eq!(request.subject, "owner-a");
        assert_eq!(
            request.permissions,
            vec![
                "semantic:lifecycle".to_string(),
                "semantic:lifecycle:secondary".to_string(),
            ]
        );
    }

    #[test]
    fn lifecycle_publication_requires_capability_wrapper() {
        let capability = LifecyclePublicationCapability::new("semantic:lifecycle");
        let context_capability = OperationContextCapability::new("operation:context");
        let mut context = issue_operation_context(
            &context_capability,
            "invitation_accept",
            7u64,
            OwnerEpoch::new(3),
            PublicationSequence::new(9),
            OperationTimeoutBudget::deferred_local_policy(),
            OwnedShutdownToken::detached(),
            TraceContext::detached(),
        );

        let progress = context.publish_update(&capability, OperationProgress::<()>::submitted());
        assert_eq!(progress.capability().as_str(), "semantic:lifecycle");
        assert_eq!(progress.operation_id(), &"invitation_accept");
        assert_eq!(progress.instance_id(), &7u64);
        assert_eq!(progress.owner_epoch().value(), 3);
        assert_eq!(progress.publication_sequence().value(), 9);
        assert!(matches!(progress.progress(), OperationProgress::Submitted));

        let terminal = context
            .begin_terminal::<(), &'static str>(&capability)
            .succeed(());
        assert_eq!(terminal.capability().as_str(), "semantic:lifecycle");
        assert!(matches!(
            terminal.outcome(),
            TerminalOutcome::Succeeded { .. }
        ));
    }

    #[test]
    fn readiness_publication_requires_capability_wrapper() {
        let capability = ReadinessPublicationCapability::new("semantic:readiness");
        let publication =
            AuthorizedReadinessPublication::authorize(&capability, "channel_membership_ready");

        assert_eq!(publication.capability().as_str(), "semantic:readiness");
        assert_eq!(publication.payload(), &"channel_membership_ready");
    }

    #[test]
    fn actor_ingress_mutation_requires_capability_wrapper() {
        let capability = ActorIngressMutationCapability::new("actor:ingress");
        let mutation = AuthorizedActorIngressMutation::authorize(&capability, "join_channel");

        assert_eq!(mutation.capability().as_str(), "actor:ingress");
        assert_eq!(mutation.mutation(), &"join_channel");
    }

    #[test]
    fn canonical_semantic_owner_protocol_requires_handoff_and_bounded_waits() {
        let protocol = super::SemanticOwnerProtocol::CANONICAL;
        assert_eq!(
            protocol.handoff_policy(),
            super::SemanticOwnerHandoffPolicy::HandoffBeforeFirstAwait
        );
        assert_eq!(
            protocol.await_policy(),
            super::SemanticOwnerAwaitPolicy::BoundedOnly
        );
        assert_eq!(
            protocol.best_effort_policy(),
            super::SemanticOwnerBestEffortPolicy::TerminalBeforeBestEffort
        );
    }

    #[test]
    fn canonical_best_effort_boundary_protocol_is_post_terminal_only() {
        assert_eq!(
            super::BestEffortBoundaryProtocol::POST_TERMINAL_ONLY.terminal_relation(),
            super::SemanticOwnerBestEffortPolicy::TerminalBeforeBestEffort
        );
    }

    #[tokio::test]
    async fn post_terminal_best_effort_preserves_first_error_and_cannot_own_terminality() {
        let mut best_effort =
            super::PostTerminalBestEffort::<super::OwnershipError>::post_terminal_only();

        assert_eq!(
            best_effort.protocol().terminal_relation(),
            super::SemanticOwnerBestEffortPolicy::TerminalBeforeBestEffort
        );

        let first = best_effort
            .capture(async { Err::<(), _>(super::OwnershipError::timeout("first")) })
            .await;
        let second = best_effort
            .capture(async { Err::<(), _>(super::OwnershipError::timeout("second")) })
            .await;

        assert!(first.is_none());
        assert!(second.is_none());
        assert_eq!(
            best_effort.first_error(),
            Some(&super::OwnershipError::timeout("first"))
        );
        assert_eq!(
            best_effort.finish(),
            Err(super::OwnershipError::timeout("first"))
        );
    }
}
