// TODO fix - Simplified DKD Protocol for E2E Testing
module DKDSimple {
    // Basic types
    type DeviceId = str
    type SessionId = str

    // DKD States as constants
    pure val DKD_INIT = "init"
    pure val DKD_COMMITMENT = "commitment"
    pure val DKD_REVEAL = "reveal"
    pure val DKD_COMPLETE = "complete"
    pure val DKD_FAILURE = "failure"

    // Simple session structure
    type DKDSession = {
        id: SessionId,
        state: str,
        threshold: int,
        participantCount: int
    }

    // State variables
    var sessions: Set[DKDSession]
    var currentTime: int

    // Helper functions
    pure def sessionExists(sessionId: SessionId): bool =
        sessions.exists(s => s.id == sessionId)

    pure def getSession(sessionId: SessionId): DKDSession =
        sessions.filter(s => s.id == sessionId).fold(
            { id: "", state: DKD_FAILURE, threshold: 0, participantCount: 0 },
            (acc, s) => s
        )

    pure def canTransition(fromState: str, toState: str): bool =
        (fromState == DKD_INIT and toState == DKD_COMMITMENT) or
        (fromState == DKD_COMMITMENT and toState == DKD_REVEAL) or
        (fromState == DKD_REVEAL and toState == DKD_COMPLETE) or
        (fromState == DKD_REVEAL and toState == DKD_FAILURE)

    // Actions
    action initSession(sessionId: SessionId, threshold: int, participantCount: int): bool =
        all {
            not(sessionExists(sessionId)),
            threshold > 0,
            threshold <= participantCount,
            sessions' = sessions.union(Set({
                id: sessionId,
                state: DKD_INIT,
                threshold: threshold,
                participantCount: participantCount
            })),
            currentTime' = currentTime + 1
        }

    action advanceSession(sessionId: SessionId, newState: str): bool =
        all {
            sessionExists(sessionId),
            val session = getSession(sessionId),
            canTransition(session.state, newState),
            val updatedSession = { ...session, state: newState },
            sessions' = sessions.exclude(Set(session)).union(Set(updatedSession)),
            currentTime' = currentTime + 1
        }

    // Invariants
    val validStates =
        sessions.forall(s =>
            s.state == DKD_INIT or
            s.state == DKD_COMMITMENT or
            s.state == DKD_REVEAL or
            s.state == DKD_COMPLETE or
            s.state == DKD_FAILURE
        )

    val validThresholds =
        sessions.forall(s => s.threshold > 0 and s.threshold <= s.participantCount)

    // Simple temporal property
    temporal progressProperty = always(
        sessions.filter(s => s.state == DKD_INIT).forall(s =>
            eventually(s.state == DKD_COMPLETE or s.state == DKD_FAILURE)
        )
    )

    // Initial state
    action init = all {
        sessions' = Set(),
        currentTime' = 0
    }

    // Step action for testing
    action step = any {
        init,
        nondet sessionId = oneOf(Set("session1", "session2", "session3"))
        nondet threshold = oneOf(1.to(3))
        nondet participantCount = oneOf(2.to(5))
        initSession(sessionId, threshold, participantCount),

        nondet sessionId = oneOf(Set("session1", "session2", "session3"))
        nondet newState = oneOf(Set(DKD_COMMITMENT, DKD_REVEAL, DKD_COMPLETE, DKD_FAILURE))
        advanceSession(sessionId, newState)
    }
}
