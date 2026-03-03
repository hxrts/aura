# Aura Terminology Review

---

# PROPOSAL

## Architectural Changes

### Contact as General Concept

`Contact` is the durable relationship state between two authorities.
`Contact establishment` is the ceremony/protocol action that creates that relationship and its **relational context** (shared namespace + shared secret for communication).

This works between any authority types:
- User ↔ User
- User ↔ Home
- Home ↔ Home
- User ↔ Neighborhood
- Home ↔ Neighborhood

This allows:
- Shared invitation flow across all relationship types
- Deduplicated infrastructure for contact establishment
- Users can communicate with group authorities directly

### Authority Types

| Authority Type | Description | Membership Rules |
|----------------|-------------|------------------|
| **User** | Individual identity | N/A (single authority) |
| **Home** | Group authority for a community | Only accepts User authorities as members. Each User can participate in at most one Home. |
| **Neighborhood** | Group authority linking homes | Only accepts Home authorities as members. |

**Note:** This changes Neighborhood from a relational context (current docs) to a full authority type.

### Membership

Any authority that is part of another authority is a **member** of that authority. This is a generic term that applies across all group authority types.

The User ↔ Home ↔ Neighborhood relationship is **typed** because there is a commitment of resources at each level:

| Relationship | Resource Commitments |
|--------------|---------------------|
| User → Home | Storage allocation, relay capacity, discovery presence |
| Home → Neighborhood | Storage allocation (1MB per neighborhood), relay participation, adjacency routing |

This typing ensures that membership implies concrete infrastructure contributions, not just access permissions.

### Peer

"Peer" is used generically for any authority/device you communicate with—for sync, relay, etc. This means terms like `HomePeer` and `NeighborhoodPeer` create terminology clash and should be avoided.
`Peer` should be reserved for transport/device semantics, not authority-role semantics.

### Home Roles (Simplified)

The current 3-tier role hierarchy (Owner → Admin/Steward → Resident) is collapsed:

| Current | Proposed | Notes |
|---------|----------|-------|
| Owner | Member | No special creator designation. Anyone part of the Home's threshold authority is a Member. |
| Admin / Steward | Member + Moderator | Moderator is an *additional* designation, not a separate tier. |
| Resident | Participant | Users who are granted access but are not in the Home threshold authority. |

**Member**: Anyone who is part of the Home's top-level threshold authority. All Members are equal—the party who created the Home has no special designation.

**Moderator**: An optional designation that can only be held by a `Member`. Grants typical IRC-style mod capabilities (kick, mute, ban, etc.). This is additive, not a tier.

**Participant**: A user who has been granted access to the Home but is not part of the threshold authority.

### Access Levels (Hop-Based)

Access levels are determined by graph distance from the home, measured in hops:

| Hops | Relationship | Default Access |
|------|--------------|----------------|
| 0-hop | User is a member of this home | **Full** |
| 1-hop | User's home is in the same neighborhood | **Partial** |
| 2-hop+ | User is beyond the neighborhood (or disconnected) | **Limited** |

**Default overrides**: Members of a home can modify access for specific users:
- Grant a 2-hop+ user **Partial** access (upgrade from Limited)
- Downgrade a 1-hop user from **Partial** to **Limited**

**Capability configuration**: Members of a home can modify what **Full**, **Partial**, and **Limited** access levels actually entail in terms of capabilities.

This replaces the spatial metaphor (Interior/Frontage/Street) with explicit graph-distance terminology.

Deterministic access computation:
- Build the authority graph from committed facts only.
- Source = user's current Home authority (or disconnected if none).
- Compute shortest path distance from source Home to target Home.
- Map distance to access: same home => Full, 1 hop => Partial, 2+ hops/disconnected => Limited.
- Apply explicit per-user override facts after default mapping.
- If multiple paths exist, use minimum hop distance only.

### Neighborhood Authority Invariants (Simple)

- `InvariantNeighborhoodMemberType`: Only `Home` authorities can be members of a `Neighborhood`.
- `InvariantNeighborhoodMembershipUnique`: Membership edge `(neighborhood_id, home_id)` is unique.
- `InvariantNeighborhoodMembershipFactBacked`: Membership changes are fact-backed.
- `InvariantNeighborhoodViewDeterministic`: Same fact set yields same neighborhood membership/hop view.

### Neighborhood Operation Categories (Simple)

- Category A: read-only queries (`list_members`, `compute_access_level`, `view_routes`).
- Category B: monotone adds (`propose_join`, `accept_join`).
- Category C: non-monotone mutations (`remove_member`, `policy_change`, `override_change`).

### Relationship Model

```
┌─────────────────────────────────────────────────────────────────┐
│                        AUTHORITIES                               │
│                                                                  │
│        User              Home              Neighborhood          │
│          │                 │                    │                │
│          └─────────────────┴────────────────────┘                │
│                            │                                     │
│                      establish contact                           │
│                            │                                     │
│                            ▼                                     │
│                   Relational Context                             │
│                (shared namespace + secret)                       │
└─────────────────────────────────────────────────────────────────┘
```

## Terminology Changes

| Current | Proposed | Rationale |
|---------|----------|-----------|
| Public Good Space | Shared Storage | Less verbose, clearer meaning |
| Owner | Member | No special creator designation |
| Admin / Steward | Member + Moderator | Moderator is an add-on designation, not a tier |
| Resident | Participant | Users not in threshold authority |
| Interior | Full | Describes capability level, not location |
| Frontage | Partial | Describes capability level, not architecture |
| Street | Limited | Describes capability level, not architecture |
| HomePeer | 0-hop (user) | Graph position, avoids "peer" clash |
| NeighborhoodPeer | 2-hop (user) | Graph position, avoids "peer" clash |
| Adjacency | 1-hop (home) | Graph position, consistent with hop-based model |
| TraversalDepth | AccessLevel | Clearer, not tied to spatial metaphor |
| Donation | Allocation | Clearer, less loaded term |
| Pinned Content | Pinned | Not a new noun—pinning is an action on Facts |

### Pinned

**Pinned** is an attribute that can be applied to any Fact. Replaces "Pinned Content"—we don't need a separate noun since pinning is just something you do to Facts.

### Witness (Keep)

**Witness** is intentionally used instead of "Validator" to distinguish the consensus-level attestation role from the FROST-level signing role:

- At the **consensus layer**, witnesses attest to operations against a prestate ("I witnessed this operation happen against this state")
- At the **FROST layer**, signers participate in threshold signature generation

Using "Witness" at the consensus level and "Signer" at the cryptographic level avoids conflating these distinct responsibilities.

**Action:** Add a short remark to `docs/106_consensus.md` explaining this rationale.

---

# WORK PLAN

This section contains the implementation plan for the terminology and architectural changes described above.

---

## Phase 1: Discovery and Validation

### 1.1 Create Discovery Script

- [x] **Create `scripts/find-terminology.sh`**

  Bash script that searches `docs/` and `crates/` for all terms to be renamed:

  | Old Term | New Term | Search Pattern |
  |----------|----------|----------------|
  | `PublicGoodSpace` | `SharedStorage` | `PublicGoodSpace\|public_good_space\|PUBLIC_GOOD_SPACE` |
  | `Owner` (role) | `Member` | Context-aware: `ResidentRole::Owner\|role.*owner` |
  | `Admin` / `Steward` | `Moderator` | `Steward\|steward\|ResidentRole::Admin` |
  | `Resident` (role) | `Participant` | Context-aware: `ResidentRole::Resident` |
  | `Interior` | `Full` | `TraversalDepth::Interior\|interior` |
  | `Frontage` | `Partial` | `TraversalDepth::Frontage\|frontage` |
  | `Street` | `Limited` | `TraversalDepth::Street\|street` |
  | `HomePeer` | (remove) | `HomePeer\|home_peer` |
  | `NeighborhoodPeer` | (remove) | `NeighborhoodPeer\|neighborhood_peer` |
  | `Adjacency` | `1-hop` | Context-aware |
  | `TraversalDepth` | `AccessLevel` | `TraversalDepth\|traversal_depth` |
  | `Donation` | `Allocation` | `Donation\|donation` (storage context) |
  | `PinnedContent` | `Pinned` | `PinnedContent\|pinned_content` |

  Script requirements:
  - Output format: `file:line:match` with context
  - Separate report sections for each term
  - Summary counts per term
  - JSON output option for programmatic use
  - Two-pass workflow:
    - pass 1 = inventory/report only (no edits)
    - pass 2 = curated replacements only (explicit allowlists/exclusions)

  **Success criteria:**
  - Script runs without errors
  - Produces machine-readable output
  - Covers both docs/ and crates/ directories

### 1.2 Validate Script Coverage

- [x] **Run script and review output**
  - Verify all instances are found
  - Check for false negatives by manual spot-checking
  - Cross-reference with `rg` searches

  **Success criteria:**
  - Manual spot-check finds no missed instances
  - Script output matches `rg` baseline for sample terms

### 1.3 Validate Zero False Positives

- [x] **Review each match category**
  - Flag any legitimate uses that should NOT be renamed
  - Create exclusion patterns for false positives
  - Document any special cases

  **Success criteria:**
  - Each match reviewed and categorized
  - False positives documented with rationale
  - Exclusion patterns added to script

---

## Phase 2: Documentation Updates

### 2.1 Rename Terms in Documentation

- [x] **Update `docs/114_social_architecture.md`**
  - Public Good Space → Shared Storage
  - Owner/Admin/Steward/Resident → Member/Moderator/Participant
  - Interior/Frontage/Street → Full/Partial/Limited
  - TraversalDepth → AccessLevel
  - Donation → Allocation
  - HomePeer/NeighborhoodPeer → hop-based terminology

- [x] **Update `docs/112_relational_contexts.md`**
  - Relational context terminology for Neighborhood as authority type

- [x] **Update `docs/102_authority_and_identity.md`**
  - Authority type definitions
  - Membership model changes

- [x] **Update `docs/002_theoretical_model.md`**
  - Add/refresh terminology reference section
  - Define `Member`/`Participant` and `Full`/`Partial`/`Limited`

- [x] **Update `docs/003_information_flow_contract.md`**
  - Add/refresh terminology reference section for flow/privacy-relevant terms

- [x] **Update `docs/004_distributed_systems_contract.md`**
  - Add/refresh terminology reference section for distributed-systems terms

- [x] **Update `docs/005_system_invariants.md`**
  - Ensure canonical names map cleanly to renamed terminology where applicable

- [x] **Update `docs/106_consensus.md`**
  - Add Witness vs Validator rationale note

- [x] **Update `docs/116_cli_tui.md`**
  - Screen/command terminology changes

- [x] **Update remaining docs as identified by script**

### 2.2 Update Project Meta Documentation

- [x] **Update `CLAUDE.md`**
  - Update terminology in Architecture Essentials section
  - Update "Where does my code go?" decision tree if affected
  - Update any references to old role names or access levels

- [x] **Update `.claude/skills/aura-quick-ref/SKILL.md`**
  - Update terminology quick reference
  - Update any affected code snippets or examples

- [x] **Update `docs/999_project_structure.md`**
  - Update any terminology references in crate descriptions

- [x] **Update `docs/000_project_overview.md`**
  - Update high-level terminology if affected

**Success criteria:**
- Discovery script reports zero matches for old terms in docs/
- Documentation is internally consistent
- CLAUDE.md and skills are consistent with new terminology
- Terminology references are present in docs 002/003/004/005 (no new standalone glossary doc)

### 2.3 Phase 2 Checkpoint

- [x] **Stage and commit Phase 2 changes**
  ```bash
  git add docs/
  git commit -m "docs: update terminology across all documentation

  - Rename Public Good Space → Shared Storage
  - Rename Owner/Admin/Steward/Resident → Member/Moderator/Participant
  - Rename Interior/Frontage/Street → Full/Partial/Limited
  - Rename TraversalDepth → AccessLevel
  - Rename Donation → Allocation
  - Update HomePeer/NeighborhoodPeer to hop-based terminology
  - Add terminology reference sections to docs 002/003/004/005
  - Add Witness vs Validator rationale to consensus docs"
  ```

---

## Phase 3: Code Terminology Renames

### 3.1 Core Type Renames

- [x] **`aura-core` renames**
  - `TraversalDepth` enum → `AccessLevel`
  - `TraversalDepth::Interior` → `AccessLevel::Full`
  - `TraversalDepth::Frontage` → `AccessLevel::Partial`
  - `TraversalDepth::Street` → `AccessLevel::Limited`

- [x] **`aura-social` renames**
  - `ResidentRole::Owner` → `HomeRole::Member` (or remove special status)
  - `ResidentRole::Admin` → Remove (use `Moderator` designation on `Member`)
  - `ResidentRole::Resident` → `HomeRole::Participant`
  - `HomePeer` → Remove or rename to graph-distance terminology
  - `NeighborhoodPeer` → Remove or rename
  - `Adjacency` → Consider 1-hop terminology
  - `Donation` → `Allocation`
  - `PublicGoodSpace` → `SharedStorage`

- [x] **Update all dependent crates**
  - `aura-app`
  - `aura-terminal`
  - `aura-agent`
  - Test crates

**Success criteria:**
- `just check` passes
- `just clippy` passes
- Discovery script reports zero matches for old terms in crates/

### 3.2 Implement Architectural Changes

- [x] **Neighborhood as Authority Type**
  - Update `AuthorityType` enum if needed
  - Ensure Neighborhood has authority-level operations
  - Add simple invariant enforcement:
    - `InvariantNeighborhoodMemberType`
    - `InvariantNeighborhoodMembershipUnique`
    - `InvariantNeighborhoodMembershipFactBacked`
    - `InvariantNeighborhoodViewDeterministic`
  - Add simple operation categories:
    - Category A: `list_members`, `compute_access_level`, `view_routes`
    - Category B: `propose_join`, `accept_join`
    - Category C: `remove_member`, `policy_change`, `override_change`

- [x] **Contact as General Concept**
  - Keep `Contact` as durable relationship state
  - Treat `contact establishment` as the ceremony/protocol action
  - Verify invitation flow supports all authority type pairs
  - Deduplicate any type-specific invitation code

- [x] **Home Role Simplification**
  - Collapse Owner/Admin/Steward into Member
  - Add Moderator as designation (not role tier), with rule: only a `Member` can be a `Moderator`
  - Participants are non-threshold users

- [x] **Hop-Based Access Levels**
  - Implement 0-hop/1-hop/2-hop+ relationship detection
  - Map to Full/Partial/Limited access levels
  - Support access level overrides per-user
  - Use deterministic computation: committed-fact graph + shortest path + minimum hop distance

**Success criteria:**
- All architectural changes compile
- Existing tests pass or are updated

### 3.3 Terminology + Semantics Freeze Gate

- [x] **Freeze terminology and semantics before bulk code rename**
  - Confirm docs 002/003/004/005 reflect final terms and meanings
  - Confirm role/access/contact definitions are stable
  - Only after this gate, run bulk renames in code and UI

### 3.4 Phase 3 Checkpoint

- [x] **Run full CI checks**
  ```bash
  just fmt-check
  just clippy
  just test
  ```
- [x] **Verify discovery script reports zero matches**
  ```bash
  ./scripts/find-terminology.sh --check
  ```
- [x] **Stage and commit Phase 3 changes**
  ```bash
  git add crates/
  git commit -m "refactor: rename terminology in codebase

  - TraversalDepth → AccessLevel (Full/Partial/Limited)
  - ResidentRole → HomeRole (Member/Moderator/Participant)
  - PublicGoodSpace → SharedStorage
  - Donation → Allocation
  - Remove HomePeer/NeighborhoodPeer in favor of hop-based model
  - Implement Neighborhood as authority type
  - Add hop-based access level detection"
  ```

---

## Phase 4: Functionality Implementation

### 4.1 Missing Functionality

- [x] **Moderator designation system**
  - Add `ModeratorDesignation` to home membership
  - Implement assign/unassign operations (Category B)
  - Enforce rule: only `Member` can hold `ModeratorDesignation`
  - Define moderator capabilities (kick, mute, ban)

- [x] **Access level override system**
  - Store per-user access overrides
  - 2-hop → Partial upgrade
  - 1-hop → Limited downgrade

- [x] **Access level capability configuration**
  - Per-home configuration of what Full/Partial/Limited can do
  - Default capability sets

**Success criteria:**
- New functionality compiles
- Basic operations work in isolation

### 4.2 Phase 4 Checkpoint

- [x] **Run full CI checks**
  ```bash
  just fmt-check
  just clippy
  just test
  ```
- [x] **Stage and commit Phase 4 changes**
  ```bash
  git add crates/
  git commit -m "feat: implement moderator designation and access level overrides

  - Add ModeratorDesignation to home membership
  - Implement moderator assign/unassign (Category B)
  - Enforce Moderator ⊆ Member
  - Add per-user access level overrides
  - Add per-home capability configuration"
  ```

---

## Phase 5: Property Tests

Reference: `docs/804_testing_guide.md`

### 5.1 AccessLevel Property Tests

- [x] **AccessLevel enum properties**
  ```rust
  proptest! {
      #[test]
      fn access_level_ordering(level in access_level_strategy()) {
          // Full > Partial > Limited ordering
      }

      #[test]
      fn hop_distance_maps_to_access_level(hops in 0..10u8) {
          // 0-hop → Full, 1-hop → Partial, 2+ → Limited
      }
  }
  ```

- [x] **Access override properties**
  ```rust
  proptest! {
      #[test]
      fn override_only_downgrades_or_upgrades_one_level(
          base in access_level_strategy(),
          override_target in access_level_strategy()
      ) {
          // Overrides are bounded
      }
  }
  ```

### 5.2 Home Role Property Tests

- [x] **Member/Participant distinction**
  ```rust
  proptest! {
      #[test]
      fn members_are_threshold_participants(home in home_strategy()) {
          // All Members in threshold authority
          // All Participants NOT in threshold authority
      }
  }
  ```

- [x] **Moderator designation**
  ```rust
  proptest! {
      #[test]
      fn only_members_can_be_moderators(home in home_strategy()) {
          // Moderator ⊆ Members
      }
  }
  ```

### 5.3 Shared Storage Property Tests

- [x] **Allocation constraints**
  ```rust
  proptest! {
      #[test]
      fn allocations_sum_to_total(home in home_strategy()) {
          // Sum of allocations ≤ total storage
      }
  }
  ```

**Success criteria:**
- All property tests pass
- 10,000+ test cases per property

### 5.4 Phase 5 Checkpoint

- [x] **Run property tests with extended iterations**
  ```bash
  PROPTEST_CASES=10000 cargo test --workspace -- --ignored property
  ```
- [x] **Run full test suite**
  ```bash
  just test
  ```
- [ ] **Stage and commit Phase 5 changes**
  ```bash
  git add crates/
  git commit -m "test: add property tests for access levels and home roles

  - AccessLevel ordering and hop-distance mapping
  - Access override boundary properties
  - Member/Participant threshold participation invariant
  - Moderator designation subset property
  - Allocation sum constraints"
  ```

---

## Phase 6: E2E/Integration Tests

Reference: `docs/804_testing_guide.md`, `docs/805_simulation_guide.md`

### 6.1 Access Level E2E Tests

- [ ] **Hop distance calculation**
  ```rust
  #[aura_test]
  async fn test_hop_distance_through_neighborhood() {
      // Create homes in neighborhood
      // Verify hop distances are correct
  }
  ```

- [ ] **Access level enforcement**
  ```rust
  #[aura_test]
  async fn test_limited_cannot_access_full_content() {
      // 2-hop user attempts Full-level operation
      // Verify denial
  }
  ```

### 6.2 Home Role E2E Tests

- [ ] **Member creation (no Owner special status)**
  ```rust
  #[aura_test]
  async fn test_home_creator_is_regular_member() {
      // Create home
      // Verify creator has Member role, not Owner
  }
  ```

- [ ] **Moderator operations**
  ```rust
  #[aura_test]
  async fn test_moderator_can_kick() {
      // Assign moderator
      // Moderator kicks participant
      // Verify kick succeeds
  }
  ```

### 6.3 Simulation Tests

- [ ] **TOML scenario: Home with role transitions**
  ```toml
  [[phases]]
  name = "role_transitions"
  actions = [
      { type = "create_home", creator = "alice" },
      { type = "add_member", home = "home1", user = "bob" },
      { type = "assign_moderator", home = "home1", user = "bob" },
      { type = "moderator_kick", home = "home1", moderator = "bob", target = "participant1" },
  ]
  ```

**Success criteria:**
- All E2E tests pass
- Simulation scenarios complete without property violations

### 6.4 Acceptance Test Matrix (Required)

- [ ] **Role semantics**
  - `Member` vs `Participant` behavior validated in UI + protocol tests.
- [ ] **Moderator authorization**
  - Only `Member` can be moderator.
  - Moderator assignment/removal follows authority policy path.
- [ ] **Access mapping**
  - Full/Partial/Limited mapping validated for 0/1/2+ hops and disconnected graph.
- [ ] **Contact establishment matrix**
  - Contact establishment succeeds (or fails with explicit rule) for each supported authority pair type.
- [ ] **Determinism**
  - Identical fact sets produce identical access computation and relationship views.

### 6.5 Phase 6 Checkpoint

- [ ] **Run full test suite including integration tests**
  ```bash
  just test
  ```
- [ ] **Run simulation scenarios**
  ```bash
  cargo run -p aura-terminal -- scenario run scenarios/role_transitions.toml
  ```
- [ ] **Stage and commit Phase 6 changes**
  ```bash
  git add crates/ scenarios/
  git commit -m "test: add E2E and simulation tests for new terminology

  - Hop distance calculation tests
  - Access level enforcement tests
  - Member creation without Owner privilege
  - Moderator operation tests
  - Role transition simulation scenario"
  ```

---

## Phase 7: TUI Updates

Reference: `docs/116_cli_tui.md`, `docs/807_system_internals_guide.md`

### 7.1 Update Existing Screens

- [ ] **Home screen**
  - Replace Owner/Admin/Resident labels with Member/Moderator/Participant
  - Update role display logic

- [ ] **Neighborhood screen**
  - Update terminology
  - Show hop distances if relevant

- [ ] **Contacts screen**
  - Support contact creation with any authority type

### 7.2 New Screens/Flows

- [ ] **Moderator assignment modal**
  - Select member to assign/unassign moderator
  - Confirm operation

- [ ] **Access level override modal**
  - Show current access level for user
  - Allow upgrade/downgrade within bounds
  - Confirm operation

- [ ] **Home capability configuration screen**
  - Configure what Full/Partial/Limited can do
  - Per-home settings

### 7.3 Signal Updates

- [ ] **Update `HOME_SIGNAL`**
  - Include new role model (Member/Moderator/Participant)
  - Include moderator designations

- [ ] **Update `CONTACTS_SIGNAL`**
  - Support all authority type contacts

**Success criteria:**
- TUI compiles
- Manual navigation through updated screens works
- No visual regressions

### 7.4 Phase 7 Checkpoint

- [ ] **Run full CI checks**
  ```bash
  just fmt-check
  just clippy
  just test
  ```
- [ ] **Run TUI state machine tests**
  ```bash
  cargo test -p aura-terminal --test unit_state_machine
  ```
- [ ] **Stage and commit Phase 7 changes**
  ```bash
  git add crates/aura-terminal/ crates/aura-app/
  git commit -m "feat(tui): update screens for new terminology and add moderation flows

  - Update Home/Neighborhood/Contacts screens with new labels
  - Add moderator assignment modal
  - Add access level override modal
  - Add home capability configuration screen
  - Update HOME_SIGNAL and CONTACTS_SIGNAL for new model"
  ```

---

## Phase 8: Manual Testing (Runtime Harness)

Reference: `docs/804_testing_guide.md`

### 8.1 Harness Configuration

- [ ] **Create `configs/harness/terminology-test.toml`**
  ```toml
  schema_version = 1

  [run]
  name = "terminology-test"
  pty_rows = 40
  pty_cols = 120
  seed = 5555

  [[instances]]
  id = "alice"
  mode = "local"
  data_dir = ".tmp/harness/terminology/alice"
  bind_address = "127.0.0.1:42001"

  [[instances]]
  id = "bob"
  mode = "local"
  data_dir = ".tmp/harness/terminology/bob"
  bind_address = "127.0.0.1:42002"

  [[instances]]
  id = "carol"
  mode = "local"
  data_dir = ".tmp/harness/terminology/carol"
  bind_address = "127.0.0.1:42003"
  ```

### 8.2 User Flow: Home Creation with New Roles

- [ ] **Scenario: `scenarios/harness/home-roles.toml`**
  ```toml
  schema_version = 1
  id = "home-roles-flow"
  goal = "Verify home creation uses Member role, not Owner"
  execution_mode = "scripted"

  [[steps]]
  id = "create-home"
  action = "send_keys"
  instance = "alice"
  expect = "..."  # Navigate to home creation

  [[steps]]
  id = "verify-member-label"
  action = "wait_for"
  instance = "alice"
  expect = "Member"  # Not "Owner"
  timeout_ms = 5000
  ```

  **Success criteria:** Home creator sees "Member" role, not "Owner"

### 8.3 User Flow: Moderator Assignment

- [ ] **Scenario: `scenarios/harness/moderator-assign.toml`**

  Flow:
  1. Alice creates home (becomes Member)
  2. Bob joins home (becomes Member)
  3. Alice assigns Bob as Moderator
  4. Verify Bob shows Moderator badge
  5. Carol joins as Participant
  6. Bob kicks Carol (moderator action)
  7. Verify Carol is removed

  **Success criteria:** Full moderator flow completes

### 8.4 User Flow: Access Level Override

- [ ] **Scenario: `scenarios/harness/access-override.toml`**

  Flow:
  1. Alice's home in Neighborhood N
  2. Bob's home in Neighborhood N (1-hop from Alice)
  3. Verify Bob has Partial access to Alice's home
  4. Alice downgrades Bob to Limited
  5. Verify Bob now has Limited access
  6. Carol is 2-hop from Alice
  7. Alice upgrades Carol to Partial
  8. Verify Carol now has Partial access

  **Success criteria:** Access overrides work correctly

### 8.5 User Flow: Shared Storage (renamed from Public Good Space)

- [ ] **Scenario: `scenarios/harness/shared-storage.toml`**

  Flow:
  1. Alice creates home
  2. Navigate to storage settings
  3. Verify "Shared Storage" label (not "Public Good Space")
  4. Pin a fact
  5. Verify pinned fact appears correctly

  **Success criteria:** New terminology displays correctly

### 8.6 User Flow: Cross-Authority Contact

- [ ] **Scenario: `scenarios/harness/cross-authority-contact.toml`**

  Flow:
  1. Alice (User) creates contact with Home H
  2. Verify invitation flow works
  3. Verify contact appears in contacts list
  4. Home H creates contact with Neighborhood N
  5. Verify invitation flow works

  **Success criteria:** Contact works between all authority type pairs

### 8.7 Phase 8 Checkpoint

- [ ] **Run harness lint on all scenarios**
  ```bash
  just harness-lint -- --config configs/harness/terminology-test.toml --scenario scenarios/harness/home-roles.toml
  just harness-lint -- --config configs/harness/terminology-test.toml --scenario scenarios/harness/moderator-assign.toml
  just harness-lint -- --config configs/harness/terminology-test.toml --scenario scenarios/harness/access-override.toml
  just harness-lint -- --config configs/harness/terminology-test.toml --scenario scenarios/harness/shared-storage.toml
  just harness-lint -- --config configs/harness/terminology-test.toml --scenario scenarios/harness/cross-authority-contact.toml
  ```
- [ ] **Run all harness scenarios**
  ```bash
  just harness-run -- --config configs/harness/terminology-test.toml --scenario scenarios/harness/home-roles.toml
  just harness-run -- --config configs/harness/terminology-test.toml --scenario scenarios/harness/moderator-assign.toml
  just harness-run -- --config configs/harness/terminology-test.toml --scenario scenarios/harness/access-override.toml
  just harness-run -- --config configs/harness/terminology-test.toml --scenario scenarios/harness/shared-storage.toml
  just harness-run -- --config configs/harness/terminology-test.toml --scenario scenarios/harness/cross-authority-contact.toml
  ```
- [ ] **Stage and commit Phase 8 changes**
  ```bash
  git add configs/harness/ scenarios/harness/
  git commit -m "test: add runtime harness scenarios for terminology changes

  - Home creation with Member role (not Owner)
  - Moderator assignment and kick flow
  - Access level override (1-hop→Limited, 2-hop→Partial)
  - Shared Storage label verification
  - Cross-authority contact creation"
  ```

---

## Phase 9: Cleanup

### 9.1 Remove Discovery Script

- [ ] **Delete `scripts/find-terminology.sh`**
  - Script is no longer needed after all renames are complete
  ```bash
  rm scripts/find-terminology.sh
  git add -A scripts/
  git commit -m "chore: remove terminology discovery script (no longer needed)"
  ```

---

## Verification Checklist

After completing all phases:

- [ ] `just build` passes
- [ ] `just check` passes
- [ ] `just fmt-check` passes
- [ ] `just clippy` passes
- [ ] `just test` passes
- [ ] Discovery script reports zero matches for old terms (before Phase 9 cleanup removes the script)
- [ ] All property tests pass (10,000+ cases each)
- [ ] All E2E tests pass
- [ ] All simulation scenarios pass
- [ ] All runtime harness scenarios pass
- [ ] Manual TUI walkthrough confirms terminology changes
- [ ] `docs/106_consensus.md` contains Witness rationale note
- [ ] `CLAUDE.md` uses new terminology consistently
- [ ] `.claude/skills/aura-quick-ref/SKILL.md` uses new terminology

---

## File Summary

### New Files
- `scripts/find-terminology.sh` - Discovery script
- `configs/harness/terminology-test.toml` - Harness config
- `scenarios/harness/home-roles.toml`
- `scenarios/harness/moderator-assign.toml`
- `scenarios/harness/access-override.toml`
- `scenarios/harness/shared-storage.toml`
- `scenarios/harness/cross-authority-contact.toml`

### Modified Files (Key)

**Documentation**
- `docs/002_theoretical_model.md`
- `docs/003_information_flow_contract.md`
- `docs/004_distributed_systems_contract.md`
- `docs/005_system_invariants.md`
- `docs/114_social_architecture.md`
- `docs/112_relational_contexts.md`
- `docs/102_authority_and_identity.md`
- `docs/106_consensus.md`
- `docs/116_cli_tui.md`
- `docs/999_project_structure.md`
- `docs/000_project_overview.md`

**Project Meta**
- `CLAUDE.md` (terminology in Architecture Essentials)
- `.claude/skills/aura-quick-ref/SKILL.md` (terminology quick reference)

**Crates**
- `crates/aura-core/src/domain/*.rs` (AccessLevel rename)
- `crates/aura-social/src/*.rs` (Role changes, terminology)
- `crates/aura-app/src/views/*.rs` (View model updates)
- `crates/aura-terminal/src/tui/screens/*.rs` (TUI updates)
