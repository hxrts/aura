// DKD (Deterministic Key Derivation) Protocol
// Formal specification of Aura's P2P DKD choreography

module DKDProtocol {
    // Basic types
    type DeviceId = str
    type SessionId = str

    // ========== DKD-Specific Types ==========

    type DKDState =
        | "DKDInit"
        | "DKDCommitment"
        | "DKDReveal"
        | "DKDFinalization"
        | "DKDComplete"
        | "DKDFailure"

    type Commitment = str  // Blake3 hash of point
    type Point = str       // Ed25519 curve point
    type DerivedKey = str  // Final derived key

    type DKDParticipant = {
        deviceId: DeviceId,
        shareBytes: str,
        commitment: Commitment,
        point: Point,
        hasCommitted: bool,
        hasRevealed: bool
    }

    type DKDSession = {
        sessionId: SessionId,
        contextId: str,
        appId: str,
        threshold: int,
        participants: Set[DeviceId],
        state: DKDState,
        commitments: DeviceId -> Commitment,
        reveals: DeviceId -> Point,
        derivedKey: DerivedKey,
        startEpoch: int,
        ttlEpochs: int
    }

    // ========== State Variables ==========

    var dkdSessions: SessionId -> DKDSession
    var dkdParticipants: (SessionId, DeviceId) -> DKDParticipant
    var currentEpoch: int

    // ========== Helper Functions ==========

    // Hash share to scalar and compute point
    pure def computePoint(shareBytes: str, contextId: str): Point =
        "P(" + shareBytes + "|" + contextId + ")"

    // Compute commitment from point
    pure def computeCommitment(point: Point): Commitment =
        "commit_" + point

    // Verify reveal matches commitment
    pure def verifyReveal(commitment: Commitment, point: Point): bool =
        commitment == computeCommitment(point)

    // Aggregate points to derive key
    pure def aggregatePoints(points: Set[Point]): DerivedKey =
        "key_" + points.size().toString()

    // Check if we have threshold commitments
    pure def hasThresholdCommitments(session: DKDSession): bool =
        session.commitments.keys().size() >= session.threshold

    // Check if we have matching reveals for all commitments
    pure def hasMatchingReveals(session: DKDSession): bool =
        session.commitments.keys().forall(deviceId =>
            session.reveals.keys().contains(deviceId) and
            verifyReveal(
                session.commitments.get(deviceId),
                session.reveals.get(deviceId)
            )
        )

    // ========== Actions ==========

    // Initialize DKD session
    action initiateDKDSession(
        sessionId: SessionId,
        contextId: str,
        appId: str,
        threshold: int,
        participants: Set[DeviceId]
    ): bool = all {
        // Session doesn't exist
        not(dkdSessions.keys().contains(sessionId)),
        // Valid threshold
        threshold > 0,
        threshold <= participants.size(),
        // Create session
        dkdSessions' = dkdSessions.put(sessionId, {
            sessionId: sessionId,
            contextId: contextId,
            appId: appId,
            threshold: threshold,
            participants: participants,
            state: "DKDInit",
            commitments: Map(),
            reveals: Map(),
            derivedKey: "",
            startEpoch: currentEpoch,
            ttlEpochs: 50
        }),
        // Initialize participants with deterministic shares
        dkdParticipants' = participants.fold(dkdParticipants, (acc, deviceId) =>
            acc.put((sessionId, deviceId), {
                deviceId: deviceId,
                shareBytes: bytes("share_" + deviceId),
                commitment: Commitment(""),
                point: Point(""),
                hasCommitted: false,
                hasRevealed: false
            })
        ),
        currentEpoch' = currentEpoch
    }

    // Record commitment from participant
    action recordCommitment(sessionId: SessionId, deviceId: DeviceId): bool = all {
        // Session exists and is in correct state
        dkdSessions.keys().contains(sessionId),
        val session = dkdSessions.get(sessionId),
        session.state == "DKDInit" or session.state == "DKDCommitment",
        // Compute commitment
        val shareBytes = "share_" + deviceId,
        val point = computePoint(shareBytes, session.contextId),
        val commitment = computeCommitment(point),
        // Update session with new commitment
        val newCommitments = session.commitments.put(deviceId, commitment),
        val newState = if (newCommitments.keys().size() >= session.threshold) "DKDReveal" else "DKDCommitment",
        dkdSessions' = dkdSessions.put(sessionId, { ...session,
            commitments: newCommitments,
            state: newState
        }),
        dkdParticipants' = dkdParticipants,
        currentEpoch' = currentEpoch + 1
    }

    // Reveal point from participant
    action revealPoint(sessionId: SessionId, deviceId: DeviceId): bool = all {
        // Session exists and is in reveal phase
        dkdSessions.keys().contains(sessionId),
        val session = dkdSessions.get(sessionId),
        session.state == "DKDReveal",
        // Device is in commitment set
        session.commitments.keys().contains(deviceId),
        // Get the point for this device
        val shareBytes = "share_" + deviceId,
        val point = computePoint(shareBytes, session.contextId),
        // Update session with reveal
        val newReveals = session.reveals.put(deviceId, point),
        val allRevealed = session.commitments.keys().forall(id =>
            newReveals.keys().contains(id)
        ),
        val newState = if (allRevealed) "DKDFinalization" else "DKDReveal",
        dkdSessions' = dkdSessions.put(sessionId, { ...session,
            reveals: newReveals,
            state: newState
        }),
        dkdParticipants' = dkdParticipants,
        currentEpoch' = currentEpoch + 1
    }

    // Finalize DKD and derive key
    action finalizeDKD(sessionId: SessionId): bool = all {
        // Session exists and is ready for finalization
        dkdSessions.keys().contains(sessionId),
        val session = dkdSessions.get(sessionId),
        session.state == "DKDFinalization",
        // All reveals match commitments
        hasMatchingReveals(session),
        // Derive key from revealed points
        val points = session.reveals.values().toSet(),
        val derivedKey = aggregatePoints(points),
        // Update session
        dkdSessions' = dkdSessions.put(sessionId, { ...session,
            derivedKey: derivedKey,
            state: "DKDComplete"
        }),
        dkdParticipants' = dkdParticipants,
        currentEpoch' = currentEpoch + 1
    }

    // ========== Invariants ==========

    // Threshold property: can't finalize without threshold participants
    val thresholdRequirement =
        dkdSessions.values().forall(session =>
            session.state == "DKDComplete" implies
                session.commitments.keys().size() >= session.threshold and
                session.reveals.keys().size() >= session.threshold
        )

    // Reveal verification: all reveals must match commitments
    val revealIntegrity =
        dkdSessions.values().forall(session =>
            session.reveals.keys().forall(deviceId =>
                session.commitments.keys().contains(deviceId) implies
                    verifyReveal(
                        session.commitments.get(deviceId),
                        session.reveals.get(deviceId)
                    )
            )
        )

    // No reveals without commitments
    val noOrphanReveals =
        dkdSessions.values().forall(session =>
            session.reveals.keys().isSubsetOf(session.commitments.keys())
        )

    // ========== Temporal Properties ==========

    // Eventually completes or fails
    temporal eventualCompletion = always(
        dkdSessions.values().forall(session =>
            session.state == "DKDInit" implies
                eventually(session.state == "DKDComplete" or session.state == "DKDFailure")
        )
    )

    // ========== Initial State ==========

    action init = all {
        dkdSessions' = Map(),
        dkdParticipants' = Map(),
        currentEpoch' = 0
    }
}
