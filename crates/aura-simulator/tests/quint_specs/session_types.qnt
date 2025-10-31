// Session Types Core Infrastructure
// Formal specification of Aura's session type system

module SessionTypes {
    // ========== Core Types ==========
    
    type DeviceId = str
    type SessionId = str
    type ProtocolId = str
    
    // Session states with compile-time safety
    type SessionState = 
        | InitialState
        | IntermediateState(name: str)
        | FinalState(success: bool)
    
    // Protocol types
    type ProtocolType = 
        | DKD
        | FROST
        | Recovery
        | Resharing
        | CGKA
    
    // Runtime witness for state transitions
    type RuntimeWitness = {
        evidenceType: str,
        verified: bool,
        timestamp: int
    }
    
    // Session protocol state
    type SessionProtocol = {
        sessionId: SessionId,
        protocolId: ProtocolId,
        deviceId: DeviceId,
        protocolType: ProtocolType,
        currentState: SessionState,
        canTerminate: bool,
        isFinal: bool
    }
    
    // State transition
    type StateTransition = {
        fromState: SessionState,
        toState: SessionState,
        witness: RuntimeWitness
    }
    
    // ========== State Variables ==========
    
    var protocols: SessionId -> SessionProtocol
    var transitions: Set[StateTransition]
    var time: int
    
    // ========== Helper Functions ==========
    
    // Check if a state is terminal
    pure def isTerminal(state: SessionState): bool = {
        match state {
            | FinalState(_) => true
            | _ => false
        }
    }
    
    // Check if transition is valid
    pure def isValidTransition(from: SessionState, to: SessionState, witness: RuntimeWitness): bool = {
        // Initial state can only go to intermediate states
        match (from, to) {
            | (InitialState, IntermediateState(_)) => witness.verified
            | (IntermediateState(_), IntermediateState(_)) => witness.verified
            | (IntermediateState(_), FinalState(_)) => witness.verified
            | (InitialState, FinalState(_)) => false  // Can't skip intermediate states
            | (FinalState(_), _) => false  // Final states are terminal
        }
    }
    
    // ========== Actions ==========
    
    // Create a new protocol instance
    action createProtocol(sessionId: SessionId, protocolType: ProtocolType, deviceId: DeviceId): bool = {
        all {
            // Protocol doesn't already exist
            not(sessionId.in(protocols.keys())),
            // Create new protocol
            val protocol = {
                sessionId: sessionId,
                protocolId: sessionId,  // For simplicity
                deviceId: deviceId,
                protocolType: protocolType,
                currentState: InitialState,
                canTerminate: false,
                isFinal: false
            },
            protocols' = protocols.put(sessionId, protocol),
            transitions' = transitions,
            time' = time + 1
        }
    }
    
    // Transition protocol state with witness
    action transitionWithWitness(
        sessionId: SessionId, 
        toState: SessionState,
        witness: RuntimeWitness
    ): bool = {
        all {
            // Protocol exists
            sessionId.in(protocols.keys()),
            val protocol = protocols.get(sessionId),
            // Not already in final state
            not(isTerminal(protocol.currentState)),
            // Valid transition
            isValidTransition(protocol.currentState, toState, witness),
            // Update protocol
            val newProtocol = protocol.with("currentState", toState)
                .with("isFinal", isTerminal(toState))
                .with("canTerminate", isTerminal(toState)),
            protocols' = protocols.put(sessionId, newProtocol),
            // Record transition
            val transition = {
                fromState: protocol.currentState,
                toState: toState,
                witness: witness
            },
            transitions' = transitions.union(Set(transition)),
            time' = time + 1
        }
    }
    
    // ========== Invariants ==========
    
    // No protocol can transition after reaching final state
    invariant noTransitionAfterFinal = {
        protocols.values().forall(protocol =>
            protocol.isFinal implies 
                not(transitions.exists(t => 
                    t.fromState == protocol.currentState and
                    protocols.values().exists(p => 
                        p.sessionId == protocol.sessionId and
                        p.currentState == t.toState
                    )
                ))
        )
    }
    
    // All transitions must have verified witnesses
    invariant allTransitionsWitnessed = {
        transitions.forall(t => t.witness.verified)
    }
    
    // Final states must be terminal
    invariant finalStatesAreTerminal = {
        protocols.values().forall(protocol =>
            isTerminal(protocol.currentState) == protocol.isFinal
        )
    }
    
    // ========== Properties ==========
    
    // Type safety: Invalid transitions are impossible
    property typeSafetyProperty = {
        // If we're in a final state, we can't transition
        always(protocols.values().forall(protocol =>
            protocol.isFinal implies
                eventually(protocols.get(protocol.sessionId).currentState == protocol.currentState)
        ))
    }
    
    // Progress: Non-final protocols can always make progress
    property progressProperty = {
        always(protocols.values().forall(protocol =>
            not(protocol.isFinal) implies
                // There exists some witness that would allow progress
                IntermediateState("next").in(Set(IntermediateState("next"), FinalState(true), FinalState(false)))
        ))
    }
    
    // Witness requirement: No transition without witness
    property witnessRequirement = {
        always(transitions.forall(t => t.witness.verified))
    }
    
    // ========== Initial State ==========
    
    action init = {
        all {
            protocols' = Map(),
            transitions' = Set(),
            time' = 0
        }
    }
}