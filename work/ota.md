# Aura OTA Distribution and Scoped Activation Design

This document describes a design for distributing and upgrading Aura using Aura's own storage, journals, trust model, and choreography runtime without assuming a global clock or network-wide consensus.

The key correction is this:

- Aura can have global and eventual release distribution.
- Aura cannot have a network-wide authoritative cutover state unless the whole relevant scope is actually under one agreement mechanism.
- Therefore OTA activation must be scope-bound, not modeled as one global state machine for the whole network.

## Operating Assumptions

The design assumes:

- Aura releases are built deterministically from source.
- The canonical build is defined by a Nix flake.
- Builder authorities may sign deterministic build certificates for the builds they perform.
- TEE attestation is optional at first and may be added later as a hardening layer.
- Release propagation is eventual and multi-directional.
- There is no global clock.
- The whole Aura network is not a single global consensus domain.
- Upgrades use explicit scoped activation and rollback rules rather than imperative restart logic.

## Core Consequence

If a release propagates peer-to-peer through the network, then:

- different devices learn about it at different times
- different authorities trust it under different policies
- some scopes may stage it while others never do
- some scopes may cut over while others remain on older releases

So these are valid global primitives:

- release identity
- provenance
- manifests
- certificates
- artifact distribution
- compatibility declarations

But this is not a valid global primitive:

- "the whole Aura network is now in cutover"

That means hard cutover must be scoped. If a scope has no real agreement mechanism, it can still stage and activate locally, but it cannot claim a coordinated hard-fork boundary for everyone else.

## Design Goals

1. Self-hosted distribution: Aura can distribute source, manifests, artifacts, and certificates through the Aura network.
2. Reproducibility: every release is defined by canonical source and a hermetic Nix build.
3. Authority-based trust: builder authorities and release authorities are explicit Aura identities.
4. Upgrade safety: activation and rollback are typed and scoped, not ad hoc process restart behavior.
5. Local policy control: each device or authority can decide what builder threshold or verification policy it requires.
6. Scope-correct coordination: hard cutover is only used where the scope actually has agreement or a legitimate local fence.
7. Future hardening: TEE-backed build attestation can be added without changing the core release identity model.

## V1 Implementation Boundary

Version 1 should be intentionally narrow and correct.

Supported in v1:

- release declaration, certification, and artifact distribution
- local verification and staging
- device-local activation
- authority-local activation
- managed-quorum cutover where membership is explicit
- deterministic rollback after failed activation
- launcher-mediated stage / activate / confirm / rollback flow

Explicitly out of scope for v1:

- arbitrary relational-context hard cutover without explicit agreement rules
- transparent in-place self-mutation of the running full runtime
- instant revocation propagation guarantees
- mandatory TEE-backed certification
- cross-platform artifact installation parity for every target OS on day one
- fully automated rollout orchestration across scopes that do not own a real fence

This boundary is a correctness measure. It keeps the first implementation aligned with Aura's actual agreement model instead of over-claiming coordination.

## Non-Goals

- Instant global revocation
- Mandatory TEE attestation in the initial system
- In-place mutable upgrades without staged rollback
- Trusting opaque binaries without source or build provenance
- Centralized release hosting as a required dependency
- Pretending the whole Aura network enters one authoritative cutover state

## Separation of Concerns

The OTA system should separate:

1. release identity
2. release provenance
3. build certification
4. artifact distribution
5. compatibility description
6. discovery policy
7. sharing policy
8. activation policy
9. scoped activation state
10. rollback

## Release Identity and Provenance

Aura needs both a stable release line and one exact release identifier.

```rust
pub struct AuraReleaseSeriesId(pub Hash32);
pub struct AuraReleaseId(pub Hash32);
```

`AuraReleaseSeriesId` identifies a long-lived release line, such as the canonical Aura application series.

`AuraReleaseId` identifies one exact release in that series.

Each release is defined by canonical source and canonical build inputs.

```rust
pub struct AuraReleaseProvenance {
    pub source_repo_url: String,
    pub source_bundle_hash: Hash32,
    pub build_recipe_hash: Hash32,
    pub output_hash: Hash32,
    pub nix_flake_hash: Hash32,
    pub nix_flake_lock_hash: Hash32,
}
```

The canonical build recipe must be a Nix flake. The source bundle therefore includes:

- `flake.nix`
- `flake.lock`
- all source inputs needed to produce the target binaries

The release identity should be derived from the series identity plus the provenance:

```rust
impl AuraReleaseId {
    pub fn new(series_id: AuraReleaseSeriesId, provenance: &AuraReleaseProvenance) -> Self {
        AuraReleaseId(hash(
            b"AURA_RELEASE_ID",
            series_id.0.as_bytes(),
            provenance.source_repo_url.as_bytes(),
            provenance.source_bundle_hash.as_bytes(),
            provenance.build_recipe_hash.as_bytes(),
            provenance.output_hash.as_bytes(),
            provenance.nix_flake_hash.as_bytes(),
            provenance.nix_flake_lock_hash.as_bytes(),
        ))
    }
}
```

This makes release identity self-certifying. Any verifier can rebuild the release from the declared source bundle and hermetic Nix build and confirm the output hash.

`source_repo_url` is part of the canonical release identity input. That means provenance does not only bind the source tree snapshot and flake lock; it also binds the declared upstream repository location that the release authority says the source bundle came from.

## Release Manifest

The release manifest is the signed packaging and policy description for a release.

```rust
pub struct AuraReleaseManifest {
    pub series_id: AuraReleaseSeriesId,
    pub release_id: AuraReleaseId,
    pub version: SemanticVersion,
    pub author: AuthorityId,
    pub provenance: AuraReleaseProvenance,
    pub artifacts: Vec<AuraArtifactDescriptor>,
    pub compatibility: AuraCompatibilityManifest,
    pub migrations: Vec<AuraDataMigration>,
    pub activation_profile: AuraActivationProfile,
    pub suggested_activation_time_unix_ms: Option<u64>,
    pub manifest_signature: Signature,
    pub author_public_key: PublicKey,
}
```

The manifest is a signed statement that:

- this is the exact source and build provenance for the release
- these are the target artifacts
- these are the compatibility and migration requirements
- these are the activation and rollback requirements
- these are the scopes in which coordinated cutover is meaningful

`suggested_activation_time_unix_ms` is advisory only. A local policy may ignore it,
or treat it as a "not before" hint against its own local clock, but it is not a
global fence and must never by itself authorize activation.

## Trust and Evidence Model

Release identity and release trust are separate.

- identity answers: "what exact release is this?"
- trust answers: "do I accept this release for staging or activation in this scope?"

Minimum evidence types:

- release-authority manifest signature
- builder-signed deterministic build certificates
- optional local rebuild result
- optional revocation or supersession facts

The default trust posture should be fail-closed:

- a manifest without a trusted release authority signature is not a candidate
- a manifest with insufficient builder evidence is not activatable if the local policy requires builder threshold
- a revoked release is not activatable unless a local operator explicitly overrides that policy
- conflicting evidence must remain visible and auditable rather than collapsed into one opaque "valid" bit

Default policy decisions to encode in the implementation:

- release-authority trust and builder trust are configured separately
- builder thresholds are evaluated against trusted builder identities, not raw certificate count
- local rebuild, when required, dominates remote builder evidence
- revocation blocks activation but does not erase historical evidence
- supersession is advisory unless a scope policy makes it mandatory

This means the evaluator should produce structured outcomes such as:

- insufficient release authority trust
- insufficient builder quorum
- local rebuild required but missing
- revoked by trusted authority
- staged but awaiting advisory activation time
- activation permitted

## Deterministic Build Certification

The initial system does not require TEE attestation. A builder authority may perform the hermetic Nix build and sign the result.

```rust
pub struct AuraDeterministicBuildCertificate {
    pub series_id: AuraReleaseSeriesId,
    pub release_id: AuraReleaseId,
    pub builder: AuthorityId,
    pub provenance: AuraReleaseProvenance,
    pub nix_drv_hash: Hash32,
    pub built_at: TimeStamp,
    pub tee_attestation: Option<TeeAttestation>,
    pub builder_signature: Signature,
}
```

The builder signature states:

- the builder authority ran the deterministic build
- the build used the declared provenance inputs
- the output matched the declared `output_hash`

This certificate is accumulable evidence, not the source of release identity. Multiple builders can independently certify the same release over time.

## Aura-Native Distribution

Aura should distribute its own releases through its storage and replication system.

### Distributed Objects

A release should be represented by content-addressed blobs for:

- source bundle
- `flake.nix`
- `flake.lock`
- release manifest
- compatibility and migration metadata
- platform-specific artifacts
- builder-signed build certificates

### Distribution Authorities

Any Aura authority may choose to pin and serve release blobs. This allows:

- official release authorities
- trusted builders
- block or neighborhood operators
- local peer groups

to all participate in release propagation without being the sole source of truth.

### Discovery and Availability

Release discovery should use journal facts plus ordinary Aura anti-entropy.

The system should not assume:

- a single authoritative update server
- global synchrony
- universal online availability of any one distributor

## Policy Surfaces

Aura OTA must distinguish three separate policy surfaces:

- discovery: what release authorities, builders, distributors, and release series a device is willing to learn about
- sharing: what release metadata, artifacts, certificates, or recommendations a device is willing to pin, forward, or announce to others
- activation: what trust thresholds must be met before a release is staged, activated, or cut over locally

Aura must not infer consent to one from consent to another. Discovering a release does not imply permission to redistribute it. Pinning an artifact does not imply permission to recommend it. Seeing a release does not imply permission to activate it.

Recommended policy types:

```rust
pub struct AuraReleaseDiscoveryPolicy {
    pub allowed_release_sources: Vec<AuthoritySelector>,
    pub allowed_builder_sources: Vec<AuthoritySelector>,
    pub allowed_contexts: Vec<ContextSelector>,
    pub auto_fetch_metadata: bool,
    pub auto_fetch_artifacts: bool,
    pub max_external_leakage: LeakageBudgetPolicy,
    pub max_neighbor_flow: FlowBudgetPolicy,
}

pub struct AuraReleaseSharingPolicy {
    pub pin_policy: PinPolicy,
    pub announce_to: Vec<ContextSelector>,
    pub forward_artifacts_to: Vec<ContextSelector>,
    pub forward_certificates_to: Vec<ContextSelector>,
    pub recommendation_policy: RecommendationPolicy,
}

pub struct AuraReleaseActivationPolicy {
    pub trust_policy: TrustPolicy,
    pub require_local_rebuild: bool,
    pub require_builder_threshold: Option<u8>,
    pub auto_stage: bool,
    pub auto_activate: bool,
    pub respect_suggested_activation_time: bool,
    pub activation_scope: ActivationScope,
}
```

Default posture:

- discovery is limited to explicit release authorities, builders, or distributors in the user's web of trust
- sharing is opt-in and scoped to chosen authorities or contexts
- activation remains local and private unless the user or operator explicitly enables managed rollout
- if `respect_suggested_activation_time` is enabled, the manifest's advisory time is treated only as a local "not before" hint against the local clock

## Activation Scope Model

Activation must be modeled per scope.

```rust
pub enum ActivationScope {
    DeviceLocal {
        device_id: DeviceId,
    },
    AuthorityLocal {
        authority_id: AuthorityId,
    },
    RelationalContext {
        context_id: ContextId,
    },
    ManagedQuorum {
        context_id: ContextId,
        participants: Vec<AuthorityId>,
    },
}
```

This enum intentionally has no `GlobalNetwork` variant.

That omission is a correctness requirement, not a limitation of convenience.

## Journal Facts

The OTA system needs facts for declaration, certification, distribution, policy publication, and scoped execution.

```rust
pub enum AuraReleaseDistributionFact {
    SeriesDeclared {
        series_id: AuraReleaseSeriesId,
        author: AuthorityId,
        name: String,
        declared_at: TimeStamp,
    },
    ReleaseDeclared {
        series_id: AuraReleaseSeriesId,
        release_id: AuraReleaseId,
        manifest_hash: Hash32,
        version: SemanticVersion,
        declared_at: TimeStamp,
    },
    BuildCertified {
        series_id: AuraReleaseSeriesId,
        release_id: AuraReleaseId,
        builder: AuthorityId,
        certificate_hash: Hash32,
        output_hash: Hash32,
        certified_at: TimeStamp,
    },
    ArtifactAvailable {
        release_id: AuraReleaseId,
        blob_hash: Hash32,
        provider: AuthorityId,
        pinned_at: TimeStamp,
    },
    UpgradeOfferPublished {
        release_id: AuraReleaseId,
        provider: AuthorityId,
        policy_hash: Hash32,
        published_at: TimeStamp,
    },
    ReleaseRevoked {
        release_id: AuraReleaseId,
        reason: String,
        revoked_at: TimeStamp,
    },
}
```

```rust
pub enum AuraReleasePolicyFact {
    DiscoveryPolicyPublished {
        scope: PolicyScope,
        policy_hash: Hash32,
        published_at: TimeStamp,
    },
    SharingPolicyPublished {
        scope: PolicyScope,
        policy_hash: Hash32,
        published_at: TimeStamp,
    },
    ActivationPolicyPublished {
        scope: PolicyScope,
        policy_hash: Hash32,
        published_at: TimeStamp,
    },
    RecommendationPublished {
        release_id: AuraReleaseId,
        recommender: AuthorityId,
        scope: PolicyScope,
        published_at: TimeStamp,
    },
}
```

```rust
pub enum AuraUpgradeExecutionFact {
    UpgradeStaged {
        scope: ActivationScope,
        from_release_id: AuraReleaseId,
        to_release_id: AuraReleaseId,
        staged_at: TimeStamp,
    },
    UpgradeResidencyChanged {
        scope: ActivationScope,
        release_id: AuraReleaseId,
        residency: ReleaseResidency,
        entered_at: TimeStamp,
    },
    UpgradeTransitionChanged {
        scope: ActivationScope,
        release_id: AuraReleaseId,
        transition: TransitionState,
        entered_at: TimeStamp,
    },
    UpgradeCutoverApproved {
        scope: ActivationScope,
        from_release_id: AuraReleaseId,
        to_release_id: AuraReleaseId,
        approved_at: TimeStamp,
    },
    UpgradeCutoverCompleted {
        scope: ActivationScope,
        to_release_id: AuraReleaseId,
        completed_at: TimeStamp,
    },
    UpgradeRollbackExecuted {
        scope: ActivationScope,
        from_release_id: AuraReleaseId,
        to_release_id: AuraReleaseId,
        reason: String,
        rolled_back_at: TimeStamp,
    },
    UpgradePartitionObserved {
        scope: ActivationScope,
        release_id: AuraReleaseId,
        reason: String,
        observed_at: TimeStamp,
    },
}
```

These facts allow the system to accumulate evidence and track scoped upgrade execution without mutable centralized state.

## Fact Semantics and Invariants

The OTA facts should be treated as a public API, not loose telemetry.

Evidence facts:

- `SeriesDeclared`
- `ReleaseDeclared`
- `BuildCertified`
- `ArtifactAvailable`
- `UpgradeOfferPublished`
- `ReleaseRevoked`
- policy publication facts

Execution-observation facts:

- `UpgradeStaged`
- `UpgradeResidencyChanged`
- `UpgradeTransitionChanged`
- `UpgradeCutoverApproved`
- `UpgradeCutoverCompleted`
- `UpgradeRollbackExecuted`
- `UpgradePartitionObserved`

Required invariants:

- evidence facts are append-only and never rewritten into mutable state
- execution facts describe what one scope observed or did; they do not imply global network convergence
- `ActivationScope` is always explicit on execution facts
- a scope may observe `Coexisting` or `TargetOnly` without any claim about another scope
- rollback facts must preserve both source and target release ids for auditability
- revocation facts do not retroactively erase distribution or execution history

## Verification Flow

When a device sees a candidate release:

1. Fetch the signed release manifest.
2. Verify the release author signature.
3. Verify `release_id` against the declared provenance.
4. Fetch source bundle, `flake.nix`, `flake.lock`, build certificates, and target artifacts through Aura distribution.
5. Verify builder certificates against the signing builder authority and declared provenance.
6. Apply local activation policy:
   - require only author signature
   - require one trusted builder
   - require `k-of-n` trusted builders
   - require local rebuild
   - require some combination of the above
7. If local rebuild is required, execute the hermetic Nix build and compare the resulting output hash to the manifest.
8. Stage the release only if policy is satisfied.

This separation matters:

- identity comes from provenance
- trust comes from signatures, certificates, and local policy
- availability comes from Aura distribution
- activation is a local or scoped decision, not a network-wide fact

## Compatibility Model

The release manifest should carry compatibility information describing:

- minimum prior release supported for direct upgrade
- migration requirements
- protocol or journal compatibility constraints
- whether mixed coexistence is legal
- whether hard incompatibility requires partition or rejection behavior

Compatibility should be classified explicitly:

```rust
pub enum AuraCompatibilityClass {
    BackwardCompatible,
    MixedCoexistenceAllowed,
    ScopedHardFork,
    IncompatibleWithoutPartition,
}
```

## Scoped Upgrade State Model

Aura upgrades should use typed state, but only within a scope.

Use two enums rather than one overloaded phase enum:

```rust
pub enum ReleaseResidency {
    LegacyOnly,
    Coexisting,
    TargetOnly,
}

pub enum TransitionState {
    Idle,
    AwaitingCutover,
    CuttingOver,
    RollingBack,
}
```

Meaning:

- `ReleaseResidency` describes which release set may currently run in the scope
- `TransitionState` describes whether the scope is stable, awaiting evidence, actively switching, or reverting

State meaning:

- `LegacyOnly`: only the currently active release may admit new runtime activity in this scope
- `Coexisting`: old and new releases may coexist in this scope while compatibility and migration checks run
- `TargetOnly`: the target release is active in this scope and the legacy release is no longer eligible for new activation there
- `Idle`: no cutover or rollback transition is currently executing
- `AwaitingCutover`: the scope is staged and gathering the remaining evidence needed to attempt cutover
- `CuttingOver`: the scope is actively switching the selected runtime
- `RollingBack`: the scope is reverting to the prior release under explicit deterministic rules

These are not global network states.

## What Enables Cutover

Cutover should only proceed if the scope has enough evidence:

- the target release is staged
- local trust policy has been satisfied
- required artifacts are locally available
- if local policy opts in, the local wall clock has reached the manifest's suggested activation time
- migrations have completed or are known-safe
- compatibility checks pass
- the health gate is satisfied
- if threshold approval is required, the scope actually has the authority structure to produce that approval
- if an epoch fence is required, the scope actually owns that fence

## Hard Forks Without a Global Clock

Without a global clock, a hard fork cannot mean "switch at 12:00 UTC".

For Aura, a hard fork must mean one of:

- a device-local decision
- an authority-local decision
- a context-scoped decision
- a managed-quorum decision

If a scope has no real agreement mechanism, it may still stage and locally activate a release, but it cannot claim a coordinated hard cutover for everyone else.

So:

- release distribution can be global and eventual
- hard cutover can only be scoped
- mixed-version interoperability or partition behavior must be explicit

## Partition and Session Behavior

When hard incompatibility exists, the design must say what happens operationally:

- old and new nodes may reject incompatible new sessions
- in-flight incompatible sessions may:
  - drain
  - abort
  - delegate
  - be fenced from new admission
- incompatible peers may partition cleanly rather than pretending to remain interoperable

This is not a failure of the design. It is the correct representation of a distributed system without global agreement.

## Updater / Launcher Control Plane

Aura should not rely on the full runtime replacing itself in place.

Use a smaller updater or launcher layer that is responsible for:

- fetching release manifests and artifacts
- verifying signatures and certificates
- performing optional local rebuild verification
- staging releases
- activating the selected release in one scope
- performing deterministic rollback if necessary

This updater is a control plane for activation. It should be smaller and more stable than the full runtime it launches.

## Artifact and Launcher Contract

The artifact model needs to be fixed before broad implementation.

Each artifact descriptor should answer:

- target platform / architecture
- packaging format
- content hash
- staged layout
- launcher entrypoint
- rollback restoration requirements

The launcher boundary should be explicit:

Inputs to launcher:

- selected release manifest
- resolved artifact descriptor
- staged artifact path
- activation scope
- prior active release reference
- activation directive and health policy

Outputs from launcher:

- stage result
- activate-started acknowledgement
- health confirmation
- activation failure classification
- rollback completion result

The launcher should own:

- process replacement or handoff
- booting the selected staged runtime
- reporting activation success or failure
- restoring the prior release on rollback

The OTA runtime manager should own:

- policy evaluation
- evidence collection
- scoped state transitions
- journaling facts
- deciding when to call stage / activate / rollback

This separation prevents the runtime from becoming a self-modifying process supervisor.

## Rollback

Rollback must be designed in from the start.

Rules:

1. Keep the previously active release staged until the scope reaches `TargetOnly` with `Idle` and the post-cutover health condition is stable.
2. Migrations must be versioned and explicitly classified as reversible or irreversible.
3. If cutover fails before the success condition is recorded, rollback should restore the prior release deterministically in that scope.
4. Rollback events must be journaled.

## Failure Model

The implementation should classify failures explicitly instead of collapsing them into generic activation errors.

Required failure classes:

- manifest signature invalid
- provenance mismatch
- artifact hash mismatch
- insufficient trusted builder evidence
- local rebuild mismatch
- release revoked by trusted policy
- incompatible migration precondition
- missing staged artifact
- advisory activation time not yet reached
- health gate failure after activation
- rollback failed
- partition required due to incompatible peer or in-flight session

The OTA manager, launcher, and journal facts should all preserve this classification so operator behavior and automated policy stay explainable.

## End-to-End Traces

The implementation should be validated against at least these canonical traces:

1. Device-local soft upgrade
   - discover release
   - verify manifest and builder evidence
   - stage artifact
   - enter `Coexisting` + `AwaitingCutover`
   - activate locally
   - confirm health
   - enter `TargetOnly` + `Idle`

2. Managed-quorum hard cutover
   - distribute release and evidence
   - each participant stages locally
   - scope gathers explicit approval
   - incompatible new legacy sessions are fenced
   - cutover enters `CuttingOver`
   - scope converges to `TargetOnly` + `Idle`

3. Failed activation with deterministic rollback
   - release stages successfully
   - activation begins
   - health gate fails
   - rollback enters `RollingBack`
   - prior release is restored
   - rollback fact records reason and restored release

## Telltale Integration

Telltale should be used for:

- typed scoped activation state machines
- manifest-driven admission checks for activation protocols
- typed reconfiguration boundaries where runtime composition changes during cutover
- deterministic replay and conformance artifacts for scoped activation runs

Telltale should not be used to pretend the whole network shares one activation state when it does not.

## Implementation Plan

### Phase 1: Spec Freeze and Contracts

Tasks:

- [x] Freeze the v1 scope boundary in code-facing terms.
- [x] Define the trust-policy evaluator contract and structured denial reasons.
- [x] Freeze artifact descriptor fields and platform identity rules.
- [x] Freeze launcher request/response types and health/rollback contract.
- [x] Freeze OTA fact semantics and invariants as an explicit API.
- [x] Add the three canonical end-to-end traces as test/spec references.

Phase Gate (tests/checks):

- [x] Run focused checks:
  - `just ci-crates-doc-links`
- [x] Verify all phase-focused checks are green.

Success Criteria:

- [x] The v1 OTA surface is explicit enough that later code does not need semantic reinterpretation.
- [x] Trust, artifact, launcher, and fact contracts are documented as implementation constraints rather than informal notes.
- [x] Every later OTA task can point back to one authoritative contract section in this document.

### Phase 2: Release Identity, Trust, and Evidence

Tasks:

- [x] Finalize `AuraReleaseSeriesId`, `AuraReleaseId`, `AuraReleaseProvenance`, `AuraReleaseManifest`, and deterministic build certificate structures.
- [x] Implement canonical hashing and signature verification rules.
- [x] Implement trust-policy evaluation with explicit failure classifications.
- [x] Add revocation and supersession handling to the evaluator.
- [x] Add tests for identity stability, evidence accumulation, and fail-closed trust decisions.

Phase Gate (tests/checks):

- [x] Run focused tests:
  - `env TMPDIR=/Users/hxrts/projects/aura2/.tmp cargo test -p aura-maintenance`
  - `env TMPDIR=/Users/hxrts/projects/aura2/.tmp cargo test -p aura-sync ota_policy`
- [x] Verify all phase-focused tests are green.

Success Criteria:

- [x] Release identity is self-certifying and stable under deterministic re-verification.
- [x] Trust evaluation is deterministic, auditable, and fail-closed.
- [x] Insufficient or conflicting evidence produces structured denial outcomes, not generic failures.

### Phase 3: Distribution and Policy Surfaces

Tasks:

- [x] Implement Aura-native distribution for manifests, artifacts, and certificates.
- [x] Implement discovery policy evaluation independently from sharing policy evaluation.
- [x] Implement activation policy evaluation independently from discovery and sharing.
- [x] Add journal facts for declaration, certification, availability, offers, and policy publication.
- [x] Add tests for scope-aware discovery, selective forwarding, and local-only activation visibility.

Phase Gate (tests/checks):

- [x] Run focused tests:
  - `env TMPDIR=/Users/hxrts/projects/aura2/.tmp cargo test -p aura-sync ota_distribution`
  - `env TMPDIR=/Users/hxrts/projects/aura2/.tmp cargo test -p aura-sync ota_policy`
- [x] Verify all phase-focused tests are green.

Success Criteria:

- [x] Distribution, discovery, sharing, and activation are separate code paths with separate policy decisions.
- [x] OTA facts accumulate evidence without implying global activation state.
- [x] Devices can learn about releases without automatically sharing or activating them.

### Phase 4: Scoped Activation Engine and Launcher Integration

Tasks:

- [x] Implement scoped activation state with `ReleaseResidency` and `TransitionState`.
- [x] Implement stage / activate / confirm-health / rollback launcher handoff.
- [x] Implement compatibility-driven coexistence rules for soft forks.
- [x] Implement hard-fork partition, rejection, and in-flight session handling rules.
- [x] Implement deterministic rollback with explicit failure classification and journaling.
- [x] Add end-to-end coverage for the three canonical traces.

Phase Gate (tests/checks):

- [x] Run focused tests:
  - `env TMPDIR=/Users/hxrts/projects/aura2/.tmp cargo test -p aura-sync ota_transition`
  - `env TMPDIR=/Users/hxrts/projects/aura2/.tmp cargo test -p aura-agent ota_manager`
- [x] Verify all phase-focused tests are green.

Success Criteria:

- [x] Activation is modeled per scope, never as a global network state.
- [x] Launcher integration is explicit and auditable.
- [x] Failed activation restores the prior release deterministically when rollback is permitted.
- [x] Hard incompatibility behavior is explicit rather than accidental.

### Phase 5: Managed Rollout Hardening

Tasks:

- [x] Add managed-quorum cutover approval flows where membership is explicit.
- [x] Add stronger policy controls for staged rollout, activation windows, and rollback preference.
- [x] Add revocation handling in staged and active scope states.
- [x] Add operator/runbook coverage for failed rollout, rollback, and partition cases.
- [x] Evaluate optional TEE-backed build certificates without changing release identity rules.

Phase Gate (tests/checks):

- [x] Run focused checks:
  - `env TMPDIR=/Users/hxrts/projects/aura2/.tmp cargo test -p aura-agent ota_manager`
  - `env TMPDIR=/Users/hxrts/projects/aura2/.tmp just ci-crates-doc-links`
- [x] Verify all phase-focused checks are green.

Success Criteria:

- [x] Managed rollout only exists where the scope owns real membership and approval semantics.
- [x] Revocation and supersession interact predictably with staging and activation.
- [x] TEE support remains additive hardening, not a semantic rewrite of release identity.

## Risks

The main risks are:

- replay of revoked or superseded releases
- publishing recommendations across privacy boundaries unintentionally
- treating local activation as if it were global agreement
- inconsistent cutover state after restart
- irreversible migration mistakes

Mitigations:

- signed manifests
- certificate and provenance verification
- explicit discovery / sharing / activation separation
- scoped upgrade facts
- typed activation residency and transition state
- explicit rollback facts
- compatibility classification and partition rules

## Acceptance Criteria

- [x] Aura can distribute its own source, manifests, artifacts, and certificates through the Aura network.
- [x] Every release is reproducibly defined by source and flake-locked Nix provenance.
- [x] Devices and authorities can apply local trust policy before staging a release.
- [x] Discovery, sharing, and activation remain separate policy surfaces.
- [x] Scoped activation residency and transition state are explicit and machine-checkable.
- [x] Hard cutover is only modeled where the chosen scope actually has agreement or a valid local fence.
- [x] Mixed-version coexistence and partition behavior are explicit.
- [x] Rollback is deterministic and auditable.
- [x] TEE-backed certificates can be added later without changing release identity semantics.
