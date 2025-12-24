// Protocol Consensus Liveness - Aura Consensus Termination and Progress Properties
//
// This module extends protocol_consensus with:
// - Partial synchrony model (GST-based)
// - Fast path termination properties
// - Slow path (fallback) termination properties
// - Failure mode characterization
// - Timeout and retry logic
//
// == Lean Correspondence ==
// Module: Aura.Consensus.* (verification/lean/Aura/Consensus/)
// - Liveness is not directly proven in Lean (state machine model checking in Quint)
// - Safety invariants that support liveness:
//   - Aura.Consensus.Agreement.agreement (unique commit per instance)
//   - Aura.Consensus.Validity.honest_participation (honest majority can commit)
//   - Aura.Consensus.Frost.aggregation_threshold (threshold requirement)
// - Byzantine threshold: Aura.Assumptions.byzantine_threshold
//
// == Rust Correspondence ==
// - Timeout handling: crates/aura-protocol/src/consensus/coordinator.rs
// - Fallback logic: crates/aura-protocol/src/consensus/fallback.rs
// - Gossip: crates/aura-sync/src/protocols/anti_entropy.rs
//
// See: docs/004_distributed_systems_contract.md §4, docs/104_consensus.md
//
// Key Liveness Properties:
// 1. Fast path terminates in 1 RTT when all witnesses have valid nonces (after GST)
// 2. Slow path terminates eventually via gossip (after GST)
// 3. Byzantine witnesses (< t) cannot prevent termination
// 4. Network partitions (before GST) only delay, not prevent, termination

module protocol_consensus_liveness {
    import protocol_consensus as core from "protocol_consensus"

    // ════════════════════════════════════════════════════════════════════════
    // TYPES: SYNCHRONY MODEL
    // ════════════════════════════════════════════════════════════════════════
    // Partial synchrony with Global Stabilization Time (GST).
    // Before GST: messages may be arbitrarily delayed (but not lost)
    // After GST: all messages delivered within bound Δ

    // Logical time units for model (abstract, not wall-clock)
    type Time = int

    // Time constants (in abstract units)
    pure val DELTA: Time = 3                    // Message delay bound after GST
    pure val T_FALLBACK: Time = DELTA * 2       // Fallback trigger timeout (2-3× Δ)
    pure val GOSSIP_INTERVAL: Time = 1          // Gossip period
    pure val MAX_RETRIES: int = 5               // Maximum retry attempts

    // ════════════════════════════════════════════════════════════════════════
    // STATE VARIABLES
    // ════════════════════════════════════════════════════════════════════════

    var globalTime: Time                        // Current logical time
    var gst: Time                               // Global Stabilization Time (when set)
    var gstReached: bool                        // Whether GST has occurred

    // Per-instance timing state
    type InstanceTimingState = {
        startTime: Time,                        // When instance was started
        lastActivity: Time,                     // Last message/progress time
        fallbackStartTime: core::Option[Time],  // When fallback was triggered
        retryCount: int,                        // Number of retry attempts
        messagesInFlight: int                   // Pending message count (abstract)
    }

    var instanceTiming: core::ConsensusId -> InstanceTimingState

    // Message queue abstraction for synchrony model
    type PendingMessage = {
        cid: core::ConsensusId,
        sender: core::AuthorityId,
        sentTime: Time,
        deliveryTime: Time                      // Earliest delivery (sentTime + delay)
    }

    var messageQueue: Set[PendingMessage]

    // Track honest vs Byzantine behavior for liveness analysis
    type WitnessParticipation = {
        witness: core::AuthorityId,
        isHonest: bool,                         // Follows protocol correctly
        isOnline: bool,                         // Currently reachable
        lastSeen: Time,                         // Last known activity
        sharesSent: int                         // Total shares contributed
    }

    var witnessParticipation: core::AuthorityId -> WitnessParticipation

    // Count of Byzantine witnesses (must be < threshold for liveness)
    var byzantineCount: int

    // ════════════════════════════════════════════════════════════════════════
    // EXPOSE: SEMANTIC INTERFACE
    // ════════════════════════════════════════════════════════════════════════
    // These predicates define liveness MEANING.
    // - Rust conformance tests verify these properties
    // - Lean cannot prove liveness directly (uses model checking)
    //
    // Exposed predicates:
    //   - isSynchronous: whether network is past GST
    //   - canMakeProgress: whether instance can advance toward termination
    //   - isTerminal: whether instance has reached final state
    //   - hasQuorumOnline: whether threshold honest witnesses are available
    // ════════════════════════════════════════════════════════════════════════

    /// Check if network is synchronous (past GST).
    /// Rust: coordinator::is_synchronous
    pure def isSynchronous(time: Time, gstTime: Time, reached: bool): bool =
        reached and time >= gstTime

    // Calculate message delay based on synchrony
    pure def messageDelay(time: Time, gstTime: Time, reached: bool): Time =
        if (isSynchronous(time, gstTime, reached))
            DELTA                               // Bounded after GST
        else
            DELTA * 3                           // Conservative before GST

    // Check if sufficient honest witnesses are online
    pure def hasQuorumOnline(
        witnesses: Set[core::AuthorityId],
        participation: core::AuthorityId -> WitnessParticipation,
        threshold: int
    ): bool = {
        val onlineHonest = witnesses.filter(w =>
            w.in(participation.keys()) and
            participation.get(w).isHonest and
            participation.get(w).isOnline
        )
        onlineHonest.size() >= threshold
    }

    // Count honest online witnesses
    pure def countOnlineHonest(
        witnesses: Set[core::AuthorityId],
        participation: core::AuthorityId -> WitnessParticipation
    ): int =
        witnesses.filter(w =>
            w.in(participation.keys()) and
            participation.get(w).isHonest and
            participation.get(w).isOnline
        ).size()

    // Check if instance has timed out (needs fallback or retry)
    pure def hasTimedOut(
        timing: InstanceTimingState,
        currentTime: Time,
        timeout: Time
    ): bool =
        (currentTime - timing.lastActivity) >= timeout

    // Check if fast path should complete (has threshold valid nonces)
    pure def fastPathCanComplete(
        inst: core::ConsensusInstance,
        nonces: core::AuthorityId -> core::Option[core::CachedNonce],
        epoch: core::Epoch,
        validityWindow: int,
        participation: core::AuthorityId -> WitnessParticipation
    ): bool = {
        val readyWitnesses = inst.witnesses.filter(w =>
            core::isNonceValid(
                if (w.in(nonces.keys())) nonces.get(w) else core::None,
                epoch,
                validityWindow
            ) and
            w.in(participation.keys()) and
            participation.get(w).isOnline and
            participation.get(w).isHonest
        )
        readyWitnesses.size() >= inst.threshold
    }

    // ==================== INIT ACTION ====================

    action initLiveness: bool = all {
        core::init,
        globalTime' = 0,
        gst' = 10,                              // GST occurs at time 10 (arbitrary)
        gstReached' = false,
        instanceTiming' = Map(),
        messageQueue' = Set(),
        witnessParticipation' = core::globalWitnesses.fold(Map(), (acc, w) =>
            acc.put(w, {
                witness: w,
                isHonest: true,                 // Start all honest
                isOnline: true,                 // Start all online
                lastSeen: 0,
                sharesSent: 0
            })
        ),
        byzantineCount' = 0
    }

    // ==================== TIME ADVANCEMENT ====================

    // Advance global time (triggers message delivery, timeouts)
    action advanceTime: bool = {
        val deliverableMessages = messageQueue.filter(m => m.deliveryTime <= globalTime + 1)
        val affectedCids = deliverableMessages.map(m => m.cid)
        all {
            globalTime' = globalTime + 1,
            gstReached' = globalTime + 1 >= gst,
            gst' = gst,
            // Deliver messages that are ready
            messageQueue' = messageQueue.exclude(deliverableMessages),
            // Update last activity for instances with delivered messages
            instanceTiming' = instanceTiming.keys().fold(instanceTiming, (acc, cid) =>
                if (cid.in(affectedCids))
                    acc.put(cid, { ...acc.get(cid), lastActivity: globalTime + 1 })
                else
                    acc
            ),
            witnessParticipation' = witnessParticipation,
            byzantineCount' = byzantineCount,
            core::instances' = core::instances,
            core::committedFacts' = core::committedFacts,
            core::globalWitnesses' = core::globalWitnesses,
            core::currentEpoch' = core::currentEpoch,
            core::witnessNonces' = core::witnessNonces
        }
    }

    // GST event: network stabilizes
    action stabilizeNetwork: bool = all {
        not(gstReached),
        gstReached' = true,
        gst' = globalTime,
        globalTime' = globalTime,
        instanceTiming' = instanceTiming,
        messageQueue' = messageQueue,
        witnessParticipation' = witnessParticipation,
        byzantineCount' = byzantineCount,
        core::instances' = core::instances,
        core::committedFacts' = core::committedFacts,
        core::globalWitnesses' = core::globalWitnesses,
        core::currentEpoch' = core::currentEpoch,
        core::witnessNonces' = core::witnessNonces
    }

    // ==================== WITNESS BEHAVIOR ====================

    // Witness goes offline (network partition, device failure)
    action witnessGoesOffline(w: core::AuthorityId): bool = all {
        w.in(witnessParticipation.keys()),
        witnessParticipation.get(w).isOnline,
        witnessParticipation' = witnessParticipation.put(w, {
            ...witnessParticipation.get(w),
            isOnline: false,
            lastSeen: globalTime
        }),
        globalTime' = globalTime,
        gst' = gst,
        gstReached' = gstReached,
        instanceTiming' = instanceTiming,
        messageQueue' = messageQueue,
        byzantineCount' = byzantineCount,
        core::instances' = core::instances,
        core::committedFacts' = core::committedFacts,
        core::globalWitnesses' = core::globalWitnesses,
        core::currentEpoch' = core::currentEpoch,
        core::witnessNonces' = core::witnessNonces
    }

    // Witness comes back online
    action witnessComesOnline(w: core::AuthorityId): bool = all {
        w.in(witnessParticipation.keys()),
        not(witnessParticipation.get(w).isOnline),
        witnessParticipation' = witnessParticipation.put(w, {
            ...witnessParticipation.get(w),
            isOnline: true,
            lastSeen: globalTime
        }),
        globalTime' = globalTime,
        gst' = gst,
        gstReached' = gstReached,
        instanceTiming' = instanceTiming,
        messageQueue' = messageQueue,
        byzantineCount' = byzantineCount,
        core::instances' = core::instances,
        core::committedFacts' = core::committedFacts,
        core::globalWitnesses' = core::globalWitnesses,
        core::currentEpoch' = core::currentEpoch,
        core::witnessNonces' = core::witnessNonces
    }

    // Witness becomes Byzantine (bounded by < threshold)
    action witnessBecomesByzantine(w: core::AuthorityId): bool = all {
        w.in(witnessParticipation.keys()),
        witnessParticipation.get(w).isHonest,
        // Byzantine count must stay below threshold for liveness
        byzantineCount < 2,                     // For 2-of-3 threshold
        witnessParticipation' = witnessParticipation.put(w, {
            ...witnessParticipation.get(w),
            isHonest: false
        }),
        byzantineCount' = byzantineCount + 1,
        globalTime' = globalTime,
        gst' = gst,
        gstReached' = gstReached,
        instanceTiming' = instanceTiming,
        messageQueue' = messageQueue,
        core::instances' = core::instances,
        core::committedFacts' = core::committedFacts,
        core::globalWitnesses' = core::globalWitnesses,
        core::currentEpoch' = core::currentEpoch,
        core::witnessNonces' = core::witnessNonces
    }

    // ==================== CONSENSUS WITH TIMING ====================

    // Start consensus with timing tracking
    action startConsensusWithTiming(
        cid: core::ConsensusId,
        initiator: core::AuthorityId,
        op: core::OperationData,
        pHash: core::PrestateHash,
        witnesses: Set[core::AuthorityId],
        threshold: int
    ): bool = all {
        core::startConsensus(cid, initiator, op, pHash, witnesses, threshold),
        instanceTiming' = instanceTiming.put(cid, {
            startTime: globalTime,
            lastActivity: globalTime,
            fallbackStartTime: core::None,
            retryCount: 0,
            messagesInFlight: witnesses.size()
        }),
        globalTime' = globalTime,
        gst' = gst,
        gstReached' = gstReached,
        messageQueue' = messageQueue,
        witnessParticipation' = witnessParticipation,
        byzantineCount' = byzantineCount
    }

    // Submit share with timing update
    action submitShareWithTiming(
        cid: core::ConsensusId,
        witness: core::AuthorityId,
        rid: core::ResultId,
        share: core::ShareData
    ): bool = all {
        core::submitWitnessShare(cid, witness, rid, share),
        // Update timing
        cid.in(instanceTiming.keys()),
        instanceTiming' = instanceTiming.put(cid, {
            ...instanceTiming.get(cid),
            lastActivity: globalTime
        }),
        // Update participation stats
        witnessParticipation' = if (witness.in(witnessParticipation.keys()))
            witnessParticipation.put(witness, {
                ...witnessParticipation.get(witness),
                lastSeen: globalTime,
                sharesSent: witnessParticipation.get(witness).sharesSent + 1
            })
        else witnessParticipation,
        globalTime' = globalTime,
        gst' = gst,
        gstReached' = gstReached,
        messageQueue' = messageQueue,
        byzantineCount' = byzantineCount
    }

    // Trigger fallback with timing
    action triggerFallbackWithTiming(cid: core::ConsensusId): bool = all {
        core::triggerFallback(cid),
        cid.in(instanceTiming.keys()),
        instanceTiming' = instanceTiming.put(cid, {
            ...instanceTiming.get(cid),
            fallbackStartTime: core::Some(globalTime),
            lastActivity: globalTime
        }),
        globalTime' = globalTime,
        gst' = gst,
        gstReached' = gstReached,
        messageQueue' = messageQueue,
        witnessParticipation' = witnessParticipation,
        byzantineCount' = byzantineCount
    }

    // Timeout-triggered fallback
    action timeoutTriggerFallback(cid: core::ConsensusId): bool = {
        val inst = if (cid.in(core::instances.keys())) core::instances.get(cid)
                   else { cid: "", operation: "", prestateHash: "", threshold: 0,
                          witnesses: Set(), initiator: "", phase: core::ConsensusPending,
                          proposals: Set(), commitFact: core::None, fallbackTimerActive: false,
                          equivocators: Set() }
        val timing = if (cid.in(instanceTiming.keys())) instanceTiming.get(cid)
                     else { startTime: 0, lastActivity: 0, fallbackStartTime: core::None,
                            retryCount: 0, messagesInFlight: 0 }
        all {
            cid.in(core::instances.keys()),
            cid.in(instanceTiming.keys()),
            inst.phase == core::FastPathActive,
            hasTimedOut(timing, globalTime, T_FALLBACK),
            core::triggerFallback(cid),
            instanceTiming' = instanceTiming.put(cid, {
                ...timing,
                fallbackStartTime: core::Some(globalTime),
                lastActivity: globalTime,
                retryCount: timing.retryCount + 1
            }),
            globalTime' = globalTime,
            gst' = gst,
            gstReached' = gstReached,
            messageQueue' = messageQueue,
            witnessParticipation' = witnessParticipation,
            byzantineCount' = byzantineCount
        }
    }

    // Gossip with timing (periodic action during fallback)
    action gossipWithTiming(cid: core::ConsensusId, shareSet: Set[core::ShareProposal]): bool = {
        val timing = if (cid.in(instanceTiming.keys())) instanceTiming.get(cid)
                     else { startTime: 0, lastActivity: 0, fallbackStartTime: core::None,
                            retryCount: 0, messagesInFlight: 0 }
        all {
            cid.in(instanceTiming.keys()),
            // Gossip only during fallback
            core::gossipShares(cid, shareSet),
            instanceTiming' = instanceTiming.put(cid, {
                ...timing,
                lastActivity: globalTime
            }),
            globalTime' = globalTime,
            gst' = gst,
            gstReached' = gstReached,
            messageQueue' = messageQueue,
            witnessParticipation' = witnessParticipation,
            byzantineCount' = byzantineCount
        }
    }

    // ==================== TERMINATION PREDICATES ====================

    // Instance has terminated (committed or failed)
    pure def isTerminated(inst: core::ConsensusInstance): bool =
        inst.phase == core::ConsensusCommitted or inst.phase == core::ConsensusFailed

    // All instances have terminated
    val AllInstancesTerminated =
        core::instances.keys().forall(cid => isTerminated(core::instances.get(cid)))

    // ==================== LIVENESS INVARIANTS ====================
    // These properties ensure consensus eventually terminates.
    // Note: Liveness proven via model checking, not Lean theorems.

    // Progress: Under synchrony with honest quorum, active instances make progress
    // Lean support: Aura.Consensus.Validity.honest_participation
    val InvariantProgressUnderSynchrony =
        gstReached implies core::instances.keys().forall(cid => {
            val inst = core::instances.get(cid)
            val hasQuorum = hasQuorumOnline(inst.witnesses, witnessParticipation, inst.threshold)
            val isActive = inst.phase == core::FastPathActive or inst.phase == core::FallbackActive
            // If active with quorum, either terminated or has recent activity
            not(isActive and hasQuorum) or (
                cid.in(instanceTiming.keys()) implies
                    (globalTime - instanceTiming.get(cid).lastActivity) < T_FALLBACK * 2
            )
        })

    // Byzantine tolerance: < threshold Byzantine witnesses cannot prevent termination
    // Lean: Aura.Assumptions.byzantine_threshold
    val InvariantByzantineTolerance =
        core::instances.keys().forall(cid => {
            val inst = core::instances.get(cid)
            byzantineCount < inst.threshold
        })

    // Retry bound: Instances don't retry indefinitely
    val InvariantRetryBound =
        instanceTiming.keys().forall(cid =>
            instanceTiming.get(cid).retryCount <= MAX_RETRIES
        )

    // ==================== TEMPORAL LIVENESS PROPERTIES ====================
    // Note: Quint temporal properties need careful structuring to avoid cyclic dependencies.
    // We define helper predicates to simplify the temporal logic.

    // Helper: Check if instance is ready for fast path completion
    pure def canCompleteFastPath(
        inst: core::ConsensusInstance,
        nonces: core::AuthorityId -> core::Option[core::CachedNonce],
        epoch: core::Epoch,
        validityWindow: int,
        participation: core::AuthorityId -> WitnessParticipation,
        synch: bool
    ): bool =
        inst.phase == core::FastPathActive and
        synch and
        fastPathCanComplete(inst, nonces, epoch, validityWindow, participation)

    // Helper: Check if instance is in fallback with quorum
    pure def inFallbackWithQuorum(
        inst: core::ConsensusInstance,
        participation: core::AuthorityId -> WitnessParticipation,
        synch: bool
    ): bool =
        inst.phase == core::FallbackActive and
        synch and
        hasQuorumOnline(inst.witnesses, participation, inst.threshold)

    // Helper: Check if instance is active with quorum
    pure def isActiveWithQuorum(
        inst: core::ConsensusInstance,
        participation: core::AuthorityId -> WitnessParticipation,
        synch: bool
    ): bool = {
        val isActive = inst.phase == core::FastPathActive or inst.phase == core::FallbackActive
        isActive and synch and hasQuorumOnline(inst.witnesses, participation, inst.threshold)
    }

    // Temporal: Fast path leads to commit (stated as invariant approximation)
    // Full temporal property: eventually(phase == Committed) given preconditions
    // Approximation: If conditions met and time passes, phase should not stay stuck
    val FastPathProgressCheck =
        core::instances.keys().forall(cid => {
            val inst = core::instances.get(cid)
            canCompleteFastPath(inst, core::witnessNonces, core::currentEpoch,
                               core::NONCE_VALIDITY_WINDOW, witnessParticipation, gstReached)
            implies (inst.proposals.size() > 0 or inst.phase == core::ConsensusCommitted)
        })

    // Temporal: Slow path eventually terminates (stated as invariant approximation)
    val SlowPathProgressCheck =
        core::instances.keys().forall(cid => {
            val inst = core::instances.get(cid)
            inFallbackWithQuorum(inst, witnessParticipation, gstReached)
            implies (inst.proposals.size() > 0 or isTerminated(inst))
        })

    // Temporal: Active instances make progress (stated as invariant approximation)
    val ActiveMakesProgress =
        core::instances.keys().forall(cid => {
            val inst = core::instances.get(cid)
            isActiveWithQuorum(inst, witnessParticipation, gstReached)
            implies (inst.fallbackTimerActive or inst.proposals.size() > 0 or isTerminated(inst))
        })

    // No Deadlock: There's always an enabled action for active instances
    val NoDeadlock =
        core::instances.keys().forall(cid => {
            val inst = core::instances.get(cid)
            val isActive = inst.phase == core::FastPathActive or inst.phase == core::FallbackActive
            // Active instances can always make progress (via timeout, gossip, or share submission)
            not(isActive) or (
                // Can submit share, gossip, trigger fallback, or complete
                inst.witnesses.exists(w => not(core::hasProposal(inst.proposals, w))) or
                inst.fallbackTimerActive or
                inst.proposals.size() >= inst.threshold
            )
        })

    // ==================== FAILURE MODE CHARACTERIZATION ====================

    // Failure modes that can delay (but not prevent) liveness

    // FM1: Network partition (before GST)
    val FailureModePartition =
        not(gstReached) and
        core::instances.keys().exists(cid => {
            val inst = core::instances.get(cid)
            val isActive = inst.phase == core::FastPathActive or inst.phase == core::FallbackActive
            val onlineCount = countOnlineHonest(inst.witnesses, witnessParticipation)
            isActive and onlineCount < inst.threshold
        })

    // FM2: Nonce exhaustion (triggers slow path)
    val FailureModeNonceExhaustion =
        core::instances.keys().exists(cid => {
            val inst = core::instances.get(cid)
            inst.phase == core::FastPathActive and
            not(core::allWitnessesHaveValidNonces(
                inst.witnesses, core::witnessNonces, core::currentEpoch, core::NONCE_VALIDITY_WINDOW
            ))
        })

    // FM3: Initiator failure (requires fallback)
    // Modeled implicitly: when fast path stalls, fallback triggers

    // FM4: Byzantine behavior (< threshold)
    val FailureModeByzantine =
        byzantineCount > 0 and byzantineCount < 2  // Non-zero but tolerable

    // System is in a failure mode but still live
    val InFailureModeButLive =
        (FailureModePartition or FailureModeNonceExhaustion or FailureModeByzantine) and
        gstReached implies core::instances.keys().forall(cid => {
            val inst = core::instances.get(cid)
            val hasQuorum = hasQuorumOnline(inst.witnesses, witnessParticipation, inst.threshold)
            not(hasQuorum) or inst.phase != core::ConsensusFailed
        })

    // ==================== STEP RELATION ====================

    action step = any {
        advanceTime,
        stabilizeNetwork,
        // Witness behavior
        nondet w = oneOf(core::globalWitnesses)
        any {
            witnessGoesOffline(w),
            witnessComesOnline(w),
            witnessBecomesByzantine(w),
        },
        // Consensus with timing
        nondet cid = oneOf(Set("cns1", "cns2", "cns3"))
        nondet initiator = oneOf(Set("w1", "w2"))
        nondet op = oneOf(Set("update_policy", "add_device"))
        nondet pHash = oneOf(Set("pre_abc", "pre_xyz"))
        nondet witnesses = oneOf(Set(Set("w1", "w2", "w3"), Set("w2", "w3", "w4")))
        nondet threshold = oneOf(Set(2, 3))
        nondet witness = oneOf(Set("w1", "w2", "w3", "w4"))
        nondet rid = oneOf(Set("rid_op1", "rid_op2"))
        // ShareData is now a structured record
        nondet shareValue = oneOf(Set("share_a", "share_b", "share_c"))
        nondet nonceBinding = oneOf(Set("nonce_commit_1", "nonce_commit_2", "nonce_commit_3"))
        val dataBinding = core::computeDataBinding(cid, rid, pHash)
        val share: core::ShareData = { shareValue: shareValue, nonceBinding: nonceBinding, dataBinding: dataBinding }
        any {
            startConsensusWithTiming(cid, initiator, op, pHash, witnesses, threshold),
            submitShareWithTiming(cid, witness, rid, share),
            triggerFallbackWithTiming(cid),
            timeoutTriggerFallback(cid),
            // Nonce management
            nondet commitment = oneOf(Set("nc1", "nc2", "nc3"))
            all {
                core::cacheNonce(witness, commitment),
                globalTime' = globalTime,
                gst' = gst,
                gstReached' = gstReached,
                instanceTiming' = instanceTiming,
                messageQueue' = messageQueue,
                witnessParticipation' = witnessParticipation,
                byzantineCount' = byzantineCount
            },
            all {
                core::advanceEpoch,
                globalTime' = globalTime,
                gst' = gst,
                gstReached' = gstReached,
                instanceTiming' = instanceTiming,
                messageQueue' = messageQueue,
                witnessParticipation' = witnessParticipation,
                byzantineCount' = byzantineCount
            },
            all {
                core::failConsensus(cid),
                instanceTiming' = instanceTiming,
                globalTime' = globalTime,
                gst' = gst,
                gstReached' = gstReached,
                messageQueue' = messageQueue,
                witnessParticipation' = witnessParticipation,
                byzantineCount' = byzantineCount
            },
            all {
                core::completeViaFallback(cid, rid),
                instanceTiming' = instanceTiming,
                globalTime' = globalTime,
                gst' = gst,
                gstReached' = gstReached,
                messageQueue' = messageQueue,
                witnessParticipation' = witnessParticipation,
                byzantineCount' = byzantineCount
            }
        }
    }
}
