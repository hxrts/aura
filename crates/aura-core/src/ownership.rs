use crate::{
    effects::{CapabilityKey, CapabilityTokenRequest},
    AuraError, ProtocolErrorCode,
};
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::marker::PhantomData;

/// Repo-wide ownership taxonomy for parity-critical surfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnershipCategory {
    Pure,
    MoveOwned,
    ActorOwned,
    Observed,
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
    pub fn missing_capability(capability: impl Into<String>) -> Self {
        Self::MissingCapability {
            capability: capability.into(),
        }
    }

    pub fn stale_owner(detail: impl Into<String>) -> Self {
        Self::StaleOwner {
            detail: detail.into(),
        }
    }

    pub fn invalid_transfer(detail: impl Into<String>) -> Self {
        Self::InvalidTransfer {
            detail: detail.into(),
        }
    }

    pub fn owner_dropped(detail: impl Into<String>) -> Self {
        Self::OwnerDropped {
            detail: detail.into(),
        }
    }

    pub fn terminal_regression(detail: impl Into<String>) -> Self {
        Self::TerminalRegression {
            detail: detail.into(),
        }
    }

    pub fn timeout(detail: impl Into<String>) -> Self {
        Self::Timeout {
            detail: detail.into(),
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

/// Capability-gated lifecycle publication artifact.
///
/// Higher layers should require this wrapper, or an equivalent owner-scoped
/// capability check, before authoring parity-critical lifecycle facts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorizedLifecyclePublication<Phase, Output, Error> {
    capability: LifecyclePublicationCapability,
    lifecycle: OperationLifecycle<Phase, Output, Error>,
}

impl<Phase, Output, Error> AuthorizedLifecyclePublication<Phase, Output, Error> {
    pub fn authorize(
        capability: &LifecyclePublicationCapability,
        lifecycle: OperationLifecycle<Phase, Output, Error>,
    ) -> Self {
        Self {
            capability: capability.clone(),
            lifecycle,
        }
    }

    pub fn capability(&self) -> &CapabilityKey {
        self.capability.as_key()
    }

    pub fn lifecycle(&self) -> &OperationLifecycle<Phase, Output, Error> {
        &self.lifecycle
    }

    pub fn into_parts(
        self,
    ) -> (
        LifecyclePublicationCapability,
        OperationLifecycle<Phase, Output, Error>,
    ) {
        (self.capability, self.lifecycle)
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
    /// Capability required to publish authoritative readiness updates.
    ReadinessPublicationCapability
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

/// Generic typed lifecycle for long-running parity-critical operations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum OperationLifecycle<Phase, Output, Error> {
    Submitted,
    Progress { phase: Phase },
    Succeeded { output: Output },
    Failed { error: Error },
    Cancelled,
}

impl<Phase, Output, Error> OperationLifecycle<Phase, Output, Error> {
    pub fn submitted() -> Self {
        Self::Submitted
    }

    pub fn progress(phase: Phase) -> Self {
        Self::Progress { phase }
    }

    pub fn succeeded(output: Output) -> Self {
        Self::Succeeded { output }
    }

    pub fn failed(error: Error) -> Self {
        Self::Failed { error }
    }

    pub fn cancelled() -> Self {
        Self::Cancelled
    }

    pub fn phase(&self) -> Option<&Phase> {
        match self {
            Self::Progress { phase } => Some(phase),
            _ => None,
        }
    }

    pub fn output(&self) -> Option<&Output> {
        match self {
            Self::Succeeded { output } => Some(output),
            _ => None,
        }
    }

    pub fn error(&self) -> Option<&Error> {
        match self {
            Self::Failed { error } => Some(error),
            _ => None,
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

impl<Phase, Output, Error> Terminality for OperationLifecycle<Phase, Output, Error> {
    fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Succeeded { .. } | Self::Failed { .. } | Self::Cancelled
        )
    }

    fn is_submitted(&self) -> bool {
        matches!(self, Self::Submitted)
    }

    fn is_in_progress(&self) -> bool {
        matches!(self, Self::Progress { .. })
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
        issue_operation_handle, issue_owner_token, ActorIngressMutationCapability,
        AuthorizedActorIngressMutation, AuthorizedLifecyclePublication,
        AuthorizedReadinessPublication, LifecyclePublicationCapability, OperationLifecycle,
        OwnershipCapability, OwnershipCategory, OwnershipError, OwnershipErrorDomain,
        OwnershipTransfer, OwnershipTransferCapability, ReadinessPublicationCapability,
        Terminality,
    };
    use crate::{effects::CapabilityKey, AuraError, ProtocolErrorCode};

    #[test]
    fn ownership_category_round_trips_through_json() {
        let json = serde_json::to_string(&OwnershipCategory::ActorOwned).expect("serialize");
        assert_eq!(json, "\"actor_owned\"");
        let round_trip: OwnershipCategory = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(round_trip, OwnershipCategory::ActorOwned);
    }

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
    fn operation_lifecycle_reports_terminality() {
        let submitted = OperationLifecycle::<&'static str, (), &'static str>::submitted();
        assert!(submitted.is_submitted());
        assert!(!submitted.is_terminal());

        let progress = OperationLifecycle::<&'static str, (), &'static str>::progress("waiting");
        assert!(progress.is_in_progress());
        assert_eq!(progress.phase(), Some(&"waiting"));
        assert!(!progress.is_terminal());

        let success =
            OperationLifecycle::<&'static str, &'static str, &'static str>::succeeded("done");
        assert!(success.is_succeeded());
        assert!(success.is_terminal());
        assert_eq!(success.output(), Some(&"done"));

        let failed = OperationLifecycle::<&'static str, (), &'static str>::failed("timeout");
        assert!(failed.is_failed());
        assert!(failed.is_terminal());
        assert_eq!(failed.error(), Some(&"timeout"));

        let cancelled = OperationLifecycle::<&'static str, (), &'static str>::cancelled();
        assert!(cancelled.is_cancelled());
        assert!(cancelled.is_terminal());
    }

    #[test]
    fn ownership_error_exposes_domain_code_and_aura_mapping() {
        let error = OwnershipError::missing_capability("invitation:create");
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
        let publication =
            AuthorizedLifecyclePublication::<&'static str, (), &'static str>::authorize(
                &capability,
                OperationLifecycle::submitted(),
            );

        assert_eq!(publication.capability().as_str(), "semantic:lifecycle");
        assert!(publication.lifecycle().is_submitted());
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
