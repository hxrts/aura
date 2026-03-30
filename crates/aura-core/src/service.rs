//! Shared service-family vocabulary and canonical service/object anchor types.
//!
//! These types define the Phase 1 adaptive-privacy service model at Layer 1 so
//! higher layers can share one vocabulary for service families, policy surfaces,
//! authoritative advertisements, transport/protocol objects, and runtime-derived
//! local selection inputs.

use crate::domain::content::ContentId;
use crate::types::epochs::Epoch;
use crate::types::identifiers::{AuthorityId, ContextId, DeviceId};
use serde::{Deserialize, Serialize};
use std::marker::PhantomData;

/// Operational family for a concrete service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ServiceFamily {
    /// Create, refresh, or upgrade a usable path.
    Establish,
    /// Move opaque objects between providers or peers.
    Move,
    /// Keep opaque objects available across time.
    Hold,
}

/// Object taxonomy for service-model types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ServiceObjectCategory {
    /// Shared advertisements, descriptors, or fact-like inputs.
    AuthoritativeShared,
    /// Execution-time envelopes and custody objects.
    TransportProtocol,
    /// Runtime-local views, scores, and candidate sets.
    RuntimeDerivedLocal,
    /// Receipts, witnesses, or accounting artifacts.
    ProofAccounting,
}

/// Required specification surface for a concrete service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PolicySurface {
    /// Shared discovery advertisements and validity windows.
    Discover,
    /// Provider admission and policy.
    Permit,
    /// Execution/accounting rules.
    Transfer,
    /// Runtime-local provider/path selection.
    Select,
}

/// Declaration metadata for a concrete service boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServiceSurfaceDeclaration<T: ?Sized> {
    /// Stable service boundary name.
    pub service_name: &'static str,
    /// Service families used by the boundary.
    pub families: &'static [ServiceFamily],
    /// Object categories materialized by the boundary.
    pub object_categories: &'static [ServiceObjectCategory],
    /// Ownership point for discovery inputs.
    pub discover_owner: &'static str,
    /// Ownership point for permit evaluation.
    pub permit_owner: &'static str,
    /// Ownership point for transfer execution.
    pub transfer_owner: &'static str,
    /// Ownership point for runtime-local selection.
    pub select_owner: &'static str,
    /// Authoritative shared objects named by the boundary.
    pub authoritative_shared: &'static [&'static str],
    /// Runtime-local state named by the boundary.
    pub runtime_local: &'static [&'static str],
    marker: PhantomData<fn() -> T>,
}

impl<T: ?Sized> ServiceSurfaceDeclaration<T> {
    /// Construct service-boundary declaration metadata.
    pub const fn new(
        service_name: &'static str,
        families: &'static [ServiceFamily],
        object_categories: &'static [ServiceObjectCategory],
        discover_owner: &'static str,
        permit_owner: &'static str,
        transfer_owner: &'static str,
        select_owner: &'static str,
        authoritative_shared: &'static [&'static str],
        runtime_local: &'static [&'static str],
    ) -> Self {
        Self {
            service_name,
            families,
            object_categories,
            discover_owner,
            permit_owner,
            transfer_owner,
            select_owner,
            authoritative_shared,
            runtime_local,
            marker: PhantomData,
        }
    }
}

/// Named descriptor profile over a shared family surface.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ServiceProfile {
    /// Establish bootstrap and refresh.
    DirectBootstrap,
    /// Opaque movement through relay/transit surfaces.
    RelayTransport,
    /// Deferred-delivery hold profile over the shared custody substrate.
    DeferredDeliveryHold,
    /// Cache-replica hold profile over the shared custody substrate.
    CacheReplicaHold,
    /// Extension point for future profiles.
    Named(String),
}

/// Link-layer protocol for a connectivity endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LinkProtocol {
    Quic,
    QuicReflexive,
    Tcp,
    WebSocket,
    WebSocketRelay,
}

/// Shared connectivity endpoint separate from service advertisement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkEndpoint {
    /// Transport/link protocol spoken at this endpoint.
    pub protocol: LinkProtocol,
    /// Direct address when the endpoint is addressable.
    #[serde(default)]
    pub address: Option<String>,
    /// Relay authority when the endpoint is relay-mediated.
    #[serde(default)]
    pub relay_authority: Option<AuthorityId>,
    /// STUN server used for reflexive discovery, when applicable.
    #[serde(default)]
    pub stun_server: Option<String>,
    /// Locally bound interface provenance when known.
    #[serde(default)]
    pub bound_local: Option<String>,
}

impl LinkEndpoint {
    /// Construct a direct endpoint.
    pub fn direct(protocol: LinkProtocol, address: impl Into<String>) -> Self {
        Self {
            protocol,
            address: Some(address.into()),
            relay_authority: None,
            stun_server: None,
            bound_local: None,
        }
    }

    /// Construct a relay-mediated endpoint.
    pub fn relay(relay_authority: AuthorityId) -> Self {
        Self {
            protocol: LinkProtocol::WebSocketRelay,
            address: None,
            relay_authority: Some(relay_authority),
            stun_server: None,
            bound_local: None,
        }
    }
}

/// One hop within a route.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelayHop {
    /// Provider handling this hop.
    pub authority_id: AuthorityId,
    /// Connectivity endpoint for this hop.
    pub link_endpoint: LinkEndpoint,
}

/// Routed connectivity path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Route {
    /// Ordered intermediate hops. Empty means a direct/zero-hop path.
    #[serde(default)]
    pub hops: Vec<RelayHop>,
    /// Final destination endpoint.
    pub destination: LinkEndpoint,
}

impl Route {
    /// Construct a direct route to the destination.
    pub fn direct(destination: LinkEndpoint) -> Self {
        Self {
            hops: Vec::new(),
            destination,
        }
    }

    /// Return whether the route is direct.
    pub fn is_direct(&self) -> bool {
        self.hops.is_empty()
    }

    /// Return all endpoints touched by this route in traversal order.
    pub fn traversal_endpoints(&self) -> Vec<LinkEndpoint> {
        let mut endpoints = self
            .hops
            .iter()
            .map(|hop| hop.link_endpoint.clone())
            .collect::<Vec<_>>();
        endpoints.push(self.destination.clone());
        endpoints
    }
}

/// Explicit establish-family path object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EstablishPath {
    /// Route used to bootstrap or upgrade the usable path.
    pub route: Route,
}

impl EstablishPath {
    /// Construct a direct establish path.
    pub fn direct(destination: LinkEndpoint) -> Self {
        Self {
            route: Route::direct(destination),
        }
    }

    /// Return whether the establish path is direct.
    pub fn is_direct(&self) -> bool {
        self.route.is_direct()
    }
}

/// Explicit move-family path object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MovePath {
    /// Route used to move the opaque object.
    pub route: Route,
}

impl MovePath {
    /// Construct a direct move path.
    pub fn direct(destination: LinkEndpoint) -> Self {
        Self {
            route: Route::direct(destination),
        }
    }

    /// Return whether the move path is direct.
    pub fn is_direct(&self) -> bool {
        self.route.is_direct()
    }
}

/// Descriptor-wide quantitative limits.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceLimits {
    /// Optional max payload size supported by the surface.
    #[serde(default)]
    pub max_payload_bytes: Option<u32>,
    /// Optional max hop count supported by the surface.
    #[serde(default)]
    pub max_hops: Option<u8>,
    /// Optional retention window for hold-like surfaces.
    #[serde(default)]
    pub retention_ms: Option<u64>,
}

/// Non-authoritative local-quality hints published alongside a descriptor.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceQualityHints {
    /// Relative local priority hint.
    #[serde(default)]
    pub priority: Option<u8>,
    /// Relative locality hint.
    #[serde(default)]
    pub locality: Option<u8>,
    /// Relative availability hint.
    #[serde(default)]
    pub availability: Option<u16>,
}

/// Common header carried by all service descriptors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceDescriptorHeader {
    /// Authority advertising the surface.
    pub provider_authority: AuthorityId,
    /// Concrete device when known.
    #[serde(default)]
    pub provider_device: Option<DeviceId>,
    /// Context or scope bound to the advertisement.
    pub service_scope: ContextId,
    /// Start of the validity window.
    pub valid_from: u64,
    /// End of the validity window.
    pub valid_until: u64,
    /// Epoch binding for rotation/invalidation.
    pub epoch: u64,
    /// Operational family described by this descriptor.
    pub family: ServiceFamily,
    /// Shared family limits.
    #[serde(default)]
    pub limits: ServiceLimits,
    /// Optional non-authoritative quality hints.
    #[serde(default)]
    pub quality_hints: Option<ServiceQualityHints>,
}

impl ServiceDescriptorHeader {
    /// Check whether the descriptor is valid at the provided time.
    pub fn is_valid(&self, now_ms: u64) -> bool {
        now_ms >= self.valid_from && now_ms < self.valid_until
    }
}

/// Establish-family descriptor details.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EstablishDescriptor {
    /// Connectivity endpoints usable for establishment.
    #[serde(default)]
    pub link_endpoints: Vec<LinkEndpoint>,
}

impl EstablishDescriptor {
    /// Materialize explicit establish paths from advertised endpoints.
    pub fn advertised_paths(&self) -> Vec<EstablishPath> {
        self.link_endpoints
            .iter()
            .cloned()
            .map(EstablishPath::direct)
            .collect()
    }
}

/// Move-family descriptor details.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MoveDescriptor {
    /// Connectivity endpoints usable for movement.
    #[serde(default)]
    pub link_endpoints: Vec<LinkEndpoint>,
    /// Route-layer material for relayed transport when known.
    #[serde(default)]
    pub route: Option<Route>,
}

impl MoveDescriptor {
    /// Materialize explicit move paths from advertised route material.
    pub fn advertised_paths(&self) -> Vec<MovePath> {
        let mut paths = self
            .link_endpoints
            .iter()
            .cloned()
            .map(MovePath::direct)
            .collect::<Vec<_>>();
        if let Some(route) = &self.route {
            let candidate = MovePath {
                route: route.clone(),
            };
            if !paths.iter().any(|path| path == &candidate) {
                paths.push(candidate);
            }
        }
        paths
    }
}

/// Hold-family descriptor details.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HoldDescriptor {
    /// Connectivity endpoints usable for deposit/retrieval.
    #[serde(default)]
    pub link_endpoints: Vec<LinkEndpoint>,
    /// Selector rotation epoch for retrieval capabilities.
    #[serde(default)]
    pub selector_epoch: Option<u64>,
}

/// Concrete descriptor surface for a service family.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServiceDescriptorKind {
    /// Establish-family surface.
    Establish(EstablishDescriptor),
    /// Move-family surface.
    Move(MoveDescriptor),
    /// Hold-family surface.
    Hold(HoldDescriptor),
}

impl ServiceDescriptorKind {
    /// Family carried by this descriptor kind.
    pub fn family(&self) -> ServiceFamily {
        match self {
            Self::Establish(_) => ServiceFamily::Establish,
            Self::Move(_) => ServiceFamily::Move,
            Self::Hold(_) => ServiceFamily::Hold,
        }
    }

    /// Connectivity endpoints advertised by this surface.
    pub fn link_endpoints(&self) -> &[LinkEndpoint] {
        match self {
            Self::Establish(descriptor) => &descriptor.link_endpoints,
            Self::Move(descriptor) => &descriptor.link_endpoints,
            Self::Hold(descriptor) => &descriptor.link_endpoints,
        }
    }
}

/// Canonical authoritative service advertisement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceDescriptor {
    /// Shared header for the advertisement.
    pub header: ServiceDescriptorHeader,
    /// Profile layered over the family surface.
    pub profile: ServiceProfile,
    /// Family-specific descriptor data.
    pub kind: ServiceDescriptorKind,
}

impl ServiceDescriptor {
    /// Validate that the header family matches the descriptor body family.
    pub fn has_consistent_family(&self) -> bool {
        self.header.family == self.kind.family()
    }
}

/// Opaque movement object used across relay, retrieval-in-flight, and cache seeding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MoveEnvelope {
    /// Move path selected for the envelope, if already bound.
    #[serde(default)]
    pub path: Option<MovePath>,
    /// Opaque application/protocol payload bytes.
    #[serde(with = "serde_bytes")]
    pub payload: Vec<u8>,
}

/// Runtime-local routing profile derived from local conditions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalRoutingProfile {
    /// Number of privacy-motivated intermediate hops to request.
    pub mixing_depth: u8,
    /// Added delay budget for privacy shaping.
    pub delay_ms: u64,
    /// Synthetic cover target rate.
    pub cover_rate_per_second: u32,
    /// Desired path diversity floor.
    pub path_diversity: u8,
}

impl LocalRoutingProfile {
    /// Named routing preset matching pre-privacy behavior.
    pub fn passthrough() -> Self {
        Self {
            mixing_depth: 0,
            delay_ms: 0,
            cover_rate_per_second: 0,
            path_diversity: 1,
        }
    }
}

/// Opaque held object used by the shared `Hold` custody substrate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeldObject {
    /// Content address for the held opaque object.
    pub content_id: ContentId,
    /// Scope this object belongs to.
    pub scope: ContextId,
    /// Retention deadline when known.
    #[serde(default)]
    pub retention_until: Option<u64>,
    /// Encrypted opaque payload bytes.
    #[serde(with = "serde_bytes")]
    pub ciphertext: Vec<u8>,
}

/// Opaque retrieval capability for selector-based hold retrieval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetrievalCapability {
    /// Scope this capability belongs to.
    pub scope: ContextId,
    /// Opaque selector token.
    pub selector: [u8; 32],
    /// Epoch binding for rotation.
    pub epoch: u64,
    /// Capability expiry.
    pub valid_until: u64,
}

impl RetrievalCapability {
    /// Return whether the retrieval capability is valid for the provided epoch and time.
    pub fn is_valid_for(&self, now_ms: u64, epoch: Epoch) -> bool {
        self.epoch == epoch.value() && now_ms < self.valid_until
    }

    /// Validate the retrieval capability under the provided epoch and time.
    pub fn validate_for(&self, now_ms: u64, epoch: Epoch) -> Result<(), RetrievalCapabilityError> {
        if self.epoch != epoch.value() {
            return Err(RetrievalCapabilityError::EpochMismatch {
                expected: epoch,
                actual: Epoch::new(self.epoch),
            });
        }
        if now_ms >= self.valid_until {
            return Err(RetrievalCapabilityError::Expired {
                valid_until: self.valid_until,
                now_ms,
            });
        }
        Ok(())
    }
}

/// Requested and accepted retention metadata for a held object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HoldRetentionMetadata {
    /// Requested retention duration in milliseconds.
    pub requested_ms: u64,
    /// Accepted bounded retention duration in milliseconds.
    pub accepted_ms: u64,
    /// Epoch this deposit is scoped to.
    pub deposit_epoch: Epoch,
    /// Deposit timestamp in milliseconds.
    pub deposited_at_ms: u64,
    /// Expiration timestamp in milliseconds.
    pub expires_at_ms: u64,
}

impl HoldRetentionMetadata {
    /// Return whether the metadata is expired at the provided time.
    pub fn is_expired(&self, now_ms: u64) -> bool {
        now_ms >= self.expires_at_ms
    }
}

/// Metadata coupling a moved object to a later hold deposit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MoveToHoldHandoff {
    /// Scope the handoff belongs to.
    pub scope: ContextId,
    /// Source move path that delivered the object into custody, when known.
    #[serde(default)]
    pub source_path: Option<MovePath>,
    /// Timestamp when the handoff was initiated.
    pub moved_at_ms: u64,
}

/// Typed request to deposit an opaque held object under the shared custody substrate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HoldDepositRequest {
    /// Named hold profile layered over the shared substrate.
    pub profile: ServiceProfile,
    /// Opaque held object to retain.
    pub held_object: HeldObject,
    /// Requested retention duration in milliseconds.
    pub requested_retention_ms: u64,
    /// Epoch fence for this deposit.
    pub deposit_epoch: Epoch,
    /// Optional movement handoff metadata when the object arrived via `Move`.
    #[serde(default)]
    pub handoff: Option<MoveToHoldHandoff>,
}

impl HoldDepositRequest {
    /// Validate the request uses a hold profile rather than a non-hold profile.
    pub fn validate_profile(&self) -> Result<(), HoldRequestError> {
        if self.profile.is_hold_profile() {
            Ok(())
        } else {
            Err(HoldRequestError::InvalidProfile {
                profile: self.profile.clone(),
            })
        }
    }
}

/// Typed selector-based retrieval request over the shared hold substrate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HoldRetrievalRequest {
    /// Named hold profile layered over the shared substrate.
    pub profile: ServiceProfile,
    /// Scope the retrieval is bound to.
    pub scope: ContextId,
    /// Opaque selector token derived from one or more retrieval capabilities.
    pub selector: [u8; 32],
}

impl HoldRetrievalRequest {
    /// Validate the request uses a hold profile and does not encode mailbox identity.
    pub fn validate_profile(&self) -> Result<(), HoldRequestError> {
        if self.profile.is_hold_profile() {
            Ok(())
        } else {
            Err(HoldRequestError::InvalidProfile {
                profile: self.profile.clone(),
            })
        }
    }
}

/// Common anonymous reply-block envelope for bounded accountability callbacks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountabilityReplyBlock {
    /// Scope the reply block is bound to.
    pub scope: ContextId,
    /// Opaque single-use reply token.
    pub token: [u8; 32],
    /// Opaque command binding preventing replay across commands.
    pub command_scope: [u8; 32],
    /// Reply-block expiry timestamp.
    pub valid_until: u64,
}

impl AccountabilityReplyBlock {
    /// Validate the reply block against the current time.
    pub fn validate_at(&self, now_ms: u64) -> Result<(), ReplyBlockError> {
        if now_ms >= self.valid_until {
            return Err(ReplyBlockError::Expired {
                valid_until: self.valid_until,
                now_ms,
            });
        }
        Ok(())
    }
}

macro_rules! define_reply_block {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        pub struct $name {
            /// Anonymous reply-block payload.
            pub inner: AccountabilityReplyBlock,
        }

        impl $name {
            /// Validate the reply block against the current time.
            pub fn validate_at(&self, now_ms: u64) -> Result<(), ReplyBlockError> {
                self.inner.validate_at(now_ms)
            }
        }
    };
}

define_reply_block!(MoveReceiptReplyBlock);
define_reply_block!(HoldDepositReplyBlock);
define_reply_block!(HoldRetrievalReplyBlock);
define_reply_block!(HoldAuditReplyBlock);

/// Shared hold request validation errors.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HoldRequestError {
    #[error("invalid hold profile for shared hold request: {profile:?}")]
    InvalidProfile { profile: ServiceProfile },
}

/// Retrieval capability validation errors.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RetrievalCapabilityError {
    #[error("retrieval capability expired at {valid_until}, now {now_ms}")]
    Expired { valid_until: u64, now_ms: u64 },
    #[error("retrieval capability epoch mismatch: expected {expected}, got {actual}")]
    EpochMismatch { expected: Epoch, actual: Epoch },
}

/// Reply-block validation errors.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReplyBlockError {
    #[error("reply block expired at {valid_until}, now {now_ms}")]
    Expired { valid_until: u64, now_ms: u64 },
}

/// Provenance for a provider candidate without baking in a canonical policy tier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderEvidence {
    Neighborhood,
    DirectFriend,
    IntroducedFof,
    Guardian,
    DescriptorFallback,
}

/// Runtime-local provider candidate derived from plane-owned inputs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderCandidate {
    /// Candidate provider authority.
    pub authority_id: AuthorityId,
    /// Optional device for device-specific surfaces.
    #[serde(default)]
    pub device_id: Option<DeviceId>,
    /// Service family this candidate can satisfy.
    pub family: ServiceFamily,
    /// Provenance inputs used to admit or weight the candidate.
    #[serde(default)]
    pub evidence: Vec<ProviderEvidence>,
    /// Advertised connectivity endpoints.
    #[serde(default)]
    pub link_endpoints: Vec<LinkEndpoint>,
    /// Runtime-local reachability bit.
    pub reachable: bool,
}

/// Runtime-local selection state for a family.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectionState {
    /// Family this state applies to.
    pub family: ServiceFamily,
    /// Providers selected in the current residency window.
    #[serde(default)]
    pub selected_authorities: Vec<AuthorityId>,
    /// Current selection epoch when known.
    #[serde(default)]
    pub epoch: Option<u64>,
    /// Remaining bounded residency budget, when tracked.
    #[serde(default)]
    pub bounded_residency_remaining: Option<u32>,
}

impl ServiceProfile {
    /// Return whether this profile is layered over the shared `Hold` substrate.
    pub fn is_hold_profile(&self) -> bool {
        matches!(
            self,
            Self::DeferredDeliveryHold | Self::CacheReplicaHold
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::identifiers::{AuthorityId, ContextId, DeviceId};

    fn authority(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    fn device(seed: u8) -> DeviceId {
        DeviceId::new_from_entropy([seed; 32])
    }

    fn context(seed: u8) -> ContextId {
        ContextId::new_from_entropy([seed; 32])
    }

    fn endpoint(address: &str) -> LinkEndpoint {
        LinkEndpoint::direct(LinkProtocol::Tcp, address)
    }

    #[test]
    fn route_supports_zero_hop_and_multi_hop_representations() {
        let destination = endpoint("127.0.0.1:7000");
        let direct = Route::direct(destination.clone());
        assert!(direct.is_direct());
        assert_eq!(direct.hops.len(), 0);
        assert_eq!(direct.traversal_endpoints(), vec![destination.clone()]);

        let routed = Route {
            hops: vec![RelayHop {
                authority_id: authority(3),
                link_endpoint: endpoint("10.0.0.5:7443"),
            }],
            destination,
        };
        assert!(!routed.is_direct());
        assert_eq!(routed.hops.len(), 1);
        assert_eq!(routed.traversal_endpoints().len(), 2);
    }

    #[test]
    fn service_descriptor_family_must_match_kind() {
        let descriptor = ServiceDescriptor {
            header: ServiceDescriptorHeader {
                provider_authority: authority(1),
                provider_device: Some(device(2)),
                service_scope: context(9),
                valid_from: 100,
                valid_until: 200,
                epoch: 4,
                family: ServiceFamily::Move,
                limits: ServiceLimits {
                    max_payload_bytes: Some(1024),
                    max_hops: Some(3),
                    retention_ms: None,
                },
                quality_hints: Some(ServiceQualityHints {
                    priority: Some(1),
                    locality: Some(2),
                    availability: Some(3),
                }),
            },
            profile: ServiceProfile::RelayTransport,
            kind: ServiceDescriptorKind::Move(MoveDescriptor {
                link_endpoints: vec![endpoint("10.0.0.1:8443")],
                route: None,
            }),
        };

        assert!(descriptor.has_consistent_family());
        assert!(descriptor.header.is_valid(150));
        assert!(!descriptor.header.is_valid(250));
    }

    #[test]
    fn canonical_anchor_types_roundtrip_without_loss() {
        let route = Route {
            hops: vec![RelayHop {
                authority_id: authority(5),
                link_endpoint: LinkEndpoint::relay(authority(7)),
            }],
            destination: endpoint("10.0.0.4:9443"),
        };

        let envelope = MoveEnvelope {
            path: Some(MovePath {
                route: route.clone(),
            }),
            payload: vec![1, 2, 3, 4],
        };
        let held = HeldObject {
            content_id: ContentId::from_bytes(b"held-object"),
            scope: context(4),
            retention_until: Some(500),
            ciphertext: vec![9, 8, 7],
        };
        let retrieval = RetrievalCapability {
            scope: context(4),
            selector: [7; 32],
            epoch: 8,
            valid_until: 999,
        };
        let candidate = ProviderCandidate {
            authority_id: authority(9),
            device_id: Some(device(1)),
            family: ServiceFamily::Hold,
            evidence: vec![ProviderEvidence::Neighborhood],
            link_endpoints: vec![endpoint("127.0.0.1:5555")],
            reachable: true,
        };
        let state = SelectionState {
            family: ServiceFamily::Hold,
            selected_authorities: vec![authority(9)],
            epoch: Some(3),
            bounded_residency_remaining: Some(2),
        };
        let passthrough = LocalRoutingProfile::passthrough();

        let envelope_json = serde_json::to_vec(&envelope).expect("serialize move envelope");
        let held_json = serde_json::to_vec(&held).expect("serialize held object");
        let retrieval_json = serde_json::to_vec(&retrieval).expect("serialize retrieval cap");
        let candidate_json = serde_json::to_vec(&candidate).expect("serialize provider candidate");
        let state_json = serde_json::to_vec(&state).expect("serialize selection state");

        assert_eq!(
            serde_json::from_slice::<MoveEnvelope>(&envelope_json).expect("deserialize"),
            envelope
        );
        assert_eq!(
            serde_json::from_slice::<HeldObject>(&held_json).expect("deserialize"),
            held
        );
        assert_eq!(
            serde_json::from_slice::<RetrievalCapability>(&retrieval_json).expect("deserialize"),
            retrieval
        );
        assert_eq!(
            serde_json::from_slice::<ProviderCandidate>(&candidate_json).expect("deserialize"),
            candidate
        );
        assert_eq!(
            serde_json::from_slice::<SelectionState>(&state_json).expect("deserialize"),
            state
        );
        assert_eq!(
            envelope.path,
            Some(MovePath {
                route: route.clone(),
            })
        );
        assert_eq!(passthrough.mixing_depth, 0);
        assert_eq!(passthrough.delay_ms, 0);
        assert_eq!(passthrough.cover_rate_per_second, 0);
        assert_eq!(passthrough.path_diversity, 1);
    }

    #[test]
    fn descriptors_materialize_explicit_path_objects() {
        let establish = EstablishDescriptor {
            link_endpoints: vec![endpoint("127.0.0.1:7000")],
        };
        let move_descriptor = MoveDescriptor {
            link_endpoints: vec![endpoint("127.0.0.1:8000")],
            route: Some(Route {
                hops: vec![RelayHop {
                    authority_id: authority(2),
                    link_endpoint: LinkEndpoint::relay(authority(3)),
                }],
                destination: endpoint("127.0.0.1:9000"),
            }),
        };

        let establish_paths = establish.advertised_paths();
        let move_paths = move_descriptor.advertised_paths();

        assert_eq!(establish_paths.len(), 1);
        assert!(establish_paths[0].is_direct());
        assert_eq!(move_paths.len(), 2);
        assert!(move_paths.iter().any(MovePath::is_direct));
        assert!(move_paths.iter().any(|path| !path.is_direct()));
    }

    #[test]
    fn hold_requests_require_hold_profiles() {
        let deposit = HoldDepositRequest {
            profile: ServiceProfile::DeferredDeliveryHold,
            held_object: HeldObject {
                content_id: ContentId::from_bytes(b"held-object"),
                scope: context(2),
                retention_until: None,
                ciphertext: vec![1, 2, 3],
            },
            requested_retention_ms: 500,
            deposit_epoch: Epoch::new(4),
            handoff: Some(MoveToHoldHandoff {
                scope: context(2),
                source_path: None,
                moved_at_ms: 10,
            }),
        };
        let retrieval = HoldRetrievalRequest {
            profile: ServiceProfile::CacheReplicaHold,
            scope: context(2),
            selector: [9; 32],
        };

        assert!(deposit.validate_profile().is_ok());
        assert!(retrieval.validate_profile().is_ok());
        assert!(ServiceProfile::DirectBootstrap
            .is_hold_profile()
            .eq(&false));
        assert_eq!(
            HoldRetrievalRequest {
                profile: ServiceProfile::RelayTransport,
                ..retrieval
            }
            .validate_profile()
            .unwrap_err(),
            HoldRequestError::InvalidProfile {
                profile: ServiceProfile::RelayTransport
            }
        );
    }

    #[test]
    fn retrieval_capability_validation_is_epoch_scoped() {
        let capability = RetrievalCapability {
            scope: context(3),
            selector: [3; 32],
            epoch: 5,
            valid_until: 200,
        };

        assert!(capability.validate_for(150, Epoch::new(5)).is_ok());
        assert_eq!(
            capability.validate_for(250, Epoch::new(5)).unwrap_err(),
            RetrievalCapabilityError::Expired {
                valid_until: 200,
                now_ms: 250
            }
        );
        assert_eq!(
            capability.validate_for(150, Epoch::new(6)).unwrap_err(),
            RetrievalCapabilityError::EpochMismatch {
                expected: Epoch::new(6),
                actual: Epoch::new(5)
            }
        );
    }

    #[test]
    fn reply_blocks_expire_cleanly() {
        let reply_block = HoldRetrievalReplyBlock {
            inner: AccountabilityReplyBlock {
                scope: context(7),
                token: [4; 32],
                command_scope: [5; 32],
                valid_until: 100,
            },
        };

        assert!(reply_block.validate_at(99).is_ok());
        assert_eq!(
            reply_block.validate_at(100).unwrap_err(),
            ReplyBlockError::Expired {
                valid_until: 100,
                now_ms: 100
            }
        );
    }
}
