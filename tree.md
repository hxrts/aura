# Channel Ratchet Design (Message-Count Slots with Overlap)

Goal: Introduce per-channel ratchets that provide short forward-secrecy windows, non-blocking transitions, and minimal state. Avoid wall clocks; use message counts and optional consensus-backed bumps for idle channels.

## Design Overview
- Per `(ContextId, peer)` maintain directional ratchets with slot-based keys.
- Slots advance every `N` messages (tunable), with a fixed overlap of the previous slot (`Δ` messages after the new slot appears).
- Always keep two slots “hot”: `slot_k` (current) and `slot_{k+1}` (next). Pre-derive `slot_{k+2}` when `slot_{k+1}` becomes active.
- Envelopes carry `(slot_id, counter)`; counters advance per message; bounded skip window per slot tolerates reordering.
- During overlap, accept both current and previous slot; once overlap expires (by message count), drop the old slot. No drops on boundary as long as within `Δ`.
- Idle channels: allow a “ratchet proposal” that establishes a fresh slot id via lightweight DH. For high-value/control paths, optionally route the proposal through the existing epoch/consensus choreography to synchronize slot ids without traffic.
- Checkpoints: periodically emit tiny facts/receipts with `(slot_id, counter, skip_window)` to support recovery; align checkpoint GC with maintenance snapshots.

## Message Flow
1. Establish channel → derive initial `slot_0` from context key + fresh DH; counters start at 0.
2. Send path: on each message, increment counter; if `counter % N == 0`, mark `slot_{k+1}` as available and tag outgoing with the new `slot_id`. Continue accepting `slot_k` for the next `Δ` messages seen.
3. Receive path: accept if `(slot_id, counter)` fits current or previous slot within skip/overlap; advance counters and store minimal skipped cache.
4. Overlap end: after `Δ` messages since first seeing `slot_{k+1}`, drop acceptance of `slot_k`.
5. Rekey prep: when `slot_{k+1}` becomes active, pre-derive `slot_{k+2}` (key material stays local until used).

## idle Ratchet Bump
- If no messages for a configurable duration or count hysteresis, allow a peer to propose a new slot via a DH handshake message.
- For critical/admin flows, allow an optional consensus-backed “ratchet proposal” fact (using existing epoch rotation choreography) to sync the new slot id across participants without traffic.

## State and Persistence
- Per direction: `(current_slot_id, current_counter, skip_cache)` and `(next_slot_id, next_counter)`.
- Skip cache bounded to tolerate out-of-order delivery; overlap bounded by `Δ`.
- Periodic checkpoints (facts/receipts) store `(slot_id, counter, skip_window_bounds)` for deterministic recovery; GC checkpoints with snapshots per `docs/807_maintenance_ota_guide.md`.

## Envelope Format Additions
- Add `slot_id` and `slot_counter` fields; retain padding to fixed size.
- Key IDs derived deterministically from context key + slot index (or DH for proposals).

## Failure Modes
- Message arrives with old `slot_id` outside overlap → fail and trigger fast re-establish.
- Counter beyond skip window → fail and re-establish.
- Missing checkpoints on recovery → re-establish channel; bound loss to last checkpoint window.

## Security & PCS
- Short FS window: compromise yields at most `N+Δ` messages before slot advances.
- Replay bounded by overlap + skip window; enforce strict expiry.
- No wall-clock dependency; advancement is driven by message count or explicit proposal.

## UX Considerations
- Non-blocking boundaries via overlap; no pauses on rekey under normal traffic.
- Idle channels can refresh via proposal without user prompt; only suspicious/admin bumps use blocking consensus.
- Clear error telemetry when a re-establish is needed (counter/slot window exceeded).

## Work Plan
- [ ] Parameterize `N` (messages per slot) and `Δ` (overlap) with safe defaults; wire into config.
- [ ] Extend envelope schema to carry `slot_id` and `slot_counter`; ensure padding unchanged.
- [ ] Implement per-direction slot state (current/next) with bounded skip cache; add overlap acceptance logic.
- [ ] Add slot advancement on send/recv (message-count based); pre-derive next slot when new becomes active.
- [ ] Add idle ratchet proposal flow (lightweight DH); for high-value paths, integrate optional consensus-backed proposal via existing epoch rotation choreography.
- [ ] Implement periodic checkpoint emission (facts/receipts with slot_id,counter,skip bounds); integrate checkpoint GC with maintenance snapshots.
- [ ] Add fast re-establish path when slot/windows are exceeded; ensure no consensus is invoked for routine channel rekey.
- [ ] Tests: loss/reorder within window, overlap boundary, exceeding window triggers re-establish, recovery from checkpoints, idle proposal, consensus-backed proposal for admin use.
- [ ] Docs: update transport/info-flow PCS sections, tree_comparison PCS notes, and theoretical model to describe slot-based ratchets, overlap behavior, recovery/checkpointing, and GC alignment.
