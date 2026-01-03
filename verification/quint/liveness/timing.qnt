// Protocol Liveness Timing - Synchrony Model and Timing Types
//
// This module defines the partial synchrony model (GST-based) and timing
// primitives used for liveness analysis. Separated from main liveness spec
// for modularity.
//
// See: docs/004_distributed_systems_contract.md §4, docs/104_consensus.md §15

module protocol_liveness_timing {
    import protocol_consensus as core from "../consensus/core"

    // ════════════════════════════════════════════════════════════════════════
    // TYPES: SYNCHRONY MODEL
    //
    // Partial synchrony with Global Stabilization Time (GST).
    // Before GST: messages may be arbitrarily delayed (but not lost)
    // After GST: all messages delivered within bound Δ
    // ════════════════════════════════════════════════════════════════════════

    // Logical time units for model (abstract, not wall-clock)
    type Time = int

    // Time constants (in abstract units)
    pure val DELTA: Time = 3                    // Message delay bound after GST
    pure val T_FALLBACK: Time = DELTA * 2       // Fallback trigger timeout (2-3× Δ)
    pure val GOSSIP_INTERVAL: Time = 1          // Gossip period
    pure val MAX_RETRIES: int = 5               // Maximum retry attempts

    // ════════════════════════════════════════════════════════════════════════
    // TIMING BOUNDS
    //
    // Fast path: optimistically responsive (completes at network speed)
    // Slow path: availability-optimized (tolerates partitions, no tight time bound)
    // ════════════════════════════════════════════════════════════════════════

    // Fast path timing: 2δ = 1 RTT (propose + collect shares)
    // This is the "optimistically responsive" guarantee from the literature
    pure val FAST_PATH_BOUND: Time = 2 * DELTA  // 2δ for responsive completion

    // ════════════════════════════════════════════════════════════════════════
    // TIMING STATE TYPES
    // ════════════════════════════════════════════════════════════════════════

    // Per-instance timing state
    type InstanceTimingState = {
        startTime: Time,                        // When instance was started
        lastActivity: Time,                     // Last message/progress time
        fallbackStartTime: core::Option[Time],  // When fallback was triggered
        retryCount: int,                        // Number of retry attempts
        messagesInFlight: int                   // Pending message count (abstract)
    }

    // Message queue abstraction for synchrony model
    type PendingMessage = {
        cid: core::ConsensusId,
        sender: core::AuthorityId,
        sentTime: Time,
        deliveryTime: Time                      // Earliest delivery (sentTime + delay)
    }

    // Track honest vs Byzantine behavior for liveness analysis
    type WitnessParticipation = {
        witness: core::AuthorityId,
        isHonest: bool,                         // Follows protocol correctly
        isOnline: bool,                         // Currently reachable
        lastSeen: Time,                         // Last known activity
        sharesSent: int                         // Total shares contributed
    }

    // ════════════════════════════════════════════════════════════════════════
    // SYNCHRONY HELPERS
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

    // Check if instance has timed out (needs fallback or retry)
    pure def hasTimedOut(
        timing: InstanceTimingState,
        currentTime: Time,
        timeout: Time
    ): bool =
        (currentTime - timing.lastActivity) >= timeout

    // ════════════════════════════════════════════════════════════════════════
    // QUORUM HELPERS
    // ════════════════════════════════════════════════════════════════════════

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

    // ════════════════════════════════════════════════════════════════════════
    // FAST PATH HELPERS
    // ════════════════════════════════════════════════════════════════════════

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

    // Helper: All witnesses in instance are online and honest
    pure def allWitnessesOnlineHonest(
        inst: core::ConsensusInstance,
        participation: core::AuthorityId -> WitnessParticipation
    ): bool =
        inst.witnesses.forall(w =>
            w.in(participation.keys()) and
            participation.get(w).isOnline and
            participation.get(w).isHonest
        )

    // Helper: All witnesses have valid nonces for signing
    pure def allWitnessesHaveValidNonces(
        inst: core::ConsensusInstance,
        nonces: core::AuthorityId -> core::Option[core::CachedNonce],
        epoch: core::Epoch,
        validityWindow: int
    ): bool =
        inst.witnesses.forall(w =>
            core::isNonceValid(
                if (w.in(nonces.keys())) nonces.get(w) else core::None,
                epoch,
                validityWindow
            )
        )

    // Helper: Check conditions for fast path responsive completion
    pure def fastPathResponsiveConditions(
        inst: core::ConsensusInstance,
        nonces: core::AuthorityId -> core::Option[core::CachedNonce],
        epoch: core::Epoch,
        validityWindow: int,
        participation: core::AuthorityId -> WitnessParticipation,
        synch: bool
    ): bool =
        inst.phase == core::FastPathActive and
        synch and
        allWitnessesOnlineHonest(inst, participation) and
        allWitnessesHaveValidNonces(inst, nonces, epoch, validityWindow)
}
