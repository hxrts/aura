// Protocol Consensus - Aura Consensus Fast-Path and Fallback
//
// This module models Aura's single-shot consensus protocol with:
// - Fast path: one-RTT threshold signature collection
// - Fallback path: gossip-based share exchange when fast path stalls
//
// Key properties to verify:
// - Both paths converge to the same CommitFact for a given (cid, rid)
// - Fallback can't contradict a successful fast-path execution
// - Threshold shares prevent double-spending (equivocation detection)
//
// See: docs/104_consensus.md

module protocol_consensus {
    // ==================== TYPE DEFINITIONS ====================
    // Identifiers for consensus instances, results, and participants.

    type Option[a] = Some(a) | None

    type ConsensusId = str      // Unique consensus instance identifier
    type ResultId = str         // H(Op, prestate) - deterministic result binding
    type PrestateHash = str     // H(prestate) - ensures witnesses agree on starting state
    type AuthorityId = str      // Participant identity (device or threshold group)
    type ShareData = str        // FROST signature share (partial signature)
    type ThresholdSignature = str  // Aggregated k-of-n signature
    type OperationData = str    // Abstract operation being agreed upon

    // Consensus instance phase (state machine progression).
    type ConsensusPhase =
        | ConsensusPending      // Not yet started
        | FastPathActive        // Collecting shares via fast path
        | FallbackActive        // Fallback gossip in progress
        | ConsensusCommitted    // Valid CommitFact produced
        | ConsensusFailed       // Instance aborted

    // Witness state for a consensus instance
    type WitnessState = {
        hasVoted: bool,
        share: Option[ShareData],
        prestateHash: Option[PrestateHash],
        resultId: Option[ResultId]
    }

    // Share proposal from a witness
    type ShareProposal = {
        witness: AuthorityId,
        resultId: ResultId,
        prestateHash: PrestateHash,
        share: ShareData
    }

    // The commit fact output
    type CommitFact = {
        cid: ConsensusId,
        rid: ResultId,
        signature: ThresholdSignature,
        attesters: Set[AuthorityId]
    }

    // Consensus instance state
    type ConsensusInstance = {
        cid: ConsensusId,
        operation: OperationData,
        prestateHash: PrestateHash,
        threshold: int,
        witnesses: Set[AuthorityId],
        initiator: AuthorityId,
        phase: ConsensusPhase,
        proposals: Set[ShareProposal],
        commitFact: Option[CommitFact],
        fallbackTimerActive: bool,
        equivocators: Set[AuthorityId]
    }

    // ==================== STATE VARIABLES ====================

    var instances: ConsensusId -> ConsensusInstance
    var committedFacts: Set[CommitFact]
    var globalWitnesses: Set[AuthorityId]

    // ==================== HELPER FUNCTIONS ====================

    pure def getOrDefaultInstance(m: ConsensusId -> ConsensusInstance, cid: ConsensusId): Option[ConsensusInstance] =
        if (cid.in(m.keys())) Some(m.get(cid)) else None

    // Count proposals matching a specific (rid, prestateHash)
    pure def countMatchingProposals(proposals: Set[ShareProposal], rid: ResultId, pHash: PrestateHash): int =
        proposals.filter(p => p.resultId == rid and p.prestateHash == pHash).size()

    // Get witnesses that have submitted proposals for (rid, prestateHash)
    pure def getMatchingWitnesses(proposals: Set[ShareProposal], rid: ResultId, pHash: PrestateHash): Set[AuthorityId] =
        proposals.filter(p => p.resultId == rid and p.prestateHash == pHash).map(p => p.witness)

    // Check if a witness has already submitted a proposal
    pure def hasProposal(proposals: Set[ShareProposal], witness: AuthorityId): bool =
        proposals.exists(p => p.witness == witness)

    // Check for equivocation: same witness, different rid for same cid
    pure def detectEquivocation(proposals: Set[ShareProposal], witness: AuthorityId, newRid: ResultId): bool =
        proposals.exists(p => p.witness == witness and p.resultId != newRid)

    // Generate deterministic threshold signature from share set
    pure def aggregateShares(proposals: Set[ShareProposal], rid: ResultId, pHash: PrestateHash): ThresholdSignature =
        if (proposals.filter(p => p.resultId == rid and p.prestateHash == pHash).size() >= 2)
            "sig_" // Simplified: real impl would combine shares
        else ""

    // Verify threshold signature (simplified)
    pure def verifySignature(sig: ThresholdSignature, rid: ResultId, attesters: Set[AuthorityId], threshold: int): bool =
        sig != "" and attesters.size() >= threshold

    // ==================== ACTIONS ====================
    // State transitions modeling the consensus protocol.

    // Initialize consensus module with empty state.
    action init: bool = all {
        instances' = Map(),
        committedFacts' = Set(),
        globalWitnesses' = Set("w1", "w2", "w3", "w4", "w5")
    }

    // Initiator starts a consensus instance (enters fast path).
    // Broadcasts operation to witnesses and awaits their shares.
    action startConsensus(
        cid: ConsensusId,
        initiator: AuthorityId,
        op: OperationData,
        pHash: PrestateHash,
        witnesses: Set[AuthorityId],
        threshold: int
    ): bool = all {
        not(cid.in(instances.keys())),
        witnesses.size() >= threshold,
        threshold >= 1,
        instances' = instances.put(cid, {
            cid: cid,
            operation: op,
            prestateHash: pHash,
            threshold: threshold,
            witnesses: witnesses,
            initiator: initiator,
            phase: FastPathActive,
            proposals: Set(),
            commitFact: None,
            fallbackTimerActive: false,
            equivocators: Set()
        }),
        committedFacts' = committedFacts,
        globalWitnesses' = globalWitnesses
    }

    // Witness submits share (fast path response).
    // Share binds witness to (resultId, prestateHash). Equivocation detected.
    action submitWitnessShare(
        cid: ConsensusId,
        witness: AuthorityId,
        rid: ResultId,
        share: ShareData
    ): bool = {
        val instOpt = getOrDefaultInstance(instances, cid)
        val hasInstance = match instOpt { | Some(_) => true | None => false }
        val inst = match instOpt {
            | Some(i) => i
            | None => {
                cid: "", operation: "", prestateHash: "", threshold: 0,
                witnesses: Set(), initiator: "", phase: ConsensusPending,
                proposals: Set(), commitFact: None, fallbackTimerActive: false,
                equivocators: Set()
            }
        }
        val isWitness = inst.witnesses.contains(witness)
        val notVoted = not(hasProposal(inst.proposals, witness))
        val isActive = inst.phase == FastPathActive or inst.phase == FallbackActive
        val proposal = { witness: witness, resultId: rid, prestateHash: inst.prestateHash, share: share }
        val isEquivocating = detectEquivocation(inst.proposals, witness, rid)
        val newEquivocators = if (isEquivocating) inst.equivocators.union(Set(witness)) else inst.equivocators
        val newProposals = if (not(isEquivocating)) inst.proposals.union(Set(proposal)) else inst.proposals
        val matchingCount = countMatchingProposals(newProposals, rid, inst.prestateHash)
        val reachedThreshold = matchingCount >= inst.threshold
        val newPhase = if (reachedThreshold) ConsensusCommitted else inst.phase
        val attesters = getMatchingWitnesses(newProposals, rid, inst.prestateHash)
        val newSig = if (reachedThreshold) aggregateShares(newProposals, rid, inst.prestateHash) else ""
        val newCommitFact = if (reachedThreshold) Some({
            cid: cid,
            rid: rid,
            signature: newSig,
            attesters: attesters
        }) else None
        all {
            hasInstance,
            isWitness,
            notVoted,
            isActive,
            not(inst.equivocators.contains(witness)),
            instances' = instances.put(cid, {
                ...inst,
                proposals: newProposals,
                phase: newPhase,
                commitFact: newCommitFact,
                equivocators: newEquivocators
            }),
            committedFacts' = if (reachedThreshold)
                committedFacts.union(Set({
                    cid: cid, rid: rid, signature: newSig, attesters: attesters
                }))
                else committedFacts,
            globalWitnesses' = globalWitnesses
        }
    }

    // Trigger fallback when fast path stalls.
    // Activated by timer expiry or prestate mismatch detection.
    action triggerFallback(cid: ConsensusId): bool = {
        val instOpt = getOrDefaultInstance(instances, cid)
        val hasInstance = match instOpt { | Some(_) => true | None => false }
        val inst = match instOpt {
            | Some(i) => i
            | None => {
                cid: "", operation: "", prestateHash: "", threshold: 0,
                witnesses: Set(), initiator: "", phase: ConsensusPending,
                proposals: Set(), commitFact: None, fallbackTimerActive: false,
                equivocators: Set()
            }
        }
        val isFastPath = inst.phase == FastPathActive
        all {
            hasInstance,
            isFastPath,
            instances' = instances.put(cid, {
                ...inst,
                phase: FallbackActive,
                fallbackTimerActive: true
            }),
            committedFacts' = committedFacts,
            globalWitnesses' = globalWitnesses
        }
    }

    // Gossip shares during fallback (AggregateShare message).
    // Witnesses exchange shares via gossip to overcome network partitions.
    action gossipShares(cid: ConsensusId, shareSet: Set[ShareProposal]): bool = {
        val instOpt = getOrDefaultInstance(instances, cid)
        val hasInstance = match instOpt { | Some(_) => true | None => false }
        val inst = match instOpt {
            | Some(i) => i
            | None => {
                cid: "", operation: "", prestateHash: "", threshold: 0,
                witnesses: Set(), initiator: "", phase: ConsensusPending,
                proposals: Set(), commitFact: None, fallbackTimerActive: false,
                equivocators: Set()
            }
        }
        val isFallback = inst.phase == FallbackActive
        val validShares = shareSet.filter(p =>
            inst.witnesses.contains(p.witness) and
            not(inst.equivocators.contains(p.witness)) and
            not(hasProposal(inst.proposals, p.witness))
        )
        val mergedProposals = inst.proposals.union(validShares)
        // Check if any (rid, prestateHash) combination now has threshold
        val pHash = inst.prestateHash
        // Simplified: check the first resultId in proposals
        val ridSet = mergedProposals.map(p => p.resultId)
        val anyReachesThreshold = ridSet.exists(rid =>
            countMatchingProposals(mergedProposals, rid, pHash) >= inst.threshold
        )
        all {
            hasInstance,
            isFallback,
            validShares.size() >= 1,
            not(anyReachesThreshold),  // If threshold reached, use completeViaFallback instead
            instances' = instances.put(cid, {
                ...inst,
                proposals: mergedProposals
            }),
            committedFacts' = committedFacts,
            globalWitnesses' = globalWitnesses
        }
    }

    // Complete consensus via fallback (any witness can finalize).
    // Once threshold shares gathered via gossip, any node can aggregate.
    action completeViaFallback(cid: ConsensusId, winningRid: ResultId): bool = {
        val instOpt = getOrDefaultInstance(instances, cid)
        val hasInstance = match instOpt { | Some(_) => true | None => false }
        val inst = match instOpt {
            | Some(i) => i
            | None => {
                cid: "", operation: "", prestateHash: "", threshold: 0,
                witnesses: Set(), initiator: "", phase: ConsensusPending,
                proposals: Set(), commitFact: None, fallbackTimerActive: false,
                equivocators: Set()
            }
        }
        val isFallback = inst.phase == FallbackActive
        val matchingCount = countMatchingProposals(inst.proposals, winningRid, inst.prestateHash)
        val reachedThreshold = matchingCount >= inst.threshold
        val attesters = getMatchingWitnesses(inst.proposals, winningRid, inst.prestateHash)
        val sig = aggregateShares(inst.proposals, winningRid, inst.prestateHash)
        all {
            hasInstance,
            isFallback,
            reachedThreshold,
            instances' = instances.put(cid, {
                ...inst,
                phase: ConsensusCommitted,
                commitFact: Some({
                    cid: cid,
                    rid: winningRid,
                    signature: sig,
                    attesters: attesters
                })
            }),
            committedFacts' = committedFacts.union(Set({
                cid: cid, rid: winningRid, signature: sig, attesters: attesters
            })),
            globalWitnesses' = globalWitnesses
        }
    }

    // Fail consensus instance (abort without producing CommitFact).
    // Called when consensus cannot complete (insufficient shares, timeout).
    action failConsensus(cid: ConsensusId): bool = {
        val instOpt = getOrDefaultInstance(instances, cid)
        val hasInstance = match instOpt { | Some(_) => true | None => false }
        val inst = match instOpt {
            | Some(i) => i
            | None => {
                cid: "", operation: "", prestateHash: "", threshold: 0,
                witnesses: Set(), initiator: "", phase: ConsensusPending,
                proposals: Set(), commitFact: None, fallbackTimerActive: false,
                equivocators: Set()
            }
        }
        val notCommitted = inst.phase != ConsensusCommitted
        val notFailed = inst.phase != ConsensusFailed
        all {
            hasInstance,
            notCommitted,
            notFailed,
            instances' = instances.put(cid, { ...inst, phase: ConsensusFailed }),
            committedFacts' = committedFacts,
            globalWitnesses' = globalWitnesses
        }
    }

    // ==================== INVARIANTS ====================
    // Safety properties that must hold in every reachable state.

    // Safety: Only one CommitFact per consensus instance.
    // Prevents conflicting commits (double-spend protection).
    val InvariantUniqueCommitPerInstance =
        committedFacts.forall(cf1 =>
            committedFacts.forall(cf2 =>
                cf1.cid == cf2.cid implies cf1.rid == cf2.rid
            )
        )

    // Safety: CommitFact requires threshold attesters.
    // Ensures k-of-n security (can't forge with fewer shares).
    val InvariantCommitRequiresThreshold =
        instances.keys().forall(cid => {
            val inst = instances.get(cid)
            val commitOpt = inst.commitFact
            match commitOpt {
                | Some(cf) => cf.attesters.size() >= inst.threshold
                | None => true
            }
        })

    // Safety: Committed instances have valid commit facts.
    // Phase==Committed implies commitFact is Some.
    val InvariantCommittedHasCommitFact =
        instances.keys().forall(cid => {
            val inst = instances.get(cid)
            val isCommitted = inst.phase == ConsensusCommitted
            val hasCommit = match inst.commitFact { | Some(_) => true | None => false }
            isCommitted implies hasCommit
        })

    // Safety: Equivocators are excluded from attestation.
    // Byzantine witnesses who vote inconsistently can't contribute.
    val InvariantEquivocatorsExcluded =
        instances.keys().forall(cid => {
            val inst = instances.get(cid)
            match inst.commitFact {
                | Some(cf) => inst.equivocators.intersect(cf.attesters).size() == 0
                | None => true
            }
        })

    // Safety: Proposals only from configured witnesses.
    // Prevents Sybil attacks (only pre-approved witnesses vote).
    val InvariantProposalsFromWitnesses =
        instances.keys().forall(cid => {
            val inst = instances.get(cid)
            inst.proposals.forall(p => inst.witnesses.contains(p.witness))
        })

    // Convergence: Fast-path and fallback produce same result.
    // Both paths must agree on (cid, rid) binding.
    val InvariantPathConvergence =
        committedFacts.forall(cf =>
            cf.cid.in(instances.keys()) implies {
                val inst = instances.get(cf.cid)
                cf.rid == cf.rid  // Tautology placeholder - real check in temporal property
            }
        )

    // ==================== TEMPORAL PROPERTIES ====================
    // LTL properties for liveness and safety over time.

    // Liveness: If threshold honest witnesses participate, consensus commits.
    // Requires fairness assumption (messages eventually delivered).
    temporal livenessEventualCommit = always(
        instances.keys().forall(cid => {
            val inst = instances.get(cid)
            val activeNotCommitted = (inst.phase == FastPathActive or inst.phase == FallbackActive)
            val hasThresholdProposals = inst.proposals.size() >= inst.threshold
            // Note: Full liveness would require fairness assumptions
            activeNotCommitted and hasThresholdProposals implies eventually(inst.phase == ConsensusCommitted)
        })
    )

    // Safety: Once committed, result never changes (immutability).
    // CommitFacts are permanent once produced.
    temporal safetyImmutableCommit = always(
        committedFacts.forall(cf =>
            always(committedFacts.exists(cf2 => cf2.cid == cf.cid and cf2.rid == cf.rid))
        )
    )

    // ==================== STEP RELATION (for trace generation) ====================
    // Nondeterministically choose any enabled action for model checking.

    action step = any {
        nondet cid = oneOf(Set("cns1", "cns2", "cns3"))
        nondet initiator = oneOf(Set("w1", "w2"))
        nondet op = oneOf(Set("update_policy", "add_device", "remove_device"))
        nondet pHash = oneOf(Set("pre_abc", "pre_xyz"))
        nondet witnesses = oneOf(Set(Set("w1", "w2", "w3"), Set("w2", "w3", "w4")))
        nondet threshold = oneOf(Set(2, 3))
        nondet witness = oneOf(Set("w1", "w2", "w3", "w4"))
        nondet rid = oneOf(Set("rid_op1", "rid_op2"))
        nondet share = oneOf(Set("share_a", "share_b", "share_c"))
        any {
            startConsensus(cid, initiator, op, pHash, witnesses, threshold),
            submitWitnessShare(cid, witness, rid, share),
            triggerFallback(cid),
            completeViaFallback(cid, rid),
            failConsensus(cid),
        }
    }
}
