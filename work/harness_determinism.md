# Harness Determinism Plan

This document defines the work required to make Aura's PTY + Playwright harness predictable, typed, and maintainable while keeping the real Aura runtime as the primary end-to-end validation path.

Goals:
- make browser and TUI scenario execution uniform
- eliminate ad hoc selectors, text scraping, and sleep-based waits from core scenarios
- move to typed controls, typed observations, and deterministic readiness/operation state
- ensure both frontends expose a stable, machine-readable UI state contract
- keep the real-runtime harness lane reliable enough to be the default developer and LLM feedback loop
- make Quint, simulator, and harness responsibilities clean and non-overlapping
- enforce parity between web and TUI for shared flows while allowing explicit environment-specific exceptions

Non-goals:
- preserve all current harness internals unchanged
- make simulator-backed execution the default correctness oracle
- enforce pixel or layout identity between web and TUI

Guiding principles:
- the harness validates the real software by default
- the simulator is a first-class alternate substrate, not the primary harness lane
- semantic state is the source of truth; rendered text is a fallback/debug aid
- shared flows should be described once semantically and executed through adapters
- parity means semantic capability and outcome parity, not identical implementation details

## Phase 1: Shared UI Contract in `aura-app`

### 1. Create a shared UI contract module in `aura-app`
- [ ] Add a shared UI contract module under `aura-app` containing typed identifiers and shared UI-facing semantic types

Implementation guidance:
- define stable ids such as `ScreenId`, `ControlId`, `FieldId`, `ModalId`, `ListId`, `ToastKind`, and related core enums in one shared module
- keep the contract application-facing and frontend-neutral
- do not put DOM selectors, PTY keys, or renderer-specific details into this contract

Success criteria:
- `aura-app` contains the shared UI contract module
- `aura-ui`, `aura-web`, `aura-terminal`, and `aura-harness` can depend on these types through `aura-app`
- no new duplicated string constants for core UI identities are introduced outside the shared contract module

### 2. Define a shared `UiSnapshot` model
- [ ] Add a typed shared snapshot model covering the common logical UI state

Implementation guidance:
- include current screen, focused control, open modal, selected entities, visible lists keyed by stable ids, readiness, operation state, and toasts
- model semantic state rather than rendered prose

Success criteria:
- there is one canonical `UiSnapshot` type under `aura-app`
- the snapshot uses stable domain ids or typed UI ids instead of row indexes and display text
- the snapshot shape is expressive enough for both web and TUI observation

### 3. Checkpoint: validate shared UI contract foundation and commit
- [ ] Run targeted checks for the shared UI contract and create a dedicated git commit for this phase

Implementation guidance:
- run focused compile/test coverage for crates that depend on the new contract boundary
- keep this phase in a dedicated commit so later migrations have a clean baseline

Success criteria:
- targeted compile checks for `aura-app`, `aura-ui`, `aura-terminal`, and `aura-harness` are clean
- any contract serialization or type-level tests added for this phase pass
- the phase lands in a dedicated git commit

## Phase 2: Shared Semantic Scenario Contract in `aura-app`

### 4. Define the canonical semantic scenario contract in `aura-app`
- [ ] Add a shared semantic scenario contract that is independent of TUI keys, Playwright selectors, and renderer-specific details

Implementation guidance:
- define typed scenario actions, expectations, actor identifiers, and environment controls under `aura-app`
- keep the contract semantic and application-facing, not frontend-facing
- use this contract as the common handoff format between Quint trace generation, simulator execution, and harness execution

Success criteria:
- there is one canonical typed scenario contract under `aura-app`
- the contract can express shared user flows, runtime/environment controls, and expected semantic outcomes
- the contract contains no PTY key sequences, DOM selectors, CSS ids, or renderer-specific actions

### 5. Tighten harness config typing around the semantic contract
- [ ] Eliminate free-form scenario configuration fields where typed alternatives exist

Implementation guidance:
- parse scenario actions, expectations, semantic ids, and environment controls into typed enums/newtypes at load time
- reject malformed or unsupported scenario descriptions early

Success criteria:
- scenario parsing rejects invalid action names, invalid expectation names, and malformed semantic ids at load time
- raw stringly typed control/field/screen references are replaced by typed parsing where the shared contract covers them
- config validation errors are precise and actionable

### 6. Unify TOML scenario formats under the semantic contract
- [ ] Replace parallel TOML dialects with one canonical semantic scenario schema

Implementation guidance:
- keep TOML only if it is representing semantic actions and expectations, not frontend mechanics
- define one schema that can represent actors, actions, expectations, and environment controls
- provide adapters or migration tools for older scenario formats if needed

Success criteria:
- there is one primary TOML schema for semantic scenarios
- scenario files do not encode raw keys or selectors as the primary representation for shared flows
- Quint-exported traces and hand-authored harness scenarios use the same conceptual schema

### 7. Add clean handoff boundaries between Quint, simulator, and harness
- [ ] Formalize the boundaries and data flow between model generation, runtime substrate, and frontend execution

Implementation guidance:
- document and implement the pipeline as:
  - Quint generates semantic traces
  - simulator provides deterministic runtime execution conditions when explicitly selected
  - harness executes semantic traces against real frontends
- keep each layer responsible for one concern only

Success criteria:
- the codebase has a clear boundary between model generation, runtime/environment execution, and frontend interaction/observation
- data passed between layers uses shared typed contracts rather than ad hoc per-layer structures
- ownership of failures is clearer because each layer has a narrower responsibility

### 8. Checkpoint: validate semantic scenario contract and commit
- [ ] Run targeted checks for the semantic scenario contract and create a dedicated git commit for this phase

Implementation guidance:
- add focused tests for parsing, contract translation, and compatibility adapters
- confirm that the semantic contract can represent at least one existing TUI flow and one web flow without renderer-specific fields

Success criteria:
- targeted tests for semantic scenario parsing and translation pass
- compile checks for `aura-app`, `aura-harness`, and any Quint export/import adapters are clean
- the phase lands in a dedicated git commit

## Phase 3: Deterministic Observation Foundations

### 9. Expose a read-only web UI state endpoint
- [ ] Add a read-only harness/dev-mode browser state export for deterministic observation

Implementation guidance:
- expose a read-only state surface in harness/dev mode, for example `window.__AURA_UI_STATE__()`
- return the shared `UiSnapshot` shape or a trivially translatable equivalent
- do not expose action methods through this surface

Success criteria:
- browser harness assertions can use structured UI state instead of DOM text scraping for core flows
- the state surface is read-only and machine-friendly
- the exported state maps cleanly into `UiSnapshot`

### 10. Expose a machine-readable TUI state export
- [ ] Add a TUI-side structured state export matching the shared UI snapshot contract

Implementation guidance:
- expose current screen, focus tree, selected ids, modal state, input mode, list contents, readiness, and operations in a machine-readable form
- keep rendered text as a debugging surface, not the primary state transport

Success criteria:
- the PTY harness does not need to infer focus or selection from rendered text for core flows
- the TUI export maps cleanly into the shared `UiSnapshot`
- structured TUI state is available to harness assertions

### 11. Separate interaction and observation channels
- [ ] Rework harness internals so interaction and observation are independent and explicit

Implementation guidance:
- web interaction uses Playwright while web observation uses structured UI state
- TUI interaction uses PTY keystrokes while TUI observation uses structured TUI state
- keep DOM scraping and PTY text scraping only as fallback/debug paths

Success criteria:
- interaction and observation paths are clearly separated in the harness architecture
- core scenario assertions no longer depend on raw text scraping by default
- fallback paths are non-primary and explicitly marked as such

### 12. Checkpoint: validate structured observation and commit
- [ ] Run targeted checks for web/TUI structured observation and create a dedicated git commit for this phase

Implementation guidance:
- add focused tests for `UiSnapshot` export/import from both web and TUI
- run at least one browser and one PTY scenario using structured observation as the primary assertion path

Success criteria:
- targeted browser and TUI observation tests pass cleanly
- at least one web scenario and one TUI scenario use structured observation end to end
- the phase lands in a dedicated git commit

## Phase 4: Stable Semantic Addressing

### 13. Make control identity first-class and typed
- [ ] Replace ad hoc ids/selectors with typed control identity across both frontends

Implementation guidance:
- every actionable web control and TUI element should map to a stable typed `ControlId`
- generate renderer-specific ids from the shared contract rather than hand-writing them

Success criteria:
- harness steps can target `ControlId` values directly without embedding CSS selectors or button labels
- control identity does not depend on layout position, visible wording, or list row order
- both frontends expose the same semantic control identities for shared flows

### 14. Standardize modal and form structure
- [ ] Make all modals and forms conform to a uniform typed structure

Implementation guidance:
- each modal should have a stable `ModalId`, typed field ids, and primary/secondary actions
- each form should expose typed field ids and a stable submit action

Success criteria:
- browser and TUI drivers can interact with modal/form controls using the same semantic identifiers
- shared flows do not depend on bespoke modal mechanics per frontend
- modal/form state is visible in `UiSnapshot`

### 15. Standardize app-shell structure in the web UI
- [ ] Ensure the browser app has a single deterministic root and stable shell regions

Implementation guidance:
- standardize one app root, one modal root, one toast region, and stable screen roots
- derive shell ids/test ids from the shared contract rather than ad hoc strings

Success criteria:
- Playwright never has to infer the correct app shell heuristically
- shell structure is stable and intentionally addressable
- shell ids are consistent across harness-covered flows

### 16. Require every list item and selection to be id-based
- [ ] Eliminate row-index and text-based selection from both harness-facing UIs and require stable list item identity

Implementation guidance:
- represent all list selections by stable ids, not row indexes
- ensure contact rows, channels, homes, notifications, devices, and settings items each have stable ids derived from domain identity or typed UI identity

Success criteria:
- all list selections are represented by stable ids
- harness flows can select list items without using visible labels as the primary key
- the snapshot reports selected entities and visible list items by typed ids

### 17. Model optimistic vs confirmed state explicitly
- [ ] Distinguish local optimistic UI state from confirmed runtime state

Implementation guidance:
- make the snapshot explicitly represent pending local state versus confirmed runtime state where applicable
- do not let scenarios infer confirmation indirectly from timing or toasts

Success criteria:
- harness expectations can wait for confirmed state instead of accidentally passing on optimistic placeholders
- pending local state is explicit and inspectable
- no core scenario needs to guess whether a change is actually confirmed

### 18. Checkpoint: validate stable addressing and commit
- [ ] Run targeted checks for control/list identity and create a dedicated git commit for this phase

Implementation guidance:
- add focused tests for id generation, modal/form addressing, and list selection semantics
- run one browser and one TUI scenario driven only by semantic control/list ids

Success criteria:
- targeted tests for control identity and list identity pass
- at least one web and one TUI scenario complete without raw selectors or row-index assumptions for shared flows
- the phase lands in a dedicated git commit

## Phase 5: Deterministic Readiness and Operation State

### 19. Add explicit screen readiness contracts
- [ ] Give each screen a deterministic readiness signal

Implementation guidance:
- each screen should report readiness/loading state explicitly in the snapshot
- do not infer readiness from incidental text or arbitrary delays

Success criteria:
- harnesses do not interact with a screen until readiness is true
- readiness is explicit, observable, and stable
- screen startup races are reduced because readiness is a first-class concept

### 20. Add explicit operation lifecycle state
- [ ] Represent async operation lifecycle uniformly in the snapshot

Implementation guidance:
- major operations should report `idle`, `submitting`, `succeeded`, or `failed`
- key operations by typed operation ids or typed action context

Success criteria:
- harness waits can target operation lifecycle directly instead of depending on secondary UI artifacts like toasts
- operation state is comparable across web and TUI for shared flows
- operation completion is semantically visible in the snapshot

### 21. Enforce no arbitrary sleeps in core scenarios
- [ ] Remove sleep-based synchronization from primary scenario flows

Implementation guidance:
- replace sleeps with readiness, state change, selector presence, or operation lifecycle waits
- keep any remaining sleeps confined to explicitly marked low-level/debug scenarios and justify them

Success criteria:
- core scenarios contain no arbitrary time sleeps for synchronization
- waits are semantic and bounded
- timing-based flakiness from avoidable sleeps is reduced

### 22. Checkpoint: validate readiness and lifecycle semantics and commit
- [ ] Run targeted checks for readiness/lifecycle semantics and create a dedicated git commit for this phase

Implementation guidance:
- add focused tests for readiness transitions and operation lifecycle transitions
- run representative async flows in both web and TUI using lifecycle-driven waits

Success criteria:
- targeted readiness/lifecycle tests pass
- representative async scenarios no longer rely on arbitrary sleeps
- the phase lands in a dedicated git commit

## Phase 6: Real-Runtime Harness Hardening

### 23. Make real-runtime harness execution the explicitly hardened primary lane
- [ ] Add an explicit project policy and implementation plan that treats the real-runtime harness lane as the primary end-to-end validation path

Implementation guidance:
- document that the harness should validate the real Aura runtime and real frontends by default
- treat simulator-backed runs as complementary, not as an excuse for real-runtime flakiness
- prioritize engineering work that removes avoidable nondeterminism from the real-runtime path

Success criteria:
- the plan and docs explicitly identify the real-runtime harness lane as the primary validation loop
- engineering work is framed around reducing harness-induced nondeterminism in the real-runtime path
- simulator-backed runs are documented as an alternate lane, not the default correctness oracle

### 24. Standardize deterministic environment provisioning for real-runtime runs
- [ ] Make per-scenario environment setup predictable and isolated

Implementation guidance:
- assign deterministic temp dirs, profile dirs, instance ids, ports, and artifact paths per run
- ensure scenario startup does not depend on incidental machine state or leftover processes
- centralize resource allocation rather than scattering it across drivers and scripts

Success criteria:
- each real-runtime scenario gets isolated and predictable filesystem, port, and profile resources
- repeated runs do not interfere with each other through leaked state
- environment provisioning is reproducible and diagnosable

### 25. Harden startup and readiness sequencing for real-runtime runs
- [ ] Replace heuristic startup waits with explicit health and readiness checks

Implementation guidance:
- add bounded startup phases for runtime processes, TUI sessions, browser sessions, and web servers
- make each phase report success/failure explicitly
- do not allow interaction until all required readiness gates have passed

Success criteria:
- harness startup uses explicit, bounded readiness checks instead of fragile timing heuristics
- failures during startup are attributable to a specific phase
- scenarios do not begin interacting with partially started systems

### 26. Harden teardown and process cleanup
- [ ] Make scenario shutdown deterministic and leak-resistant

Implementation guidance:
- track spawned processes and child resources centrally
- add teardown verification for lingering processes, bound ports, temp dirs, and browser contexts
- fail loudly when cleanup is incomplete rather than leaving latent interference for later runs

Success criteria:
- scenario teardown leaves no unintended long-lived processes or bound ports behind
- leaked processes and stale resources are detected and reported
- repeated local development runs do not degrade over time due to harness residue

### 27. Add a harness-mode runtime profile for the real runtime
- [ ] Introduce a real-runtime harness mode that improves determinism without changing core semantics

Implementation guidance:
- reduce or disable avoidable nondeterminism in harness mode, such as unnecessary animation, unstable debounce, or poorly bounded polling behavior
- keep core runtime semantics unchanged
- make the harness mode explicit and inspectable

Success criteria:
- the real runtime can run in a harness-oriented mode that improves determinism and observability
- this mode does not replace the real runtime with a simulator or mock
- developers can tell when a scenario is using harness-mode runtime settings

### 28. Add deterministic resource and port management policies
- [ ] Remove ad hoc port selection and resource ownership from the real-runtime harness path

Implementation guidance:
- centralize port allocation and ownership tracking
- ensure browser, web server, TUI, and backend resources are allocated predictably and do not race one another
- make port and resource collisions actionable failures rather than mysterious hangs

Success criteria:
- resource ownership is centralized and visible
- port collisions and resource conflicts are detected early and reported clearly
- harness runs do not rely on opportunistic port selection or hidden defaults

### 29. Add per-layer failure attribution for real-runtime runs
- [ ] Make failures in the real-runtime lane attributable to the responsible layer

Implementation guidance:
- distinguish frontend interaction failures, structured observation failures, runtime startup failures, backend RPC failures, and cleanup failures
- report failures with layer-specific context rather than generic timeout messages

Success criteria:
- failures are classified by layer with actionable diagnostics
- developers can tell whether a failure came from the harness, frontend, runtime, transport, or environment setup
- silent stalls are replaced by bounded, attributable failures

### 30. Add process-leak and residue checks to local and CI workflows
- [ ] Automatically detect harness residue that would make subsequent real-runtime runs flaky

Implementation guidance:
- add checks for leaked child processes, stale browser profiles, stale lock files, and ports that remained bound after teardown
- integrate these checks into the harness lifecycle and optionally into CI or local verification commands

Success criteria:
- the harness can detect and report residue from previous runs
- local developer loops surface leak problems early instead of silently accumulating instability
- CI can fail fast when harness residue would taint subsequent runs

### 31. Checkpoint: validate real-runtime hardening and commit
- [ ] Run targeted real-runtime harness checks and create a dedicated git commit for this phase

Implementation guidance:
- run representative real-runtime TUI, web, and mixed scenarios repeatedly enough to catch startup/teardown/resource issues
- use this checkpoint to confirm the harness substrate itself is no longer the main source of flakiness for these flows

Success criteria:
- targeted repeated-run checks for real-runtime scenarios are clean
- startup, teardown, and residue checks pass locally
- the phase lands in a dedicated git commit

## Phase 7: Quint, Simulator, and Harness Separation of Concerns

### 32. Deprecate direct Quint-to-TUI driving
- [ ] Remove or quarantine Quint infrastructure whose responsibility is to drive the TUI directly

Implementation guidance:
- identify the current Quint MBT components that emit or depend on TUI-oriented steps
- replace them with export into the shared semantic scenario contract
- keep any temporary compatibility adapter narrow and clearly marked for removal

Success criteria:
- Quint no longer owns TUI key-driving behavior as a primary execution path
- Quint outputs semantic traces rather than frontend-specific scripts
- any remaining Quint-to-TUI compatibility path is temporary, clearly documented, and not used by default

### 33. Make the harness the single frontend execution layer
- [ ] Consolidate real frontend execution under the harness with dedicated TUI and web drivers

Implementation guidance:
- route semantic scenario actions through the harness, not through a separate MBT frontend executor
- keep frontend-specific translation inside driver implementations only
- ensure both TUI and web use the same scenario action model and semantic expectations

Success criteria:
- there is one primary executor for real frontend scenarios
- TUI execution and web execution are both adapters of the same semantic scenario model
- no second, parallel frontend-driving stack remains for MBT flows

### 34. Make the simulator a first-class alternate deterministic runtime substrate
- [ ] Make the simulator a first-class alternate substrate for harness scenarios while keeping the real runtime as the default execution substrate

Implementation guidance:
- separate runtime/environment control from frontend interaction in the scenario model
- make network topology, timing, partitions, and delivery controls expressible as semantic environment operations
- keep the real Aura runtime as the default backend for harness scenarios and LLM-driven validation
- use the simulator as an explicit alternate backend for scenarios that need deterministic distributed conditions, failure injection, or MBT replay

Success criteria:
- harness scenarios run against the real runtime by default
- the simulator can be selected explicitly as an alternate runtime substrate for shared harness scenarios
- environment controls are applied through a typed interface rather than ad hoc test hooks
- deterministic and fault-injection scenarios do not require a separate bespoke MBT runtime path

### 35. Add migration tasks for existing Quint and harness infrastructure
- [ ] Create and execute a migration path from current MBT/TUI-specific infrastructure to the unified model

Implementation guidance:
- inventory current Quint trace generation, MBT execution, harness scenario parsing, and simulator integration points
- move in phases:
  - define shared contract
  - add adapters
  - migrate high-value scenarios
  - deprecate old paths
- do not attempt a flag-day rewrite unless the scope is truly small

Success criteria:
- there is a concrete migration sequence for existing MBT and harness code
- high-value shared scenarios are migrated first
- old execution paths are either removed or clearly marked as legacy with planned deletion

### 36. Update architecture and testing docs for the new separation
- [ ] Update documentation so the project describes the new role boundaries and testing pipeline clearly

Implementation guidance:
- update the authoritative docs under `docs/`, not just `work/`
- document the responsibilities of Quint, simulator, harness, and frontends
- document the canonical scenario contract and parity expectations

Success criteria:
- authoritative documentation explains:
  - Quint as model/trace generation
  - simulator as a selectable deterministic runtime substrate
  - harness as the real frontend executor
  - `aura-app` as the home of shared semantic scenario and UI/parity contracts
- developers can understand where new testing logic belongs without relying on tribal knowledge

### 37. Add policy checks to prevent responsibility drift
- [ ] Add automated checks so the old architectural blur does not reappear

Implementation guidance:
- add checks that flag frontend-driving logic in the wrong layer
- add checks that flag frontend-specific actions inside semantic scenario contracts
- add checks that flag new parallel scenario dialects or duplicate execution paths

Success criteria:
- CI or local policy checks fail if:
  - Quint code starts depending on TUI/web interaction details again
  - semantic scenario contracts include frontend-specific mechanics
  - new parallel scenario formats are introduced without going through the shared contract
- the intended separation of concerns is enforceable, not aspirational

### 38. Checkpoint: validate responsibility split and commit
- [ ] Run targeted checks for the Quint/simulator/harness split and create a dedicated git commit for this phase

Implementation guidance:
- validate that one semantic trace can flow through the expected handoff boundaries without frontend-specific payloads leaking into the contract
- confirm that the main real-frontend execution path goes through the harness

Success criteria:
- targeted architectural and integration checks pass
- the new responsibility boundaries are exercised by at least one migrated flow
- the phase lands in a dedicated git commit

## Phase 8: Scenario Migration and Diagnostics

### 39. Replace raw selectors in scenario files with semantic references
- [ ] Make scenarios reference semantic ids instead of CSS selectors for primary flows

Implementation guidance:
- keep selectors isolated to browser-driver internals or generated mappings
- use semantic control/list/modal references in scenarios wherever the contract supports them

Success criteria:
- core scenario files use semantic references rather than raw selectors for shared flows
- selector strings are isolated to the driver layer or generated artifacts
- scenario intent is readable without frontend-specific mechanics leaking through

### 40. Add a minimum viable migration checkpoint set
- [ ] Migrate one TUI scenario, one web scenario, one mixed scenario, and one Quint-exported trace through the new semantic path before mass migration

Implementation guidance:
- choose representative high-value flows
- use these as the proving ground before migrating the full scenario inventory

Success criteria:
- one TUI, one web, one mixed, and one Quint-originated flow all execute through the new semantic scenario contract and harness path
- issues found here are resolved before mass migration begins
- the team has a concrete reference implementation for subsequent migrations

### 41. Migrate core scenarios to semantic actions and state-based assertions
- [ ] Rewrite the main end-to-end scenarios to use the new deterministic foundation

Implementation guidance:
- move high-value scenarios first
- replace text-based and frontend-mechanics-heavy assertions with semantic state waits

Success criteria:
- primary mixed web/TUI scenarios use semantic actions, typed observations, and structured state waits
- core scenarios no longer depend on raw DOM text, incidental toast copy, or inferred PTY rendering details
- scenario readability improves because steps express intent rather than frontend mechanics

### 42. Add scenario debugger artifacts
- [ ] Capture structured debugging data automatically on scenario failure

Implementation guidance:
- capture at least: last actions, structured UI snapshot, browser screenshot or TUI capture, relevant logs, and operation state
- tie artifacts to the failing step automatically

Success criteria:
- developers can diagnose a failure without reproducing it manually first
- failure artifacts are easy to locate and include the key structured state needed for triage
- the harness produces bounded, useful diagnostics on failure

### 43. Add flaky-scenario detection and timing metrics
- [ ] Track instability trends before they become chronic failures

Implementation guidance:
- record per-step timing and failure/timeout trends
- surface scenarios with increasing variance or repeated retries

Success criteria:
- scenario timing and failure variance is observable
- unstable scenarios can be identified before they become chronic CI failures
- timing metrics are usable for targeted hardening work

### 44. Add lint/policy checks for determinism rules
- [ ] Enforce determinism policies automatically

Implementation guidance:
- fail when core scenarios introduce arbitrary sleeps, raw selectors, or raw text assertions where banned
- keep these checks scoped to the new deterministic scenario model

Success criteria:
- CI or local linting fails when core scenarios violate the determinism rules
- new harness-covered flows cannot silently regress into brittle mechanics
- determinism policy checks are precise and actionable

### 45. Deprecate legacy fallback paths
- [ ] Remove or quarantine old brittle observation and interaction paths

Implementation guidance:
- delete obsolete fallback paths where feasible
- where deletion is not immediate, mark them clearly as debug-only compatibility paths with planned removal

Success criteria:
- core scenarios do not depend on legacy fallback paths
- DOM scraping and PTY text scraping are either removed or clearly relegated to debug-only fallback status
- the harness architecture clearly separates supported deterministic APIs from temporary compatibility code

### 46. Checkpoint: validate migrated scenarios and diagnostics and commit
- [ ] Run targeted migrated-scenario checks and create a dedicated git commit for this phase

Implementation guidance:
- validate the minimum viable migrated set plus at least a few additional high-value scenarios
- ensure debugger artifacts and determinism checks are active and useful

Success criteria:
- targeted migrated scenarios run cleanly through the new path
- diagnostics and policy checks work on real failures
- the phase lands in a dedicated git commit

## Phase 9: Web/TUI Parity Foundation and Enforcement

### 47. Define an explicit parity contract in `aura-app`
- [ ] Add a shared parity contract describing what must match between web and TUI, and what is allowed to diverge by environment

Implementation guidance:
- describe shared screens, shared actions, shared modals, shared list views, and shared operation flows
- declare parity exceptions explicitly and type them

Success criteria:
- parity exceptions are explicitly declared, typed, and justified rather than emerging ad hoc
- harness-covered flows have an explicit declaration of whether they are `shared`, `web_only`, or `tui_only`
- examples of allowed exceptions include environment-specific capabilities such as browser theme controls

### 48. Mirror screen/module structure between web and TUI
- [ ] Reorganize or standardize frontend code layout so corresponding screens and major flows are easy to match across implementations

Implementation guidance:
- mirror screen and major flow structure where it improves predictability
- allow deliberate mappings where exact mirroring would be artificial, but document them explicitly

Success criteria:
- a developer can locate the implementation of a given shared screen or flow in both frontends predictably
- parity-critical screens and flows are no longer scattered under unrelated files without a stable mapping
- the mapping is documented and machine-checkable where possible

### 49. Mirror shared definition names across web and TUI
- [ ] Standardize naming for shared screen definitions, actions, modal flows, and major UI concepts

Implementation guidance:
- use canonical identifiers from `aura-app` wherever a shared concept exists
- allow renderer-specific names only when the feature is explicitly an environment-specific exception

Success criteria:
- shared screen names, modal names, and flow names use the same canonical identifiers across web and TUI
- there are no duplicated but drifting names for the same shared concept in harness-covered code paths
- renaming a shared flow or screen in one frontend requires updating the shared contract, making drift visible

### 50. Add a parity checking script for structure and naming
- [ ] Add a repository script that checks mirrored structure, naming, and shared-flow coverage between web and TUI

Implementation guidance:
- drive the script from the shared parity contract rather than hardcoded one-off heuristics
- distinguish between true violations and declared parity exceptions

Success criteria:
- a local script can detect missing mirrored screen modules, missing shared flow definitions, and naming drift for parity-covered features
- the script is suitable for local development and CI use
- parity violations are reported in a developer-actionable way

### 51. Add parity snapshots for key shared screens
- [ ] Add comparable semantic snapshots for shared screens and verify them across web and TUI

Implementation guidance:
- compare semantic capability and state, not rendering details
- use the shared `UiSnapshot` model as the comparison surface

Success criteria:
- parity checks can compare screen structure at the semantic level: selected item, visible actions, modal availability, operation state, and readiness
- parity is not defined as pixel or text equality; it is defined as semantic capability and state equivalence
- mismatches are reported in a developer-actionable way

### 52. Add harness-driven parity scenarios
- [ ] Add scenarios that execute the same shared user flow in both frontends and compare the semantic results

Implementation guidance:
- use the shared scenario contract plus the shared parity contract
- compare semantic outcomes, not rendering details
- allow declared environment-specific exceptions without fragmenting the main scenario model

Success criteria:
- for shared flows, the harness can execute equivalent web and TUI scenarios against the same semantic action model
- parity assertions compare structured state and operation outcomes
- the harness can report where one frontend is missing a shared action, screen state, or operation outcome

### 53. Enforce parity for newly added shared flows
- [ ] Add policy checks so new shared flows cannot land in only one frontend silently

Implementation guidance:
- require parity manifest updates when adding new shared screens, modals, or actions
- allow explicit parity exceptions when justified

Success criteria:
- CI or local checks fail when a new shared flow is added to one frontend without corresponding parity contract updates
- adding a new shared screen, modal, or action requires updating the shared parity manifest and either implementing both frontends or declaring an explicit exception
- parity regressions are caught during development, not discovered later by ad hoc manual testing

### 54. Add developer documentation for parity rules
- [ ] Document how shared frontends should stay aligned and how exceptions must be declared

Implementation guidance:
- describe parity expectations, naming rules, structure rules, exception handling, and enforcement tooling
- point contributors to the parity contract, parity checking script, and parity scenarios

Success criteria:
- contributors can tell whether a new feature belongs in both frontends or is an approved environment-specific exception
- there is a clear developer-facing doc describing parity expectations and enforcement
- the doc points to the parity contract, harness parity scenarios, and parity checking script

### 55. Checkpoint: validate parity foundation and commit
- [ ] Run targeted parity checks and create a dedicated git commit for this phase

Implementation guidance:
- run the parity script plus a small set of parity scenarios for representative shared flows
- validate that declared exceptions are handled correctly and non-exceptions fail appropriately

Success criteria:
- targeted parity checks are clean
- representative shared flows pass parity validation across TUI and web where expected
- the phase lands in a dedicated git commit

## Phase 10: Full Harness Scenario Recovery and Green Matrix

### 56. Inventory and classify all existing harness scenarios
- [ ] Create a complete inventory of current harness scenarios and classify each as `shared`, `web_only`, `tui_only`, `legacy`, or `to_be_removed`

Implementation guidance:
- include current execution path, frontend coverage, runtime substrate expectations, and migration status
- use this inventory as the authoritative tracker for the final green matrix work

Success criteria:
- every harness scenario has an explicit classification and migration status
- there is no ambiguity about which scenarios are expected to run on which frontends
- legacy or redundant scenarios are identified explicitly rather than lingering invisibly

### 57. Migrate all shared harness scenarios to the unified deterministic model
- [ ] Update every shared harness scenario to use the new semantic contract, deterministic waits, and structured observation

Implementation guidance:
- prioritize high-value shared scenarios first, but do not stop until the full shared set is migrated
- eliminate renderer-specific mechanics from shared scenario definitions

Success criteria:
- all shared scenarios use the unified semantic scenario model
- shared scenarios no longer depend on raw selectors, raw keys, arbitrary sleeps, or incidental text as primary mechanics
- the migrated shared set is maintainable and consistent

### 58. Get all TUI harness scenarios clean end to end
- [ ] Ensure all TUI-designated scenarios run cleanly on the TUI lane

Implementation guidance:
- use the scenario inventory to track completeness and failures
- fix runtime, harness, and TUI issues uncovered by the full run rather than papering over them with scenario-local hacks

Success criteria:
- every scenario classified as `shared` or `tui_only` runs cleanly on the TUI lane unless explicitly marked legacy or removed
- failures are actionable product or harness issues, not unexplained flake
- the TUI lane is clean enough to trust as a routine development feedback loop

### 59. Get all web harness scenarios clean end to end
- [ ] Ensure all web-designated scenarios run cleanly on the web lane

Implementation guidance:
- use the same inventory-driven approach as the TUI lane
- fix webapp gaps, parity gaps, and harness/browser issues uncovered by the full run

Success criteria:
- every scenario classified as `shared` or `web_only` runs cleanly on the web lane unless explicitly marked legacy or removed
- the web lane is clean enough to trust as a routine development feedback loop
- shared flows that fail only on web are treated as parity or implementation bugs, not accepted drift

### 60. Add a dual-run matrix for shared scenarios
- [ ] Ensure every shared scenario is runnable on both TUI and web as part of the final harness matrix

Implementation guidance:
- use the parity contract and scenario inventory together
- make the frontend target explicit and systematic rather than ad hoc

Success criteria:
- every scenario classified as `shared` is runnable on both TUI and web through the harness
- the final matrix makes frontend coverage visible and enforceable
- parity exceptions are explicit rather than hidden in scenario duplication

### 61. Add aggregate local and CI commands for the full scenario matrix
- [ ] Add commands that exercise the final scenario matrix predictably in local development and CI

Implementation guidance:
- provide separate commands for focused local work and broader matrix verification
- make lane selection and scenario selection explicit

Success criteria:
- developers can run the relevant harness matrix locally without bespoke manual orchestration
- CI can run the appropriate matrix subsets reliably
- the command surface reflects the shared/web/TUI classification clearly

### 62. Final checkpoint: run the full harness matrix, ensure it is clean, and commit
- [ ] Run the full targeted harness matrix for the migrated scenario set, confirm it is clean, and create a dedicated git commit for the final recovery phase

Implementation guidance:
- use the scenario inventory and classifications to decide the expected matrix
- this checkpoint should represent the end state of the refactor, not just an intermediate sample

Success criteria:
- the expected migrated harness matrix is clean across TUI and web for all non-legacy scenarios
- shared scenarios run on both frontends where required
- the final recovery phase lands in a dedicated git commit

## Recommended execution order
1. Shared UI contract in `aura-app`
2. Shared semantic scenario contract in `aura-app`
3. Deterministic observation foundations
4. Stable semantic addressing
5. Readiness and operation lifecycle state
6. Real-runtime harness hardening
7. Quint/simulator/harness responsibility split
8. Scenario migration and diagnostics
9. Web/TUI parity foundation and enforcement
10. Full harness scenario recovery and green matrix

## Definition of done
- mixed web/TUI scenarios run against real frontends using semantic, typed actions
- harness assertions primarily use structured UI state, not fragile text scraping
- browser and TUI flows are addressable through the same conceptual model
- shared web/TUI flows are covered by an explicit parity contract with declared exceptions
- parity drift in shared screens, naming, and flow coverage is detectable automatically
- Quint, simulator, and harness each have a clearly separated responsibility with clean typed handoffs
- there is one canonical semantic scenario model rather than parallel frontend-specific scenario dialects
- the real-runtime harness lane is the primary end-to-end validation path and is engineered for high reliability
- real-runtime runs have deterministic startup, teardown, resource allocation, and failure attribution
- all non-legacy harness scenarios are classified, migrated, and clean on their expected frontend matrix
- failures are bounded, diagnosable, and artifact-rich
- core scenarios are deterministic enough to be trusted in local development and CI
