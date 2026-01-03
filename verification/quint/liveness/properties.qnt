// Protocol Liveness Properties - Invariants, Temporal Properties, Failure Modes
//
// This module defines all liveness-related properties for consensus:
// - Termination predicates
// - Progress invariants
// - Fast path timing bound
// - Availability properties (T3e.4-T3e.6)
// - Failure mode characterization
//
// NOTE: This module defines PURE property predicates. State variables are passed
// as parameters to enable model checking from the main liveness module.
//
// See: docs/004_distributed_systems_contract.md §4, docs/104_consensus.md

module protocol_liveness_properties {
    import protocol_consensus as core from "../consensus/core"
    import protocol_liveness_timing as timing from "./timing"
    import protocol_liveness_connectivity as conn from "./connectivity"

    // ════════════════════════════════════════════════════════════════════════
    // TERMINATION PREDICATES
    // ════════════════════════════════════════════════════════════════════════

    // Instance has terminated (committed or failed)
    pure def isTerminated(inst: core::ConsensusInstance): bool =
        inst.phase == core::ConsensusCommitted or inst.phase == core::ConsensusFailed

    // Check if all instances in a map have terminated
    pure def allInstancesTerminated(
        instances: core::ConsensusId -> core::ConsensusInstance
    ): bool =
        instances.keys().forall(cid => isTerminated(instances.get(cid)))

    // ════════════════════════════════════════════════════════════════════════
    // PROGRESS HELPER PREDICATES
    // ════════════════════════════════════════════════════════════════════════

    // Check if instance can complete fast path
    pure def canCompleteFastPath(
        inst: core::ConsensusInstance,
        nonces: core::AuthorityId -> core::Option[core::CachedNonce],
        epoch: core::Epoch,
        validityWindow: int,
        participation: core::AuthorityId -> timing::WitnessParticipation,
        synch: bool
    ): bool =
        inst.phase == core::FastPathActive and
        synch and
        timing::fastPathCanComplete(inst, nonces, epoch, validityWindow, participation)

    // Check if instance is in fallback with quorum
    pure def inFallbackWithQuorum(
        inst: core::ConsensusInstance,
        participation: core::AuthorityId -> timing::WitnessParticipation,
        synch: bool
    ): bool =
        inst.phase == core::FallbackActive and
        synch and
        timing::hasQuorumOnline(inst.witnesses, participation, inst.threshold)

    // Check if instance is active with quorum
    pure def isActiveWithQuorum(
        inst: core::ConsensusInstance,
        participation: core::AuthorityId -> timing::WitnessParticipation,
        synch: bool
    ): bool = {
        val isActive = inst.phase == core::FastPathActive or inst.phase == core::FallbackActive
        isActive and synch and timing::hasQuorumOnline(inst.witnesses, participation, inst.threshold)
    }

    // ════════════════════════════════════════════════════════════════════════
    // PROGRESS INVARIANTS
    //
    // These properties ensure consensus makes progress under appropriate conditions.
    // ════════════════════════════════════════════════════════════════════════

    // Progress: Under synchrony with honest quorum, active instances make progress
    // Lean support: Aura.Consensus.Validity.honest_participation
    pure def invariantProgressUnderSynchrony(
        instances: core::ConsensusId -> core::ConsensusInstance,
        instanceTiming: core::ConsensusId -> timing::InstanceTimingState,
        participation: core::AuthorityId -> timing::WitnessParticipation,
        globalTime: timing::Time,
        gstReached: bool
    ): bool =
        gstReached implies instances.keys().forall(cid => {
            val inst = instances.get(cid)
            val hasQuorum = timing::hasQuorumOnline(inst.witnesses, participation, inst.threshold)
            val isActive = inst.phase == core::FastPathActive or inst.phase == core::FallbackActive
            // If active with quorum, either terminated or has recent activity
            not(isActive and hasQuorum) or (
                cid.in(instanceTiming.keys()) implies
                    (globalTime - instanceTiming.get(cid).lastActivity) < timing::T_FALLBACK * 2
            )
        })

    // Byzantine tolerance assumption: < threshold Byzantine witnesses.
    // This is an ASSUMPTION (corresponding to Lean axiom Aura.Assumptions.byzantine_threshold),
    // not a safety invariant to model-check.
    pure def assumptionByzantineTolerance(
        instances: core::ConsensusId -> core::ConsensusInstance,
        byzantineCount: int
    ): bool =
        instances.keys().forall(cid => {
            val inst = instances.get(cid)
            byzantineCount < inst.threshold
        })

    // Retry bound: Instances don't retry indefinitely
    pure def invariantRetryBound(
        instanceTiming: core::ConsensusId -> timing::InstanceTimingState
    ): bool =
        instanceTiming.keys().forall(cid =>
            instanceTiming.get(cid).retryCount <= timing::MAX_RETRIES
        )

    // ════════════════════════════════════════════════════════════════════════
    // FAST PATH TIMING BOUND
    //
    // The fast path is "optimistically responsive": it completes within 2δ
    // when all witnesses are online, honest, and have valid nonces.
    //
    // See: https://decentralizedthoughts.github.io/2020-06-12-optimal-optimistic-responsiveness/
    // ════════════════════════════════════════════════════════════════════════

    // Fast path timing bound: commits within FAST_PATH_BOUND when responsive
    pure def temporalFastPathBound(
        instances: core::ConsensusId -> core::ConsensusInstance,
        instanceTiming: core::ConsensusId -> timing::InstanceTimingState,
        witnessNonces: core::AuthorityId -> core::Option[core::CachedNonce],
        currentEpoch: core::Epoch,
        participation: core::AuthorityId -> timing::WitnessParticipation,
        globalTime: timing::Time,
        gstReached: bool
    ): bool =
        instances.keys().forall(cid => {
            val inst = instances.get(cid)
            val timingState = if (cid.in(instanceTiming.keys())) instanceTiming.get(cid)
                         else { startTime: 0, lastActivity: 0, fallbackStartTime: core::None,
                                retryCount: 0, messagesInFlight: 0 }
            val elapsedSinceStart = globalTime - timingState.startTime
            val responsiveConditions = timing::fastPathResponsiveConditions(
                inst, witnessNonces, currentEpoch,
                core::NONCE_VALIDITY_WINDOW, participation, gstReached
            )
            // If responsive conditions held and enough time passed, should not still be active
            (responsiveConditions and elapsedSinceStart > timing::FAST_PATH_BOUND)
            implies (inst.phase == core::ConsensusCommitted or inst.phase == core::FallbackActive)
        })

    // Fast path progress check (invariant approximation)
    pure def fastPathProgressCheck(
        instances: core::ConsensusId -> core::ConsensusInstance,
        witnessNonces: core::AuthorityId -> core::Option[core::CachedNonce],
        currentEpoch: core::Epoch,
        participation: core::AuthorityId -> timing::WitnessParticipation,
        gstReached: bool
    ): bool =
        instances.keys().forall(cid => {
            val inst = instances.get(cid)
            canCompleteFastPath(inst, witnessNonces, currentEpoch,
                               core::NONCE_VALIDITY_WINDOW, participation, gstReached)
            implies (inst.proposals.size() > 0 or inst.phase == core::ConsensusCommitted)
        })

    // Slow path progress check (invariant approximation)
    pure def slowPathProgressCheck(
        instances: core::ConsensusId -> core::ConsensusInstance,
        participation: core::AuthorityId -> timing::WitnessParticipation,
        gstReached: bool
    ): bool =
        instances.keys().forall(cid => {
            val inst = instances.get(cid)
            inFallbackWithQuorum(inst, participation, gstReached)
            implies (inst.proposals.size() > 0 or isTerminated(inst))
        })

    // Active instances make progress (invariant approximation)
    pure def activeMakesProgress(
        instances: core::ConsensusId -> core::ConsensusInstance,
        participation: core::AuthorityId -> timing::WitnessParticipation,
        gstReached: bool
    ): bool =
        instances.keys().forall(cid => {
            val inst = instances.get(cid)
            isActiveWithQuorum(inst, participation, gstReached)
            implies (inst.fallbackTimerActive or inst.proposals.size() > 0 or isTerminated(inst))
        })

    // No deadlock: There's always an enabled action for active instances
    pure def noDeadlock(
        instances: core::ConsensusId -> core::ConsensusInstance
    ): bool =
        instances.keys().forall(cid => {
            val inst = instances.get(cid)
            val isActive = inst.phase == core::FastPathActive or inst.phase == core::FallbackActive
            // Active instances can always make progress
            not(isActive) or (
                inst.witnesses.exists(w => not(core::hasProposal(inst.proposals, w))) or
                inst.fallbackTimerActive or
                inst.proposals.size() >= inst.threshold
            )
        })

    // ════════════════════════════════════════════════════════════════════════
    // AVAILABILITY AND CONNECTIVITY PROPERTIES (T3e.4-T3e.6)
    //
    // These properties characterize the slow path's availability guarantees.
    // ════════════════════════════════════════════════════════════════════════

    // Count proposals from witnesses who are currently honest
    pure def proposalsFromCurrentlyHonest(
        inst: core::ConsensusInstance,
        participation: core::AuthorityId -> timing::WitnessParticipation
    ): int = {
        val currentlyHonest = inst.witnesses.filter(w =>
            w.in(participation.keys()) and
            participation.get(w).isHonest
        )
        inst.proposals.filter(p => p.witness.in(currentlyHonest)).size()
    }

    // T3e.4a: Weak version - committed implies some honest participation
    pure def invariantCommitRequiresHonestParticipation(
        instances: core::ConsensusId -> core::ConsensusInstance,
        participation: core::AuthorityId -> timing::WitnessParticipation
    ): bool =
        instances.keys().forall(cid => {
            val inst = instances.get(cid)
            val honestProposals = proposalsFromCurrentlyHonest(inst, participation)
            inst.phase == core::ConsensusCommitted implies honestProposals > 0
        })

    // T3e.4b: Strong version - quorum possible only with threshold honest
    pure def invariantQuorumPossible(
        instances: core::ConsensusId -> core::ConsensusInstance,
        participation: core::AuthorityId -> timing::WitnessParticipation
    ): bool =
        instances.keys().forall(cid => {
            val inst = instances.get(cid)
            val honestWitnesses = inst.witnesses.filter(w =>
                w.in(participation.keys()) and
                participation.get(w).isHonest
            ).size()
            // If fewer than threshold honest witnesses exist, commit is impossible
            honestWitnesses < inst.threshold implies inst.phase != core::ConsensusCommitted
        })

    // T3e.5: PropertyQuorumSufficient
    // If k honest witnesses are online AND in the same connected component,
    // then the instance can make progress.
    pure def propertyQuorumSufficient(
        instances: core::ConsensusId -> core::ConsensusInstance,
        participation: core::AuthorityId -> timing::WitnessParticipation,
        gossipGraph: core::AuthorityId -> Set[core::AuthorityId]
    ): bool =
        instances.keys().forall(cid => {
            val inst = instances.get(cid)
            val isActive = inst.phase == core::FastPathActive or inst.phase == core::FallbackActive
            val hasConnectedQuorum = conn::hasQuorumInConnectedComponent(
                inst, participation, gossipGraph
            )
            // If active with connected quorum, should not be in failed state
            (isActive and hasConnectedQuorum) implies inst.phase != core::ConsensusFailed
        })

    // T3e.6: PropertyPartitionTolerance
    // Under network partition, any partition containing at least k honest
    // witnesses in a connected component can still complete.
    pure def propertyPartitionTolerance(
        instances: core::ConsensusId -> core::ConsensusInstance,
        participation: core::AuthorityId -> timing::WitnessParticipation,
        gossipGraph: core::AuthorityId -> Set[core::AuthorityId]
    ): bool =
        instances.keys().forall(cid => {
            val inst = instances.get(cid)
            val largestComponent = conn::largestHonestConnectedComponent(
                inst.witnesses, participation, gossipGraph
            )
            val isActive = inst.phase == core::FastPathActive or inst.phase == core::FallbackActive
            // If largest component has threshold honest, should not be failed
            (isActive and largestComponent.size() >= inst.threshold)
            implies inst.phase != core::ConsensusFailed
        })

    // Combined availability property
    pure def availabilityGuarantee(
        instances: core::ConsensusId -> core::ConsensusInstance,
        participation: core::AuthorityId -> timing::WitnessParticipation,
        gossipGraph: core::AuthorityId -> Set[core::AuthorityId]
    ): bool =
        invariantQuorumPossible(instances, participation) and
        propertyQuorumSufficient(instances, participation, gossipGraph) and
        propertyPartitionTolerance(instances, participation, gossipGraph)

    // ════════════════════════════════════════════════════════════════════════
    // FAILURE MODE CHARACTERIZATION
    //
    // Failure modes that can delay (but not prevent) liveness.
    // ════════════════════════════════════════════════════════════════════════

    // FM1: Network partition (before GST)
    pure def failureModePartition(
        instances: core::ConsensusId -> core::ConsensusInstance,
        participation: core::AuthorityId -> timing::WitnessParticipation,
        gstReached: bool
    ): bool =
        not(gstReached) and
        instances.keys().exists(cid => {
            val inst = instances.get(cid)
            val isActive = inst.phase == core::FastPathActive or inst.phase == core::FallbackActive
            val onlineCount = timing::countOnlineHonest(inst.witnesses, participation)
            isActive and onlineCount < inst.threshold
        })

    // FM2: Nonce exhaustion (triggers slow path)
    pure def failureModeNonceExhaustion(
        instances: core::ConsensusId -> core::ConsensusInstance,
        witnessNonces: core::AuthorityId -> core::Option[core::CachedNonce],
        currentEpoch: core::Epoch
    ): bool =
        instances.keys().exists(cid => {
            val inst = instances.get(cid)
            inst.phase == core::FastPathActive and
            not(core::allWitnessesHaveValidNonces(
                inst.witnesses, witnessNonces, currentEpoch, core::NONCE_VALIDITY_WINDOW
            ))
        })

    // FM4: Byzantine behavior (< threshold)
    pure def failureModeByzantine(byzantineCount: int): bool =
        byzantineCount > 0 and byzantineCount < 2  // Non-zero but tolerable

    // System is in a failure mode but still live
    pure def inFailureModeButLive(
        instances: core::ConsensusId -> core::ConsensusInstance,
        participation: core::AuthorityId -> timing::WitnessParticipation,
        witnessNonces: core::AuthorityId -> core::Option[core::CachedNonce],
        currentEpoch: core::Epoch,
        byzantineCount: int,
        gstReached: bool
    ): bool =
        (failureModePartition(instances, participation, gstReached) or
         failureModeNonceExhaustion(instances, witnessNonces, currentEpoch) or
         failureModeByzantine(byzantineCount)) and
        gstReached implies instances.keys().forall(cid => {
            val inst = instances.get(cid)
            val hasQuorum = timing::hasQuorumOnline(inst.witnesses, participation, inst.threshold)
            not(hasQuorum) or inst.phase != core::ConsensusFailed
        })
}
