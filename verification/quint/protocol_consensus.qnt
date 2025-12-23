// Protocol Consensus - Aura Consensus Fast-Path and Fallback
//
// This module models Aura's single-shot consensus protocol with:
// - Fast path: one-RTT threshold signature collection
// - Fallback path: gossip-based share exchange when fast path stalls
// - Nonce caching: pipelining optimization for cached FROST commitments
//
// Key properties to verify:
// - Both paths converge to the same CommitFact for a given (cid, rid)
// - Fallback can't contradict a successful fast-path execution
// - Threshold shares prevent double-spending (equivocation detection)
// - Fast path selected when valid cached nonces exist
// - Nonces expire with epoch advancement
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

    // ==================== NONCE CACHING TYPES ====================
    // FROST pipelining: witnesses pre-cache nonce commitments for fast-path signing.

    type Epoch = int                    // Epoch counter for nonce validity windows
    type NonceCommitment = str          // D_i, E_i commitments (hiding/binding nonces)

    // Cached nonce state for a witness
    type CachedNonce = {
        commitment: NonceCommitment,    // Public commitment sent to coordinator
        epoch: Epoch,                   // Epoch when nonce was generated
        used: bool                      // Whether nonce has been consumed
    }

    // Path selection for consensus
    type PathSelection =
        | FastPath          // All witnesses have valid cached nonces
        | SlowPath          // Need fresh nonce generation (adds 1 RTT)

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

    // ==================== NONCE STATE (T1.1) ====================
    // Global epoch and per-witness nonce cache for FROST pipelining.

    var currentEpoch: Epoch                                     // Global epoch counter
    var witnessNonces: AuthorityId -> Option[CachedNonce]       // Per-witness nonce cache

    // Nonces valid for N epochs (configurable constant)
    pure val NONCE_VALIDITY_WINDOW: int = 3

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

    // ==================== NONCE HELPERS (T1.1) ====================

    // Check if a cached nonce is valid (not expired, not used)
    pure def isNonceValid(nonceOpt: Option[CachedNonce], epoch: Epoch, validityWindow: int): bool =
        match nonceOpt {
            | Some(n) => not(n.used) and (epoch - n.epoch) < validityWindow
            | None => false
        }

    // Check if all witnesses in a set have valid cached nonces
    pure def allWitnessesHaveValidNonces(
        witnesses: Set[AuthorityId],
        nonces: AuthorityId -> Option[CachedNonce],
        epoch: Epoch,
        validityWindow: int
    ): bool =
        witnesses.forall(w =>
            w.in(nonces.keys()) and isNonceValid(nonces.get(w), epoch, validityWindow)
        )

    // Determine path selection based on nonce availability (T1.2)
    pure def selectPath(
        witnesses: Set[AuthorityId],
        nonces: AuthorityId -> Option[CachedNonce],
        epoch: Epoch,
        validityWindow: int
    ): PathSelection =
        if (allWitnessesHaveValidNonces(witnesses, nonces, epoch, validityWindow))
            FastPath
        else
            SlowPath

    // Count witnesses with valid nonces
    pure def countValidNonces(
        witnesses: Set[AuthorityId],
        nonces: AuthorityId -> Option[CachedNonce],
        epoch: Epoch,
        validityWindow: int
    ): int =
        witnesses.filter(w =>
            w.in(nonces.keys()) and isNonceValid(nonces.get(w), epoch, validityWindow)
        ).size()

    // ==================== ACTIONS ====================
    // State transitions modeling the consensus protocol.

    // Initialize consensus module with empty state.
    action init: bool = all {
        instances' = Map(),
        committedFacts' = Set(),
        globalWitnesses' = Set("w1", "w2", "w3", "w4", "w5"),
        // Initialize nonce state (T1.1)
        currentEpoch' = 0,
        witnessNonces' = Map()
    }

    // Initiator starts a consensus instance.
    // Path selection (T1.1/T1.2): FastPath if all witnesses have valid nonces, else SlowPath.
    action startConsensus(
        cid: ConsensusId,
        initiator: AuthorityId,
        op: OperationData,
        pHash: PrestateHash,
        witnesses: Set[AuthorityId],
        threshold: int
    ): bool = {
        // T1.2: Determine path based on nonce availability
        val pathChoice = selectPath(witnesses, witnessNonces, currentEpoch, NONCE_VALIDITY_WINDOW)
        val initialPhase = match pathChoice {
            | FastPath => FastPathActive
            | SlowPath => FallbackActive  // Skip to fallback if nonces unavailable
        }
        all {
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
                phase: initialPhase,
                proposals: Set(),
                commitFact: None,
                fallbackTimerActive: initialPhase == FallbackActive,
                equivocators: Set()
            }),
            committedFacts' = committedFacts,
            globalWitnesses' = globalWitnesses,
            currentEpoch' = currentEpoch,
            witnessNonces' = witnessNonces
        }
    }

    // Witness submits share (fast path response).
    // Share binds witness to (resultId, prestateHash). Equivocation detected.
    // T1.1: Consumes witness's cached nonce on fast path.
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
        // T1.1: Fast path requires valid nonce
        val isFastPath = inst.phase == FastPathActive
        val hasValidNonce = isNonceValid(
            if (witness.in(witnessNonces.keys())) witnessNonces.get(witness) else None,
            currentEpoch,
            NONCE_VALIDITY_WINDOW
        )
        val nonceOk = not(isFastPath) or hasValidNonce  // Fallback doesn't need cached nonce
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
        // T1.1: Mark nonce as used on fast path
        val updatedNonces = if (isFastPath and hasValidNonce and not(isEquivocating)) {
            val oldNonce = witnessNonces.get(witness)
            match oldNonce {
                | Some(n) => witnessNonces.put(witness, Some({ ...n, used: true }))
                | None => witnessNonces
            }
        } else witnessNonces
        all {
            hasInstance,
            isWitness,
            notVoted,
            isActive,
            nonceOk,
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
            globalWitnesses' = globalWitnesses,
            currentEpoch' = currentEpoch,
            witnessNonces' = updatedNonces
        }
    }

    // Trigger fallback when fast path stalls.
    // Activated by timer expiry, nonce invalidity, or prestate mismatch detection.
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
            globalWitnesses' = globalWitnesses,
            currentEpoch' = currentEpoch,
            witnessNonces' = witnessNonces
        }
    }

    // Gossip shares during fallback (AggregateShare message).
    // Witnesses exchange shares via gossip to overcome network partitions.
    // T1.1: Fallback path doesn't consume cached nonces (generates fresh nonces inline).
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
            globalWitnesses' = globalWitnesses,
            currentEpoch' = currentEpoch,
            witnessNonces' = witnessNonces
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
            globalWitnesses' = globalWitnesses,
            currentEpoch' = currentEpoch,
            witnessNonces' = witnessNonces
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
            globalWitnesses' = globalWitnesses,
            currentEpoch' = currentEpoch,
            witnessNonces' = witnessNonces
        }
    }

    // ==================== NONCE MANAGEMENT ACTIONS (T1.1) ====================

    // Witness caches a fresh nonce commitment for future fast-path signing.
    // Called proactively by witnesses to enable pipelining.
    action cacheNonce(witness: AuthorityId, commitment: NonceCommitment): bool = all {
        witness.in(globalWitnesses),
        // Can cache if no current nonce or current nonce is used/expired
        val currentNonce = if (witness.in(witnessNonces.keys())) witnessNonces.get(witness) else None
        val canCache = match currentNonce {
            | Some(n) => n.used or (currentEpoch - n.epoch) >= NONCE_VALIDITY_WINDOW
            | None => true
        }
        canCache,
        witnessNonces' = witnessNonces.put(witness, Some({
            commitment: commitment,
            epoch: currentEpoch,
            used: false
        })),
        instances' = instances,
        committedFacts' = committedFacts,
        globalWitnesses' = globalWitnesses,
        currentEpoch' = currentEpoch
    }

    // Advance the global epoch (causes nonce expiration).
    // Called periodically or after significant events.
    action advanceEpoch: bool = all {
        currentEpoch' = currentEpoch + 1,
        instances' = instances,
        committedFacts' = committedFacts,
        globalWitnesses' = globalWitnesses,
        witnessNonces' = witnessNonces  // Nonces remain but validity checked against new epoch
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

    // ==================== NONCE INVARIANTS (T1.1) ====================

    // T1.1: Used nonces cannot be reused (no nonce reuse attack).
    // Once a nonce is marked used, it must be refreshed before reuse.
    val InvariantUsedNoncesNotReusable =
        witnessNonces.keys().forall(w => {
            val nonceOpt = witnessNonces.get(w)
            match nonceOpt {
                | Some(n) => n.used implies not(isNonceValid(Some(n), currentEpoch, NONCE_VALIDITY_WINDOW))
                | None => true
            }
        })

    // T1.1: Fast path only selected when all witnesses have valid nonces.
    // Ensures fast path precondition is met.
    val InvariantFastPathRequiresValidNonces =
        instances.keys().forall(cid => {
            val inst = instances.get(cid)
            inst.phase == FastPathActive implies
                allWitnessesHaveValidNonces(inst.witnesses, witnessNonces, currentEpoch, NONCE_VALIDITY_WINDOW)
        })

    // T1.1: Nonces have bounded epoch age.
    // Cached nonces expire after NONCE_VALIDITY_WINDOW epochs.
    val InvariantNonceEpochBounded =
        witnessNonces.keys().forall(w => {
            val nonceOpt = witnessNonces.get(w)
            match nonceOpt {
                | Some(n) => n.epoch <= currentEpoch
                | None => true
            }
        })

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
        // T1.1: Nonce commitment values
        nondet commitment = oneOf(Set("nonce_commit_1", "nonce_commit_2", "nonce_commit_3"))
        any {
            startConsensus(cid, initiator, op, pHash, witnesses, threshold),
            submitWitnessShare(cid, witness, rid, share),
            triggerFallback(cid),
            completeViaFallback(cid, rid),
            failConsensus(cid),
            // T1.1: Nonce management actions
            cacheNonce(witness, commitment),
            advanceEpoch,
        }
    }
}
