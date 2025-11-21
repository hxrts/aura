# UX and Security Improvements for Aura

This memo outlines concrete UX/security improvements for Aura, what they buy us, and what would be required to implement each.

## 1) Per-message / Short-epoch Ratchets
**Goal:** Finer-grained forward secrecy (FS) and better compromise containment than daily epoch rotation. Invisible to users; visible in logs/telemetry.

**Design:** Layer a symmetric/DH ratchet atop context-derived keys. For pairwise contexts, use a simple DH + hash ratchet advancing on each send/recv. For group/broadcast, use a TreeKEM-like broadcast ratchet or per-recipient pairwise ratchets.

**Requirements:**
- Define ratchet state struct bound to `(ContextId, peer, epoch)` with persistence via Journal/effect_api or ephemeral cache per policy.
- Integrate ratchet steps into guard chain/choreography so sends/receives advance the ratchet and receipts/journal facts can bind positions when needed.
- Update `TransportEffects` to derive per-message keys from ratchet state; enforce padding remains fixed-size.
- Add recovery/fallback: on mismatch, re-establish ratchet via context key + fresh DH.

## 2) Audience-bound Envelopes
**Goal:** Harden against PITM redirect; bind messages to intended peer/host beyond TLS.

**Design:** Include `audience` field (peer key or hashed endpoint) in signed envelope headers; reject if mismatch.

**Requirements:**
- Extend envelope schema in transport to carry `audience`.
- Update signing/verification in `TransportEffects`/handler to include `audience`.
- Ensure rendezvous/relay paths propagate intended audience; add tests with tampered audience.

## 3) Pull vs Read Capability Split
**Goal:** Allow relays/sync nodes to fetch/forward ciphertext without decrypt authority; least-privilege transport.

**Design:** Introduce distinct capabilities: `pull` (retrieve bytes) vs `read` (decrypt). Guard chain enforces both for decryption paths; transport only needs `pull`.

**Requirements:**
- Extend capability model and guard evaluation to recognize `pull` vs `read`.
- Update CLI/API semantics where users grant capabilities.
- Adjust relay/transport coordinators to request only `pull`.

## 4) Compact Delta Reconciliation (RIBLT-style)
**Goal:** Faster sync and lower bandwidth for large context sets.

**Design:** Use RIBLT (or equivalent) set reconciliation for journal/context deltas and membership/state summaries.

**Requirements:**
- Add RIBLT-based delta exchange to anti-entropy/gossip for contexts with large fact sets.
- Define symbol encoding for fact hashes or state digests; handle failure cases with fallback full sync.
- Benchmarks under skewed delta sizes.

## 5) Padded, Fixed-size Envelopes by Default
**Goal:** Reduce side-channel leakage; consistent envelope size across contexts.

**Design:** Standardize padding policy in transport; enforce fixed-size encrypted envelopes and rotation schedule for tags/rtags.

**Requirements:**
- Update transport envelope builder to always pad to configured size; expose sizing in config with safe defaults.
- Add tests asserting indistinguishability (fixed length, randomized padding).

## 6) Offline-friendly Revocation UX
**Goal:** Make revocations/epoch bumps visible and bounded for users returning from offline periods.

**Design:** Cache pending revokes/epoch changes; surface “pending revokes” and force channel rekey on reconnect.

**Requirements:**
- Track revocation/epoch bump events in journal facts; present in CLI/agent API.
- Ensure guard chain enforces rekey on reconnect when epochs advanced.
- Minimal UI/logging layer to display pending actions.

## 7) Minimal, Deterministic State Views
**Goal:** Improve trust/debuggability; hide fact churn behind a clean projection.

**Design:** Present authority/context state as deterministic reductions plus consensus decisions; expose clear “current” view.

**Requirements:**
- Define projection surfaces (authority state, context membership, budgets) derived from facts/consensus outputs.
- Add CLI/API endpoints to retrieve these views; keep them read-only and deterministic.
- Tests to verify projections are stable under fact reordering.

## 8) Guard-chain Telemetry & Human-friendly Errors
**Goal:** Better operator UX when sends are blocked.

**Design:** Include structured reasons (missing capability, budget exhausted, journal coupling failure) with peer/context info; optionally include proposer/prestate for consensus-invoked steps.

**Requirements:**
- Extend guard chain error types with structured metadata.
- Update logging/CLI surfaces to render concise explanations.
- Add tests for error propagation through choreography/transport.

## 9) Stateless-by-default Advantages (Aura vs. Beehive/TreeKEM-style)
**Goal:** Leverage Aura’s stateless posture (fact-log + deterministic reduction + stateless handlers) to deliver UX/security that TreeKEM/BeeKEM stacks can’t without mutable tree state.

**What Aura can do that mutable-tree systems struggle with:**
- Full replay/audit: any node can recompute state from AttestedOps facts; Beehive/TreeKEM maintain mutable tree snapshots that aren’t trivially recomputable from an append-only log.
- Deterministic convergence under partition: facts merge by set union; reduction is deterministic. TreeKEM-style systems must reconcile mutable trees with conflict resolution and can carry unresolved conflict keys.
- Evidence-rich debugging: Aura can show “why” (facts + consensus decisions) for any state; TreeKEM-style stacks have less transparent lineage once trees are mutated.
- Clean effect injection/testing: stateless effect handlers and reduction make mocking/replay trivial; mutable tree state requires bespoke fixtures and careful sequencing.

**Requirements/guardrails:**
- Keep reductions fast via snapshots/checkpointing to avoid slow rehydrate.
- Manage fact-log growth (compaction/pruning rules) without losing replayability.
- Cache projections for latency-sensitive paths, but treat cache as a view, not state of record.

## 10) PCS Improvements Without Sacrificing UX or Stateless Recovery
**Goal:** Tighten post-compromise security (PCS) beyond coarse epoch rotation while keeping UX smooth and preserving recovery-from-facts.

**Suggestions:**
- Short-epoch/activity-triggered rekeys: bump context/channel keys after N messages or T minutes; record as facts so recovery replays the same sequence; no user prompts.
- Ratchet checkpoints: add per-channel ratchets that advance on send/recv but checkpoint positions periodically as tiny facts/receipts. On recover, restore to last checkpoint then re-establish; bounds replay without per-message log bloat.
- Auto epoch bump on suspicion: when guard chain sees repeated MAC failures or unusual retries, trigger an automatic epoch bump via fact/consensus to force fresh keys.
- Threshold share refresh: run lightweight resharing/refresh (same public key) on a cadence to regain PCS after partial compromise; capture refresh as facts.
- Storage rewraps: periodically rewrap stored blobs with fresh derived keys; emit rewrap markers as facts so rehydrate can replay the same rewraps.
- Bounded skip/ratchet window aligned with GC: carry a limited skip window tied to ratchet checkpoints and the maintenance/OTA GC model (`docs/807_maintenance_ota_guide.md`), so offline peers can catch up without unbounded state.
- Guard-integrated steps: advance ratchets/rekeys during guard/receipt creation to avoid extra UX or round trips; keep telemetry concise to avoid leaking timing/volume patterns.

**Requirements:**
- Define ratchet/checkpoint fact shapes; ensure reduction/recovery can restore to checkpoints deterministically.
- Wire rekey triggers into transport/guard chain; add tests for offline catch-up within the bounded window.
- Coordinate skip/window GC with snapshot/maintenance so discarded state matches the mathematically sound GC model.

**Tasks (avoid UX disruption from high-activity churn):**
- [ ] Design ratchet checkpoint fact schema and receipt binding (with bounded skip window aligned to GC).
- [ ] Implement per-channel ratchet with periodic checkpoints (message/time-based), decoupled from threshold ceremonies; ensure activity-triggered rekeys use lightweight re-establish flows, not full consensus.
- [ ] Add activity-based rekey policy with rate limits/backoff to prevent bursts from triggering disruptive ceremonies; document defaults.
- [ ] Update transport/guard chain to advance ratchets and apply checkpoints; add offline catch-up tests within window.
- [ ] Add auto suspicious-event epoch bump gated by backoff and user-configurable thresholds; ensure it records a fact/consensus only when necessary.
- [ ] Implement storage rewrap markers and periodic rewrap job; ensure rehydrate replays rewraps deterministically.
- [ ] Update docs: PCS section (tree_comparison.md), transport/info-flow docs, maintenance/OTA guide for skip-window/GC alignment, and theoretical model note on ratchet checkpoints.
- [ ] Add phased (“zero-latency”) rekey path: derive next keys/ratchets in advance, overlap old/new for a bounded window with key IDs, flip when ready/acked; record overlap bounds in facts; ensure overlap expiration aligns with GC.
- [ ] Separate suspicious-activity rekey pathway: blocking rotation (no overlap) triggered by guarded thresholds; document UX impact and defaults.
- [ ] Harmonize with existing epoch rotation (aura-sync) and FROST resharing:
    - Reuse `aura-sync` epoch rotation choreography for authority/context epoch bumps (including suspicious blocking bumps); add a “suspicious bump” trigger into that pipeline rather than a new flow.
    - Implement the routine channel PCS path as a transport/guard feature: phased/overlap rekeys with key IDs, bounded overlap windows, and checkpoint facts—no new consensus or epoch rotation for activity-driven rekeys.
    - Define minimal fact shapes for overlap window/key-id/checkpoint so rehydrate/replay is deterministic; align window expiry with maintenance GC.
    - Add a “refresh without new pubkey” mode to the existing FROST resharing choreography for share refresh; schedule independent of activity-based channel rekeys.
    - Update docs (tree_comparison, transport/info-flow, theoretical model) to describe single-lane authority/context epoch rotation vs. fast channel rekey, and the role of FROST resharing in key PCS.
- [ ] Add optional, non-binding intent metadata for consensus-gated/high-value operations (e.g., tree policy changes, resharing) to fail fast when obviously stale; use as advisory preflight only—do not block offline or low-latency flows.
