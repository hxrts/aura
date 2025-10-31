// Minimal DKD Protocol for E2E Testing
module DKDMinimal {
    // Simple state
    var sessionCount: int
    var completedSessions: int
    var failedSessions: int
    
    // Constants
    pure val MAX_SESSIONS = 10
    
    // Actions
    action startSession: bool = all {
        sessionCount < MAX_SESSIONS,
        sessionCount' = sessionCount + 1,
        completedSessions' = completedSessions,
        failedSessions' = failedSessions
    }
    
    action completeSession: bool = all {
        sessionCount > completedSessions + failedSessions,
        completedSessions' = completedSessions + 1,
        sessionCount' = sessionCount,
        failedSessions' = failedSessions
    }
    
    action failSession: bool = all {
        sessionCount > completedSessions + failedSessions,
        failedSessions' = failedSessions + 1,
        sessionCount' = sessionCount,
        completedSessions' = completedSessions
    }
    
    // Invariants
    val validCounts = 
        sessionCount >= 0 and
        completedSessions >= 0 and
        failedSessions >= 0 and
        completedSessions + failedSessions <= sessionCount
    
    val sessionLimit = sessionCount <= MAX_SESSIONS
    
    // Properties
    val safetyProperty = completedSessions >= 0
    val progressProperty = sessionCount <= MAX_SESSIONS
    
    // Initial state
    action init = all {
        sessionCount' = 0,
        completedSessions' = 0,
        failedSessions' = 0
    }
    
    // Step for simulation
    action step = any {
        startSession,
        completeSession,
        failSession
    }
}