# Capability Vocabulary Inventory

This Phase 0 artifact inventories the current authorization capability strings,
classifies their status, and records the canonical migration targets for the
clean-cutover capability-vocabulary refactor.

## Scope

This inventory covers authorization capability names used in:

- product Rust call sites
- Biscuit token issuance
- guard snapshots and guard checks
- choreography `.tell` files
- docs and examples that currently teach or exercise capability annotations
- test fixtures that still exercise legacy naming

This inventory does not treat the following as authorization capability names:

- `aura_core::effects::CapabilityKey` runtime-admission keys
- version-handshake feature flags such as `ceremony_supersession`
- explicit negative-test placeholders such as `unknown_capability`
- documentation placeholders such as `capability_name`
- diagnostic labels such as `bundle linking` and `session delegation`
- unrelated "capability" terminology in ownership or crypto docs

No out-of-tree module manifests currently exist in-tree. The module namespace
rules below therefore reserve the future extension path, but no concrete module
capabilities are currently admitted.

## Reserved Namespaces

### First-Party Authorization Namespaces

These namespace roots are reserved for first-party Aura capability families:

| Namespace | Owner crate | Notes |
| --- | --- | --- |
| `amp` | `aura-amp` | AMP message-flow capabilities |
| `auth` | `aura-authentication` | Authentication and guardian-auth capabilities |
| `chat` | `aura-chat` | Chat and channel message capabilities |
| `consensus` | `aura-consensus` | Consensus ceremony capabilities |
| `dkd` | `aura-authentication` | Distributed key derivation choreography capabilities |
| `example` | host-owned docs/examples namespace | Reserved for teaching examples and macro tests |
| `invitation` | `aura-invitation` | Invitation, guardian, channel, and device flows |
| `recovery` | `aura-recovery` | Recovery, guardian setup, membership change |
| `relay` | `aura-rendezvous` | Relay-forward subfamily |
| `rendezvous` | `aura-rendezvous` | Descriptor and rendezvous exchange |
| `sync` | `aura-sync` | Anti-entropy and epoch rotation |

### Generic Host-Owned Capabilities

These names stay reserved by the host and are not owned by a feature crate:

- `read`
- `write`
- `execute`
- `delegate`
- `moderator`
- `flow_charge`

### Reserved Module Namespace

Out-of-tree module-defined capabilities must use:

`module:<module_id>:<capability_path>`

Rules:

- `<module_id>` is the admitted host-reviewed module identity, not an
  arbitrary author-chosen prefix.
- `<capability_path>` uses the same validated lower-case segment grammar as
  first-party names.
- modules may not claim first-party namespace roots such as `invitation`,
  `consensus`, or `sync`
- modules may not claim generic host-owned names such as `read` or `write`
- host runtime code must consume admitted descriptors, not hand-written
  `module:<module_id>:...` strings

## Canonical First-Party Capability Inventory

These are the canonical migration targets for first-party product code.

| Canonical name | Owner crate | Current sources | Notes |
| --- | --- | --- | --- |
| `amp:send` | `aura-amp` | `crates/aura-authorization/src/biscuit_token.rs`, `crates/aura-agent/src/runtime/effects.rs`, `crates/aura-simulator/tests/guarded_amp_anti_entropy.rs` | Canonical AMP send capability |
| `amp:receive` | `aura-amp` | `crates/aura-amp/src/choreography.tell` currently uses `cap:amp_recv` | Canonical AMP receive capability |
| `auth:request` | `aura-authentication` | `crates/aura-authentication/src/guards.rs` | Canonical authentication request capability |
| `auth:submit_proof` | `aura-authentication` | `crates/aura-authentication/src/guards.rs` | Canonical proof-submission capability |
| `auth:verify` | `aura-authentication` | `crates/aura-authentication/src/guards.rs` | Canonical proof-verification capability |
| `auth:create_session` | `aura-authentication` | `crates/aura-authentication/src/guards.rs` | Authentication-owned session creation capability |
| `auth:guardian:request_approval` | `aura-authentication` | `crates/aura-authentication/src/guardian_auth_relational.tell`, `crates/aura-authentication/src/guards.rs` | Canonical guardian-auth request capability |
| `auth:guardian:coordinate` | `aura-authentication` | `crates/aura-authentication/src/guardian_auth_relational.tell` | Coordinator-side guardian-auth capability |
| `auth:guardian:submit_proof` | `aura-authentication` | `crates/aura-authentication/src/guardian_auth_relational.tell` | Guardian proof submission |
| `auth:guardian:verify` | `aura-authentication` | `crates/aura-authentication/src/guardian_auth_relational.tell`, `crates/aura-authentication/src/guards.rs` | Canonical guardian-auth verification capability |
| `chat:channel:create` | `aura-chat` | `crates/aura-chat/src/guards.rs` | Canonical chat channel-create capability |
| `chat:message:send` | `aura-chat` | `crates/aura-chat/src/guards.rs` | Canonical chat send capability |
| `consensus:initiate` | `aura-consensus` | `crates/aura-consensus/src/protocol/guards.rs` | Canonical start-of-ceremony capability |
| `consensus:witness_nonce` | `aura-consensus` | `crates/aura-consensus/src/protocol/guards.rs` | Witness nonce submission |
| `consensus:aggregate_nonces` | `aura-consensus` | `crates/aura-consensus/src/protocol/guards.rs` | Coordinator aggregation capability |
| `consensus:witness_sign` | `aura-consensus` | `crates/aura-consensus/src/protocol/guards.rs` | Witness sign-share submission |
| `consensus:finalize` | `aura-consensus` | `crates/aura-consensus/src/protocol/guards.rs` | Final consensus completion capability |
| `dkd:initiate` | `aura-authentication` | `crates/aura-authentication/src/dkd.tell` | DKD initiation |
| `dkd:commit` | `aura-authentication` | `crates/aura-authentication/src/dkd.tell` | DKD commitment |
| `dkd:reveal` | `aura-authentication` | `crates/aura-authentication/src/dkd.tell` | DKD reveal |
| `dkd:finalize` | `aura-authentication` | `crates/aura-authentication/src/dkd.tell` | DKD finalize |
| `invitation:send` | `aura-invitation` | `crates/aura-invitation/src/guards.rs`, `crates/aura-invitation/src/protocol.rs`, `crates/aura-invitation/src/protocol.invitation_exchange.tell`, token issuance | Canonical invitation send capability |
| `invitation:accept` | `aura-invitation` | `crates/aura-invitation/src/guards.rs`, `crates/aura-invitation/src/protocol.rs`, `crates/aura-invitation/src/protocol.invitation_exchange.tell`, token issuance | Canonical invitation accept capability |
| `invitation:decline` | `aura-invitation` | `crates/aura-invitation/src/guards.rs`, `crates/aura-invitation/src/protocol.rs`, token issuance | Canonical invitation decline capability |
| `invitation:cancel` | `aura-invitation` | `crates/aura-invitation/src/guards.rs`, token issuance | Canonical invitation cancel capability |
| `invitation:guardian` | `aura-invitation` | `crates/aura-invitation/src/guards.rs`, `crates/aura-invitation/src/protocol.rs`, `crates/aura-invitation/src/protocol.guardian_invitation.tell`, token issuance | Guardian invitation send capability |
| `invitation:guardian:accept` | `aura-invitation` | `crates/aura-invitation/src/protocol.rs`, `crates/aura-invitation/src/protocol.guardian_invitation.tell` | Guardian invitation accept capability |
| `invitation:channel` | `aura-invitation` | `crates/aura-invitation/src/guards.rs`, token issuance | Shared-channel invitation capability |
| `invitation:device:enroll` | `aura-invitation` | `crates/aura-invitation/src/protocol.rs`, `crates/aura-invitation/src/protocol.device_enrollment.tell` | Device-enrollment send capability |
| `invitation:device:accept` | `aura-invitation` | `crates/aura-invitation/src/protocol.rs`, `crates/aura-invitation/src/protocol.device_enrollment.tell` | Device-enrollment accept capability |
| `recovery:initiate` | `aura-recovery` | `crates/aura-authentication/src/guards.rs`, `crates/aura-agent/src/handlers/recovery.rs`, `crates/aura-recovery/src/recovery_protocol.tell` | Recovery initiation |
| `recovery:coordinate` | `aura-recovery` | `crates/aura-recovery/src/recovery_protocol.tell` | Recovery coordination capability |
| `recovery:approve` | `aura-recovery` | `crates/aura-authentication/src/guards.rs`, `crates/aura-agent/src/handlers/recovery.rs`, `crates/aura-recovery/src/recovery_protocol.tell` | Guardian approval capability |
| `recovery:finalize` | `aura-recovery` | `crates/aura-agent/src/handlers/recovery.rs`, `crates/aura-recovery/src/recovery_protocol.tell` | Canonical completion/finalization capability |
| `recovery:cancel` | `aura-recovery` | `crates/aura-agent/src/handlers/recovery.rs` | Recovery cancellation capability |
| `recovery:guardian_setup:initiate` | `aura-recovery` | `crates/aura-recovery/src/guardian_setup.tell` | Guardian setup initiation |
| `recovery:guardian_setup:accept_invitation` | `aura-recovery` | `crates/aura-recovery/src/guardian_setup.tell` | Guardian setup invitation acceptance |
| `recovery:guardian_setup:verify_invitation` | `aura-recovery` | `crates/aura-recovery/src/guardian_setup.tell` | Guardian setup verification |
| `recovery:guardian_setup:complete` | `aura-recovery` | `crates/aura-recovery/src/guardian_setup.tell` | Guardian setup completion |
| `recovery:membership_change:initiate` | `aura-recovery` | `crates/aura-recovery/src/guardian_membership.tell` | Membership-change initiation |
| `recovery:membership_change:vote` | `aura-recovery` | `crates/aura-recovery/src/guardian_membership.tell` | Guardian vote capability |
| `recovery:membership_change:verify_proposal` | `aura-recovery` | `crates/aura-recovery/src/guardian_membership.tell` | Proposal verification |
| `recovery:membership_change:complete` | `aura-recovery` | `crates/aura-recovery/src/guardian_membership.tell` | Membership-change completion |
| `relay:forward` | `aura-rendezvous` | `crates/aura-rendezvous/src/protocol.rs`, `crates/aura-rendezvous/src/protocol.relayed_rendezvous.tell`, `docs/113_rendezvous.md` | Relay forwarding subfamily |
| `rendezvous:publish` | `aura-rendezvous` | `crates/aura-rendezvous/src/protocol.rs`, `crates/aura-rendezvous/src/protocol.rendezvous_exchange.tell`, `crates/aura-agent/src/handlers/rendezvous.rs`, `crates/aura-agent/src/runtime/services/rendezvous_manager.rs`, token issuance, `docs/113_rendezvous.md` | Canonical descriptor publish capability |
| `rendezvous:connect` | `aura-rendezvous` | `crates/aura-rendezvous/src/protocol.rs`, `crates/aura-rendezvous/src/protocol.rendezvous_exchange.tell`, `crates/aura-agent/src/handlers/rendezvous.rs`, `crates/aura-agent/src/runtime/services/rendezvous_manager.rs`, `docs/113_rendezvous.md` | Canonical direct connect capability |
| `rendezvous:relay` | `aura-rendezvous` | `crates/aura-rendezvous/src/protocol.rs`, `crates/aura-rendezvous/src/protocol.relayed_rendezvous.tell`, `crates/aura-agent/src/handlers/rendezvous.rs`, `docs/113_rendezvous.md` | Canonical relayed connect capability |
| `sync:request_digest` | `aura-sync` | `crates/aura-authorization/src/biscuit_token.rs`, `crates/aura-agent/src/runtime/effects.rs` | Anti-entropy digest request capability |
| `sync:request_ops` | `aura-sync` | `crates/aura-authorization/src/biscuit_token.rs`, `crates/aura-agent/src/runtime/effects.rs` | Anti-entropy op request capability |
| `sync:push_ops` | `aura-sync` | `crates/aura-authorization/src/biscuit_token.rs`, `crates/aura-agent/src/runtime/effects.rs` | Anti-entropy batch push capability |
| `sync:announce_op` | `aura-sync` | `crates/aura-authorization/src/biscuit_token.rs`, `crates/aura-agent/src/runtime/effects.rs` | Anti-entropy announcement capability |
| `sync:push_op` | `aura-sync` | `crates/aura-authorization/src/biscuit_token.rs`, `crates/aura-agent/src/runtime/effects.rs` | Anti-entropy single-op push capability |
| `sync:epoch:propose_rotation` | `aura-sync` | `crates/aura-sync/src/protocols/epochs.tell` | Epoch rotation proposal |
| `sync:epoch:confirm_readiness` | `aura-sync` | `crates/aura-sync/src/protocols/epochs.tell` | Epoch rotation readiness confirmation |
| `sync:epoch:commit_rotation` | `aura-sync` | `crates/aura-sync/src/protocols/epochs.tell` | Epoch rotation commit |

## Legacy Aliases and Invalid Drift

These strings are present today but are not approved as long-lived capability
surface. They exist only as migration or deletion targets.

| Current string | Classification | Canonical target | Current sources | Disposition |
| --- | --- | --- | --- | --- |
| `amp:send` and `cap:amp_send` coexist | legacy split-brain naming | `amp:send` | `crates/aura-simulator/tests/guarded_amp_anti_entropy.rs`, `crates/aura-amp/src/choreography.tell`, token issuance | Keep `amp:send`; delete `cap:amp_send` |
| `cap:amp_recv` | legacy alias | `amp:receive` | `crates/aura-amp/src/choreography.tell` | Delete alias during Phase 4 |
| `auth:request_guardian` | legacy alias | `auth:guardian:request_approval` | `crates/aura-authentication/src/guards.rs` | Rename in typed family |
| `auth:approve_guardian` | legacy alias | `auth:guardian:verify` | `crates/aura-authentication/src/guards.rs` | Rename in typed family |
| `auth:authenticate` | invalid drift | `auth:verify` or a new explicit `auth:status` if the owner decides status needs its own capability | `crates/aura-agent/src/handlers/auth.rs` | Phase 2/5 owner decision, then delete drift |
| `initiate_consensus` | legacy choreography alias | `consensus:initiate` | `crates/aura-consensus/src/protocol/choreography.tell`, `crates/aura-consensus/src/protocol/GUARD_INTEGRATION.md` | Temporary parse bridge only if needed in Phase 4 |
| `witness_nonce` | legacy choreography alias | `consensus:witness_nonce` | same as above | Temporary parse bridge only if needed in Phase 4 |
| `aggregate_nonces` | legacy choreography alias | `consensus:aggregate_nonces` | same as above | Temporary parse bridge only if needed in Phase 4 |
| `witness_sign` | legacy choreography alias | `consensus:witness_sign` | same as above | Temporary parse bridge only if needed in Phase 4 |
| `finalize_consensus` | legacy choreography alias | `consensus:finalize` | same as above | Temporary parse bridge only if needed in Phase 4 |
| `invitation:device` | legacy umbrella name | split to `invitation:device:enroll` and `invitation:device:accept` | `crates/aura-invitation/src/guards.rs`, token issuance | Remove umbrella capability |
| `message:send` | legacy unowned namespace | `chat:message:send` | token issuance, `crates/aura-agent/src/runtime/effects.rs`, docs/tests in `aura-guards`, `aura-mpst`, `aura-macros` | Migrate examples/tests or move to `example:*`; product code uses `chat:*` |
| `rendezvous:publish_descriptor` | invalid drift | `rendezvous:publish` | `crates/aura-agent/src/handlers/rendezvous.rs` | Delete drift |
| `rendezvous:initiate_channel` | invalid drift | `rendezvous:connect` | `crates/aura-agent/src/handlers/rendezvous.rs` | Delete drift |
| `rendezvous:relay_request` | invalid drift | `rendezvous:relay` | `crates/aura-agent/src/handlers/rendezvous.rs` | Delete drift |
| `recovery:complete` | legacy alias | `recovery:finalize` | `crates/aura-agent/src/handlers/recovery.rs` | Rename to finalized vocabulary |
| `accept_guardian_invitation,verify_setup_invitation` | invalid composite choreography string | split to `recovery:guardian_setup:accept_invitation` and `recovery:guardian_setup:verify_invitation` | `crates/aura-recovery/src/guardian_setup.tell` | Delete comma-joined string syntax on this path |
| `vote_membership_change,verify_membership_proposal` | invalid composite choreography string | split to `recovery:membership_change:vote` and `recovery:membership_change:verify_proposal` | `crates/aura-recovery/src/guardian_membership.tell` | Delete comma-joined string syntax on this path |
| `sync:read` | invalid umbrella name | replace with operation-specific `sync:*` capability per call site | `crates/aura-sync/src/infrastructure/peers.rs` | Delete umbrella capability |
| `sync_journal` | invalid legacy name | replace with operation-specific `sync:*` capability per call site | `crates/aura-sync/src/protocols/anti_entropy.rs`, archived work notes | Delete legacy name |
| `recover:device` | invalid drift in test payload | owner should replace with a canonical `recovery:*` capability or a typed role field | `crates/aura-invitation/src/protocol.rs` test serialization | Do not preserve as compatibility alias |
| `invitation:create` | invalid test-only drift | delete or replace with a real invitation capability | `crates/aura-core/src/ownership.rs` test helper | Do not preserve |
| `recovery_initiate` | legacy test fixture alias | `recovery:initiate` | `crates/aura-testkit/src/fixtures/biscuit.rs` | Delete alias in fixture |
| `recovery_approve` | legacy test fixture alias | `recovery:approve` | `crates/aura-testkit/src/fixtures/biscuit.rs` | Delete alias in fixture |
| `threshold_sign` | invalid / unowned test fixture name | owner must replace with canonical family or remove fixture dependency | `crates/aura-testkit/src/fixtures/biscuit.rs` | Delete or replace |

## Choreography and Example Names That Must Become Namespaced

These current names are intentionally not approved as canonical product
capabilities. They either move into an owned first-party namespace or into the
reserved host-owned `example:*` namespace for teaching material.

| Current string(s) | Classification | Canonical target |
| --- | --- | --- |
| `send_ping`, `send_pong`, `send_request`, `send_response`, `send_message`, `send`, `coordinate`, `coordinate_signing`, `participate_signing` | docs/examples legacy placeholders | `example:*` names in docs, examples, macro tests, and MPST tests |
| `create_session`, `join_session`, `decline_session`, `activate_session`, `broadcast_message`, `check_status`, `report_status`, `end_session` | example-only session choreography names | `example:*` names unless the session protocol becomes a real first-party family |
| `request_session`, `invite_participants`, `respond_session`, `create_session`, `notify_participants`, `reject_session_creation`, `notify_participants_failure` | invalid unnamespaced internal choreography names | future owned `session:*` family if retained; otherwise delete |
| `request_guardian_approval`, `coordinate_guardians`, `submit_guardian_proof`, `verify_guardian` | legacy unnamespaced auth choreography names | `auth:guardian:*` family |
| `initiate_recovery`, `approve_recovery`, `coordinate_recovery`, `finalize_recovery`, `initiate_guardian_setup`, `accept_guardian_invitation`, `verify_setup_invitation`, `complete_guardian_setup`, `initiate_membership_change`, `vote_membership_change`, `verify_membership_proposal`, `complete_membership_change` | legacy unnamespaced recovery choreography names | `recovery:*` subfamilies |
| `propose_epoch_rotation`, `confirm_epoch_readiness`, `commit_epoch_rotation` | legacy unnamespaced sync choreography names | `sync:epoch:*` family |

## Explicit Audit Exclusions

These strings were caught by broad Phase 0 grep passes but are not part of the
authorization capability vocabulary:

| String | Reason for exclusion | Current sources |
| --- | --- | --- |
| `ceremony_supersession` | version-handshake feature flag, not an authorization capability | `crates/aura-protocol/src/handlers/version_handshake.rs`, `crates/aura-core/src/protocol/versions.rs` |
| `fact_journal` | version-handshake feature flag, not an authorization capability | same as above plus docs |
| `unknown_capability` | negative-test placeholder for version capability queries | `crates/aura-protocol/src/handlers/version_handshake.rs`, `crates/aura-core/src/protocol/versions.rs` |
| `capability_name` | documentation placeholder in MPST docs | `crates/aura-mpst/src/lib.rs` |
| `bundle linking` | diagnostic label passed to a reconfiguration capability check, not a capability name | `crates/aura-agent/src/runtime/services/reconfiguration_manager.rs` |
| `session delegation` | diagnostic label passed to a reconfiguration capability check, not a capability name | same as above |

## Quarantine Notes

- Historical scratch notes remain explicitly quarantined as non-authoritative
  archive material.
- This file replaces ad hoc capability-name scratch lists for the Phase 0
  refactor inventory.
- Remaining legacy names are recorded here only as migration/deletion targets.
  They are not approved compatibility surfaces.

## Audit Commands

Phase 0 inventory data was gathered with:

```bash
rg -n --no-heading 'CAP_[A-Z0-9_]+: &str = "[^"]+"' crates -g'*.rs'
rg -n --no-heading 'CapabilityId::from\("[^"]+"|has_capability\("[^"]+"' crates -g'*.rs'
rg -n --no-heading 'guard_capability = "[^"]+"|#\[guard_capability\("[^"]+"\)\]' crates docs examples -g'*.rs' -g'*.md' -g'*.tell'
rg -n --no-heading 'capability\("[^"]+"\)' crates docs examples -g'*.rs' -g'*.md'
```
