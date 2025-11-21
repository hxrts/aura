# Aura Consensus

Aura Consensus specifically designed for small groups communicating peer-to-peer with intermittent connectivity. Peers must form a stable committee for a given context, though members may be offline at any moment. The committee is only used when a specific decision needs agreement. The group does not maintain a global ordered log. Its only requirement is to agree on the operations that matter for that context.

Aura focuses on agreement over single, important operations. Most state evolves through CRDT merges, which need no coordination. However, a small set of actions cannot merge safely, such as changes to account authority. These actions run inside a context with a fixed witness group that shares threshold key material.

This setting makes fast consensus possible. Aura reaches finality in one round trip. Each witness verifies the same pre-state and produces one share, which initiator combines into a threshold signature. The signature is a complete proof of agreement.

If the witnesses disagree or the initiator stalls, the group falls back to gossiping shares until some witness gathers a valid threshold. It produces the same final signature and the same commit fact.

Aura provides strong agreement only when needed. It keeps the rest of the system simple and available through CRDTs.


## Consensus as Fact Emission

Aura does not maintain a global log. Instead it treats consensus as the production of a single commit fact. A fact is an immutable value added to the CRDT replica store, which is guaranteed to be convergent.

```rust
struct CommitFact {
    cid: ConsensusId,
    rid: ResultId,          // result id = H(operation, prestate)
    threshold_signature: ThresholdSignature,
    attester_set: BTreeSet<DeviceId>,
}
```

Consensus safety reduces to agreement on which commit fact exists. Every peer merges this fact into its replica store. Once a threshold signed commitment fact exists, it is final.


## Fast Path

The initiator begins by sending an `Execute` message to all witnesses. This message carries the operation and a hash of the pre-state it must apply to. Each witness validates this pre-state. If it matches, the witness executes the operation deterministically and returns a `WitnessShare` that includes the result id and its share.

The initiator collects at least *t* matching shares. It combines them into a threshold signature and forms the attester set directly from the witnesses whose shares were used. It writes a commit fact to the replica store and broadcasts a `Commit` message.

The initiator coordinates the fast path, which completes in one round trip.


**Messages**

- `Execute(cid, Op, prestate_hash, evidΔ)`
- `WitnessShare(cid, rid, share, prestate_hash, evidΔ)`
- `Commit(cid, rid, sig, attesters, evidΔ)`

Assume witness set `W`, threshold `t`, fallback timeout `T_fallback`.


**Initiator i**

```haskell
State:
  cid      := fresh_consensus_id()
  Op       := input operation
  shares   := {}                  // witness_id -> (rid, share)
  decided  := {}                  // decided[cid] = Bool, initially false

1. Start:
    prestate := ReadState()
    prestate_hash := H(prestate)
    For all w in W:
        Send Execute(cid, Op, prestate_hash, EvidenceDelta(cid)) to w

2. On WitnessShare(cid, rid, share, prestate_hash, evidΔ) from w:
    MergeEvidence(cid, evidΔ)
    If decided[cid] = false and w not in shares:
        shares[w] := (rid, share)
        H := { (w', s') in shares | s'.rid = rid }
        If |H| ≥ t:
            sig := CombineShares({ s'.share | (_, s') in H })
            attesters := { w' | (w', _) in H }
            CommitFact(cid, rid, sig, attesters)
            For all v in W:
                Send Commit(cid, rid, sig, attesters, EvidenceDelta(cid)) to v
            decided[cid] := true

3. On Commit(cid, rid, sig, attesters, evidΔ):
    If decided[cid] = false and VerifyThresholdSig(rid, sig, attesters):
        MergeEvidence(cid, evidΔ)
        CommitFact(cid, rid, sig, attesters)
        decided[cid] := true
```


**Witness w**

```haskell
State:
  proposals := {}                 // rid -> set(w, share, prestate_hash)
  decided   := {}                 // decided[cid] = Bool
  timers    := {}                 // timers[cid]

1. On Execute(cid, Op, prestate_hash, evidΔ) from i:
    MergeEvidence(cid, evidΔ)
    If decided[cid] = false:
        prestate := ReadState()
        If H(prestate) != prestate_hash:
            // Explicitly notify initiator and participate in fallback
            Send StateMismatch(cid, prestate_hash, H(prestate), EvidenceDelta(cid)) to i
            StartTimer(cid, T_fallback)
            return

        rid := H(Op, prestate)
        share := ProduceShare(cid, rid)

        proposals[rid] := { (w, share, prestate_hash) }
        Send WitnessShare(cid, rid, share, prestate_hash, EvidenceDelta(cid)) to i
        StartTimer(cid, T_fallback)

2. On Commit(cid, rid, sig, attesters, evidΔ):
    If decided[cid] = false and VerifyThresholdSig(rid, sig, attesters):
        MergeEvidence(cid, evidΔ)
        CommitFact(cid, rid, sig, attesters)
        decided[cid] := true
        StopTimer(cid)

3. OnTimer(cid, T_fallback):
    If decided[cid] = false:
        periodic.Start(cid)
```


## Evidence Propagation

Evidence is a CRDT keyed by `cid`. It tracks the final attesters and signature once known and merges only monotonically. A commit fact is always inserted with an monotonic commit rule: once a threshold signature appears for `cid`, it cannot be replaced.

Every message that participates in a consensus instance carries an evidence delta. The session protocol is structured so that these deltas move with the communication pattern. Peers merge evidence on send and receive. Reconvergent peers align automatically.

```haskell
EvidenceDelta(cid):
    return CRDT_Delta_for(cid)

MergeEvidence(cid, evidΔ):
    CRDT_Merge(cid, evidΔ)
```


## Fallback

Fallback runs when witnesses disagree on the result id, disagree on the pre-state hash, or when the initiator fails to return a commit within a timeout. Each witness forms proposals keyed by `(rid, prestate_hash)`. Each proposal contains the signature share produced by that witness.

Witnesses exchange their proposal maps with a sparse overlay. Each witness merges incoming maps, checking for equivocation, where equivocation is defined strictly as a witness producing two shares for the same `cid` and the same `prestate_hash` but different `rid`. When any proposal accumulates *t* valid, non-equivocating shares, the witness produces a threshold signature and broadcasts a ThresholdComplete message.

All witnesses accept the first valid signature they see.


**Messages**

- `Conflict(cid, proposals, evidΔ)`
- `AggregateShare(cid, proposals, evidΔ)`
- `ThresholdComplete(cid, rid, sig, attesters, evidΔ)`
- `StateMismatch(cid, expected_hash, actual_hash, evidΔ)`


**Initiator i (trigger)**

```haskell
1. On detecting conflicting rids or prestate_hashes in shares for cid:
    conflicts := proposals extracted from received shares
    For all w in W:
        Send Conflict(cid, conflicts, EvidenceDelta(cid)) to w
```


**Witness w (fallback)**

```haskell
State:
  proposals := {}                // (rid, prestate_hash) -> set(w, share)
  decided   := {}                // decided[cid]
  k         := 3                 // fanout
  periodic  := per-cid periodic timers

1. On Conflict(cid, conflicts, evidΔ):
    MergeEvidence(cid, evidΔ)
    merge conflicts into proposals
    CheckThreshold(cid)
    periodic.Start(cid)

2. On AggregateShare(cid, proposals', evidΔ):
    MergeEvidence(cid, evidΔ)
    For each entry (rid, pre_hash) with set S':
        For each (w', sh') in S':
            If not HasEquivocated(proposals, w', cid, pre_hash):
                proposals[(rid, pre_hash)] := proposals[(rid, pre_hash)] ∪ { (w', sh') }
    CheckThreshold(cid)

3. CheckThreshold(cid):
    For each (rid, pre_hash) in proposals:
        S := proposals[(rid, pre_hash)]
        If decided[cid] = false and |S| ≥ t:
            sig := CombineShares({ sh | (_, sh) in S })
            If VerifyThresholdSig(rid, sig):
                attesters := { w | (w, _) in S }
                CommitFact(cid, rid, sig, attesters)
                For all v in W:
                    Send ThresholdComplete(cid, rid, sig, attesters, EvidenceDelta(cid)) to v
                decided[cid] := true
                periodic.Stop(cid)

4. On ThresholdComplete(cid, rid, sig, attesters, evidΔ):
    If decided[cid] = false and VerifyThresholdSig(rid, sig, attesters):
        MergeEvidence(cid, evidΔ)
        CommitFact(cid, rid, sig, attesters)
        decided[cid] := true
        periodic.Stop(cid)

5. On periodic tick for cid and decided[cid] = false:
    peers := SampleRandomSubset(W \ {w}, k)
    For each p in peers:
        Send AggregateShare(cid, proposals, EvidenceDelta(cid)) to p

HasEquivocated(proposals, witness, cid, pre_hash):
    Return true if witness has produced two shares for same cid and same pre_hash but different rid
    Otherwise false
```


## Properties

Aura consensus uses an initiator for the 1-RTT fast path and a leaderless gossip protocol for fallback. It relies on local timers and does not assume synchronous links. It is scoped to a context rather than to the entire network. It achieves atomic agreement on a single operation rather than maintaining a total order.

Aura tolerates peers going offline, message delays, and partitions. It produces compact signatures that propagate efficiently. It integrates with CRDT state and session typed protocols already used in Aura.

Consensus is only invoked when needed. Most operations rely on CRDT merge. When required, the system can produce strong agreement with minimal coordination.


## Extended Properties

Aura runs a single-shot consensus instance inside a context-scoped group with shared threshold key material. The initiator first attempts a fast path: it sends `Execute`, witnesses verify the pre-state, compute a deterministic `rid`, and return shares. If the initiator collects `t` matching shares it produces a threshold signature and commits. If witnesses disagree on the `rid` or the initiator fails, witnesses switch to a leaderless fallback: they gossip their proposal maps until some witness sees `t` non-equivocating shares for one candidate and produces a threshold signature. Evidence and commit facts are CRDTs, so all peers converge even after offline periods.

We assume partial synchrony with a global stabilization time `GST`. After `GST`, every message between honest peers arrives within delay `Δ` (and we write `δ ≤ Δ` for the actual delay). The adversary controls fewer than `t` key shares and each honest witness signs at most one `(cid, rid, prestate_hash)` triple.

A party finalizes `(cid, rid)` only after verifying a valid threshold signature on `rid`.


### 1. Liveness (Fast Path)

If all honest witnesses compute the same `rid = H(Op, prestate)` and the initiator is honest, the fast path completes in one round trip.

**1-RTT finality Claim**
Let `t > f`. Suppose the initiator is honest and at least `t` honest witnesses compute the same `rid` for `(cid, Op)`. Then all honest parties finalize within `O(δ)` time after the initiator sends `Execute`.

**Proof Sketch**

At time `t0 > GST`, the initiator performs:

```haskell
// Initiator
broadcast Execute(cid, Op, prestate_hash)
```

Within `δ`, every honest witness receives it:

```haskell
// Witness w
if H(ReadState()) == prestate_hash:
    rid := H(Op, ReadState())
    share := SignShare(cid, rid)
    send WitnessShare(cid, rid, share)
```

All honest witnesses compute the same `rid`. Within another `δ`, the initiator receives `t` matching shares:

```haskell
// Initiator
if |{share_w | rid_w == rid}| >= t:
    sig := CombineShares(...)
    broadcast Commit(cid, rid, sig)
```

Every honest witness verifies `sig`:

```haskell
// Witness
if Verify(sig, rid):
    finalize(cid, rid)
```

All honest parties have finalized by time `t0 + 3δ`.

∎


### 2. Eventual Liveness (Fallback)

If witnesses disagree on the result id or the initiator stalls, fallback gossip ensures eventual completion whenever `t` honest witnesses eventually become connected.

**Eventual Liveness Claim**
If after some time `T ≥ GST`, at least `t` honest witnesses can exchange messages eventually, then with probability 1 some honest witness produces a threshold signature for some `(cid, rid)`, and all honest parties eventually finalize.

**Proof Sketch**

Each witness maintains merged proposals:

```haskell
// proposals : Map[(rid, prestate_hash) -> Set[(w, share)]]
on AggregateShare(msg):
    merge(msg.proposals)
```

Every gossip step pushes proposals to random peers:

```haskell
periodic:
    pset := SampleRandomSubset(W, k)
    for p in pset:
        send AggregateShare(cid, proposals)
```

Honest shares for a fixed `(rid, prestate_hash)` diffuse like epidemic gossip. Because eventually connected honest witnesses form a connected overlay with probability 1, every honest witness in that component eventually gathers all honest shares.

Once some witness has `t` non-equivocating shares:

```haskell
// Witness w
if |proposals[(rid, pre_hash)]| >= t:
    sig := CombineShares(proposals[(rid, pre_hash)])
    broadcast ThresholdComplete(cid, rid, sig)
```

Every honest peer eventually receives `ThresholdComplete`, verifies it, and finalizes. Only one threshold signature can exist (by Agreement below), so all honest peers converge on the same commit fact.

∎


### 3. Agreement

Two different result ids cannot both be finalized for the same `cid`.

**Agreement Claim**
For a fixed `cid`, no two honest parties can finalize different `(cid, rid1)` and `(cid, rid2)` with `rid1 ≠ rid2`.

**Proof Sketch**

An honest party finalizes only after verifying a threshold signature:

```haskell
if VerifyThresholdSig(rid, sig):
    finalize(cid, rid)
```

Producing a threshold signature for `rid` requires at least `t` valid shares. Each honest witness produces at most one share per `(cid, prestate_hash)`, and equivocation is rejected in fallback:

```haskell
// reject if same witness signs two rids under same prestate hash
if witness signed (rid1, pre_hash) and (rid2, pre_hash):
    discard share
```

Thus, for two different result ids to obtain valid threshold signatures:

- `rid1` would require `t` honest shares
- `rid2` would also require `t` honest shares
- The two sets of honest signers would have to be disjoint

This is impossible when signing is scoped to the same consensus instance—an honest witness cannot contribute to two different candidates. Therefore only one threshold signature can exist.

∎


### 4. Validity

Finalized results correspond to deterministic execution of the intended operation.

**Validity Claim**
If `(cid, rid)` is finalized, then `rid = H(Op, prestate)` for some honest witness’s deterministic execution of `Op`.

**Proof Sketch**

Honest witnesses sign only after verifying the pre-state hash and executing the operation deterministically:

```haskell
// Witness
if H(ReadState()) == prestate_hash:
    rid := H(Op, ReadState())
    share := SignShare(cid, rid)
```

Thus every honest share corresponds to a correct deterministic execution. A valid threshold signature necessarily includes at least one honest share (since the adversary controls fewer than `t` key shares), and therefore `rid` must come from some honest execution.

∎


## Comparison

CRDTs ensure eventual consistency and availability, but typically lack atomic agreement.

Threshold-signing systems have strong atomicity for a single value, but lack consensus semantics. They assume fixed participants, no adversarial message flow, no fallback path, no accountability, no recovery.

Classical consensus protocols have full consensus semantics, but lack 1-RTT deterministic finality and on-demand operation in intermittently connected groups. They require ordering, coordinators or view changes, and stable committees.


## Summary

These properties give Aura 1-RTT deterministic finality in the normal case, leaderless eventual completion in disagreement or initiator failure, strict agreement via threshold signature uniqueness, and semantic validity through deterministic execution and pre-state checking. CRDT-based commit facts ensure convergence even under offline operation or partial message loss.

Aura provides a consensus protocol designed for mobile devices and privacy scoped groups. It builds on CRDTs, session types, and threshold signatures to provide 1-RTT deterministic finality in the happy path. It resolves conflicts using a threshold race over a sparse random overlay.

Aura consensus fits the needs of social recovery and account level control. It avoids global validators, global logs, and global epochs. It provides atomic agreement inside a context without sacrificing the peer-to-peer and offline friendly model.
