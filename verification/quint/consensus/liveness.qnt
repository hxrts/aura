// Protocol Consensus Liveness - Aura Consensus Termination and Progress Properties
//
// This module extends protocol_consensus with liveness analysis:
// - Partial synchrony model (GST-based)
// - Fast path termination properties
// - Slow path (fallback) termination properties
// - Failure mode characterization
// - Timeout and retry logic
//
// The module imports helpers from:
// - protocol_liveness_timing: Time types, synchrony model
// - protocol_liveness_connectivity: Gossip graph, partition modeling
// - protocol_liveness_properties: Invariants, temporal properties
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
// - Timeout handling: crates/aura-consensus/src/consensus/coordinator.rs
// - Fallback logic: crates/aura-consensus/src/consensus/fallback.rs
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
    import protocol_consensus as core from "./core"
    import protocol_liveness_timing as timing from "../liveness/timing"
    import protocol_liveness_connectivity as conn from "../liveness/connectivity"
    import protocol_liveness_properties as props from "../liveness/properties"

    // ════════════════════════════════════════════════════════════════════════
    // STATE VARIABLES
    // ════════════════════════════════════════════════════════════════════════

    var globalTime: timing::Time                        // Current logical time
    var gst: timing::Time                               // Global Stabilization Time
    var gstReached: bool                                // Whether GST has occurred

    var instanceTiming: core::ConsensusId -> timing::InstanceTimingState

    var messageQueue: Set[timing::PendingMessage]

    var witnessParticipation: core::AuthorityId -> timing::WitnessParticipation

    var byzantineCount: int                             // Count of Byzantine witnesses

    // Gossip graph: adjacency list representation
    var gossipGraph: core::AuthorityId -> Set[core::AuthorityId]

    // Note: globalEvidenceTracker is owned by core module, not liveness

    // ════════════════════════════════════════════════════════════════════════
    // INITIALIZATION
    // ════════════════════════════════════════════════════════════════════════

    // Witnesses must match core::init
    pure val INIT_WITNESSES = Set("w1", "w2", "w3", "w4", "w5")

    action initLiveness: bool = all {
        core::init,
        globalTime' = 0,
        gst' = 10,                              // GST occurs at time 10 (arbitrary)
        gstReached' = false,
        instanceTiming' = Map(),
        messageQueue' = Set(),
        witnessParticipation' = INIT_WITNESSES.fold(Map(), (acc, w) =>
            acc.put(w, {
                witness: w,
                isHonest: true,
                isOnline: true,
                lastSeen: 0,
                sharesSent: 0
            })
        ),
        byzantineCount' = 0,
        gossipGraph' = conn::fullyConnectedGraph(INIT_WITNESSES)
    }

    // ════════════════════════════════════════════════════════════════════════
    // TIME ADVANCEMENT ACTIONS
    // ════════════════════════════════════════════════════════════════════════

    // Advance global time (triggers message delivery, timeouts)
    action advanceTime: bool = {
        val deliverableMessages = messageQueue.filter(m => m.deliveryTime <= globalTime + 1)
        val affectedCids = deliverableMessages.map(m => m.cid)
        all {
            globalTime' = globalTime + 1,
            gstReached' = globalTime + 1 >= gst,
            gst' = gst,
            messageQueue' = messageQueue.exclude(deliverableMessages),
            instanceTiming' = instanceTiming.keys().fold(instanceTiming, (acc, cid) =>
                if (cid.in(affectedCids))
                    acc.put(cid, { ...acc.get(cid), lastActivity: globalTime + 1 })
                else
                    acc
            ),
            witnessParticipation' = witnessParticipation,
            byzantineCount' = byzantineCount,
            gossipGraph' = gossipGraph,
            core::instances' = core::instances,
            core::committedFacts' = core::committedFacts,
            core::globalWitnesses' = core::globalWitnesses,
            core::currentEpoch' = core::currentEpoch,
            core::witnessNonces' = core::witnessNonces,
            core::globalEvidenceTracker' = core::globalEvidenceTracker
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
        gossipGraph' = gossipGraph,
        core::instances' = core::instances,
        core::committedFacts' = core::committedFacts,
        core::globalWitnesses' = core::globalWitnesses,
        core::currentEpoch' = core::currentEpoch,
        core::witnessNonces' = core::witnessNonces,
        core::globalEvidenceTracker' = core::globalEvidenceTracker
    }

    // ════════════════════════════════════════════════════════════════════════
    // WITNESS BEHAVIOR ACTIONS
    // ════════════════════════════════════════════════════════════════════════

    // Witness goes offline
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
        gossipGraph' = gossipGraph,
        core::instances' = core::instances,
        core::committedFacts' = core::committedFacts,
        core::globalWitnesses' = core::globalWitnesses,
        core::currentEpoch' = core::currentEpoch,
        core::witnessNonces' = core::witnessNonces,
        core::globalEvidenceTracker' = core::globalEvidenceTracker
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
        gossipGraph' = gossipGraph,
        core::instances' = core::instances,
        core::committedFacts' = core::committedFacts,
        core::globalWitnesses' = core::globalWitnesses,
        core::currentEpoch' = core::currentEpoch,
        core::witnessNonces' = core::witnessNonces,
        core::globalEvidenceTracker' = core::globalEvidenceTracker
    }

    // Witness becomes Byzantine (bounded by < threshold)
    action witnessBecomesByzantine(w: core::AuthorityId): bool = {
        val minThreshold = if (core::instances.keys().size() == 0) 3
            else core::instances.keys().fold(100, (min, cid) =>
                if (core::instances.get(cid).threshold < min) core::instances.get(cid).threshold else min)
        all {
            w.in(witnessParticipation.keys()),
            witnessParticipation.get(w).isHonest,
            byzantineCount + 1 < minThreshold,
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
            gossipGraph' = gossipGraph,
            core::instances' = core::instances,
            core::committedFacts' = core::committedFacts,
            core::globalWitnesses' = core::globalWitnesses,
            core::currentEpoch' = core::currentEpoch,
            core::witnessNonces' = core::witnessNonces,
            core::globalEvidenceTracker' = core::globalEvidenceTracker
        }
    }

    // ════════════════════════════════════════════════════════════════════════
    // CONSENSUS WITH TIMING ACTIONS
    // ════════════════════════════════════════════════════════════════════════

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
        byzantineCount' = byzantineCount,
        gossipGraph' = gossipGraph
    }

    // Submit share with timing update
    action submitShareWithTiming(
        cid: core::ConsensusId,
        witness: core::AuthorityId,
        rid: core::ResultId,
        share: core::ShareData
    ): bool = all {
        core::submitWitnessShare(cid, witness, rid, share, globalTime),
        cid.in(instanceTiming.keys()),
        instanceTiming' = instanceTiming.put(cid, {
            ...instanceTiming.get(cid),
            lastActivity: globalTime
        }),
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
        byzantineCount' = byzantineCount,
        gossipGraph' = gossipGraph
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
        byzantineCount' = byzantineCount,
        gossipGraph' = gossipGraph
    }

    // Timeout-triggered fallback
    action timeoutTriggerFallback(cid: core::ConsensusId): bool = {
        val inst = if (cid.in(core::instances.keys())) core::instances.get(cid)
                   else { cid: "", operation: "", prestateHash: "", threshold: 0,
                          witnesses: Set(), initiator: "", phase: core::ConsensusPending,
                          proposals: Set(), commitFact: core::None, fallbackTimerActive: false,
                          equivocators: Set(), equivocationProofs: Set() }
        val timingState = if (cid.in(instanceTiming.keys())) instanceTiming.get(cid)
                     else { startTime: 0, lastActivity: 0, fallbackStartTime: core::None,
                            retryCount: 0, messagesInFlight: 0 }
        all {
            cid.in(core::instances.keys()),
            cid.in(instanceTiming.keys()),
            inst.phase == core::FastPathActive,
            timing::hasTimedOut(timingState, globalTime, timing::T_FALLBACK),
            core::triggerFallback(cid),
            instanceTiming' = instanceTiming.put(cid, {
                ...timingState,
                fallbackStartTime: core::Some(globalTime),
                lastActivity: globalTime,
                retryCount: timingState.retryCount + 1
            }),
            globalTime' = globalTime,
            gst' = gst,
            gstReached' = gstReached,
            messageQueue' = messageQueue,
            witnessParticipation' = witnessParticipation,
            byzantineCount' = byzantineCount,
            gossipGraph' = gossipGraph
        }
    }

    // Gossip with timing
    action gossipWithTiming(cid: core::ConsensusId, shareSet: Set[core::ShareProposal]): bool = {
        val timingState = if (cid.in(instanceTiming.keys())) instanceTiming.get(cid)
                     else { startTime: 0, lastActivity: 0, fallbackStartTime: core::None,
                            retryCount: 0, messagesInFlight: 0 }
        all {
            cid.in(instanceTiming.keys()),
            core::gossipShares(cid, shareSet),
            instanceTiming' = instanceTiming.put(cid, {
                ...timingState,
                lastActivity: globalTime
            }),
            globalTime' = globalTime,
            gst' = gst,
            gstReached' = gstReached,
            messageQueue' = messageQueue,
            witnessParticipation' = witnessParticipation,
            byzantineCount' = byzantineCount,
            gossipGraph' = gossipGraph
        }
    }

    // ════════════════════════════════════════════════════════════════════════
    // CONNECTIVITY ACTIONS
    // ════════════════════════════════════════════════════════════════════════

    // Break a link between two nodes
    action breakLink(a: core::AuthorityId, b: core::AuthorityId): bool = all {
        a.in(gossipGraph.keys()),
        b.in(conn::neighbors(gossipGraph, a)),
        gossipGraph' = conn::graphWithoutLink(gossipGraph, a, b),
        globalTime' = globalTime,
        gst' = gst,
        gstReached' = gstReached,
        instanceTiming' = instanceTiming,
        messageQueue' = messageQueue,
        witnessParticipation' = witnessParticipation,
        byzantineCount' = byzantineCount,
        core::instances' = core::instances,
        core::committedFacts' = core::committedFacts,
        core::globalWitnesses' = core::globalWitnesses,
        core::currentEpoch' = core::currentEpoch,
        core::witnessNonces' = core::witnessNonces,
        core::globalEvidenceTracker' = core::globalEvidenceTracker
    }

    // Restore a link between two nodes
    action restoreLink(a: core::AuthorityId, b: core::AuthorityId): bool = all {
        a.in(gossipGraph.keys()),
        not(b.in(conn::neighbors(gossipGraph, a))),
        a != b,
        gossipGraph' = conn::graphWithLink(gossipGraph, a, b),
        globalTime' = globalTime,
        gst' = gst,
        gstReached' = gstReached,
        instanceTiming' = instanceTiming,
        messageQueue' = messageQueue,
        witnessParticipation' = witnessParticipation,
        byzantineCount' = byzantineCount,
        core::instances' = core::instances,
        core::committedFacts' = core::committedFacts,
        core::globalWitnesses' = core::globalWitnesses,
        core::currentEpoch' = core::currentEpoch,
        core::witnessNonces' = core::witnessNonces,
        core::globalEvidenceTracker' = core::globalEvidenceTracker
    }

    // Create a network partition
    action partitionNodes(isolated: Set[core::AuthorityId]): bool = {
        val remaining = gossipGraph.keys().exclude(isolated)
        all {
            isolated.size() > 0,
            remaining.size() > 0,
            gossipGraph' = conn::graphWithPartition(gossipGraph, isolated),
            globalTime' = globalTime,
            gst' = gst,
            gstReached' = gstReached,
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
    }

    // Heal a partition: restore full connectivity
    action healPartition: bool = all {
        gossipGraph' = conn::fullyConnectedGraph(gossipGraph.keys()),
        globalTime' = globalTime,
        gst' = gst,
        gstReached' = gstReached,
        instanceTiming' = instanceTiming,
        messageQueue' = messageQueue,
        witnessParticipation' = witnessParticipation,
        byzantineCount' = byzantineCount,
        core::instances' = core::instances,
        core::committedFacts' = core::committedFacts,
        core::globalWitnesses' = core::globalWitnesses,
        core::currentEpoch' = core::currentEpoch,
        core::witnessNonces' = core::witnessNonces,
        core::globalEvidenceTracker' = core::globalEvidenceTracker
    }

    // ════════════════════════════════════════════════════════════════════════
    // STEP RELATION
    // ════════════════════════════════════════════════════════════════════════

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
                byzantineCount' = byzantineCount,
                gossipGraph' = gossipGraph
            },
            all {
                core::advanceEpoch,
                globalTime' = globalTime,
                gst' = gst,
                gstReached' = gstReached,
                instanceTiming' = instanceTiming,
                messageQueue' = messageQueue,
                witnessParticipation' = witnessParticipation,
                byzantineCount' = byzantineCount,
                gossipGraph' = gossipGraph
            },
            all {
                core::failConsensus(cid),
                instanceTiming' = instanceTiming,
                globalTime' = globalTime,
                gst' = gst,
                gstReached' = gstReached,
                messageQueue' = messageQueue,
                witnessParticipation' = witnessParticipation,
                byzantineCount' = byzantineCount,
                gossipGraph' = gossipGraph
            },
            all {
                core::completeViaFallback(cid, rid),
                instanceTiming' = instanceTiming,
                globalTime' = globalTime,
                gst' = gst,
                gstReached' = gstReached,
                messageQueue' = messageQueue,
                witnessParticipation' = witnessParticipation,
                byzantineCount' = byzantineCount,
                gossipGraph' = gossipGraph
            }
        }
    }

    // ════════════════════════════════════════════════════════════════════════
    // PROPERTIES (bound to state variables)
    //
    // These bind the pure property functions to actual state for model checking.
    // ════════════════════════════════════════════════════════════════════════

    // Termination
    val AllInstancesTerminated = props::allInstancesTerminated(core::instances)

    // Progress invariants
    val InvariantProgressUnderSynchrony = props::invariantProgressUnderSynchrony(
        core::instances, instanceTiming, witnessParticipation, globalTime, gstReached
    )
    val AssumptionByzantineTolerance = props::assumptionByzantineTolerance(
        core::instances, byzantineCount
    )
    val InvariantRetryBound = props::invariantRetryBound(instanceTiming)

    // Fast path timing
    val TemporalFastPathBound = props::temporalFastPathBound(
        core::instances, instanceTiming, core::witnessNonces, core::currentEpoch,
        witnessParticipation, globalTime, gstReached
    )
    val FastPathProgressCheck = props::fastPathProgressCheck(
        core::instances, core::witnessNonces, core::currentEpoch,
        witnessParticipation, gstReached
    )
    val SlowPathProgressCheck = props::slowPathProgressCheck(
        core::instances, witnessParticipation, gstReached
    )
    val ActiveMakesProgress = props::activeMakesProgress(
        core::instances, witnessParticipation, gstReached
    )
    val NoDeadlock = props::noDeadlock(core::instances)

    // Availability properties (T3e.4-T3e.6)
    val InvariantCommitRequiresHonestParticipation =
        props::invariantCommitRequiresHonestParticipation(core::instances, witnessParticipation)
    val InvariantQuorumPossible =
        props::invariantQuorumPossible(core::instances, witnessParticipation)
    val PropertyQuorumSufficient =
        props::propertyQuorumSufficient(core::instances, witnessParticipation, gossipGraph)
    val PropertyPartitionTolerance =
        props::propertyPartitionTolerance(core::instances, witnessParticipation, gossipGraph)
    val AvailabilityGuarantee =
        props::availabilityGuarantee(core::instances, witnessParticipation, gossipGraph)

    // Failure modes
    val FailureModePartition = props::failureModePartition(
        core::instances, witnessParticipation, gstReached
    )
    val FailureModeNonceExhaustion = props::failureModeNonceExhaustion(
        core::instances, core::witnessNonces, core::currentEpoch
    )
    val FailureModeByzantine = props::failureModeByzantine(byzantineCount)
    val InFailureModeButLive = props::inFailureModeButLive(
        core::instances, witnessParticipation, core::witnessNonces,
        core::currentEpoch, byzantineCount, gstReached
    )
}
