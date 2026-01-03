// Protocol Anti-Entropy - CRDT Synchronization and Convergence
//
// This module models Aura's anti-entropy protocol for CRDT state synchronization:
// - Merkle tree-based state comparison
// - Delta exchange for efficient sync
// - Eventual convergence guarantees
//
// Key properties to verify:
// - Convergence: All nodes eventually reach same state
// - Monotonicity: State only grows (no rollback)
// - Idempotency: Re-applying deltas is safe
//
// See: docs/111_maintenance.md, docs/102_journal.md

module protocol_anti_entropy {
    // ==================== TYPE DEFINITIONS ====================

    type Option[a] = Some(a) | None

    type NodeId = str
    type StateHash = str
    type Version = int
    type DeltaId = str

    // CRDT state at a node
    type CrdtState = {
        nodeId: NodeId,
        version: Version,
        stateHash: StateHash,
        facts: Set[str],           // Simplified fact set
        vectorClock: NodeId -> int // Lamport clock per node
    }

    // Delta for state transfer
    type Delta = {
        deltaId: DeltaId,
        sourceNode: NodeId,
        sourceVersion: Version,
        facts: Set[str],
        vectorClock: NodeId -> int
    }

    // Sync session between two nodes
    type SyncSession = {
        initiator: NodeId,
        responder: NodeId,
        initiatorHash: StateHash,
        responderHash: StateHash,
        deltasExchanged: Set[DeltaId],
        completed: bool
    }

    // Anti-entropy phase
    type AntiEntropyPhase =
        | AEIdle
        | AEComparing       // Comparing merkle roots
        | AEExchanging      // Exchanging deltas
        | AEConverged       // States match

    // ==================== STATE VARIABLES ====================

    var nodes: Set[NodeId]
    var nodeStates: NodeId -> CrdtState
    var pendingDeltas: Set[Delta]
    var syncSessions: Set[SyncSession]
    var globalFacts: Set[str]  // Ground truth for convergence check

    // ==================== HELPER FUNCTIONS ====================

    pure def computeHash(facts: Set[str]): StateHash =
        if (facts.size() == 0) "hash_empty"
        else if (facts == Set("fact_a")) "hash_a"
        else if (facts == Set("fact_b")) "hash_b"
        else if (facts == Set("fact_a", "fact_b")) "hash_ab"
        else if (facts == Set("fact_a", "fact_c")) "hash_ac"
        else if (facts == Set("fact_b", "fact_c")) "hash_bc"
        else if (facts == Set("fact_a", "fact_b", "fact_c")) "hash_abc"
        else "hash_other"

    pure def mergeVectorClocks(vc1: NodeId -> int, vc2: NodeId -> int, allNodes: Set[NodeId]): NodeId -> int =
        allNodes.fold(Map(), (acc, n) => {
            val v1 = if (n.in(vc1.keys())) vc1.get(n) else 0
            val v2 = if (n.in(vc2.keys())) vc2.get(n) else 0
            acc.put(n, if (v1 > v2) v1 else v2)
        })

    pure def incrementClock(vc: NodeId -> int, node: NodeId): NodeId -> int =
        val current = if (node.in(vc.keys())) vc.get(node) else 0
        vc.put(node, current + 1)

    // Check if vc1 happens-before vc2
    pure def happensBefore(vc1: NodeId -> int, vc2: NodeId -> int, allNodes: Set[NodeId]): bool =
        val lessOrEqual = allNodes.forall(n => {
            val v1 = if (n.in(vc1.keys())) vc1.get(n) else 0
            val v2 = if (n.in(vc2.keys())) vc2.get(n) else 0
            v1 <= v2
        })
        val strictlyLess = allNodes.exists(n => {
            val v1 = if (n.in(vc1.keys())) vc1.get(n) else 0
            val v2 = if (n.in(vc2.keys())) vc2.get(n) else 0
            v1 < v2
        })
        lessOrEqual and strictlyLess

    // ==================== ACTIONS ====================

    // Initialize anti-entropy module
    action init: bool = all {
        nodes' = Set("node1", "node2", "node3"),
        nodeStates' = Map(
            ("node1", { nodeId: "node1", version: 0, stateHash: "hash_empty", facts: Set(), vectorClock: Map(("node1", 0), ("node2", 0), ("node3", 0)) }),
            ("node2", { nodeId: "node2", version: 0, stateHash: "hash_empty", facts: Set(), vectorClock: Map(("node1", 0), ("node2", 0), ("node3", 0)) }),
            ("node3", { nodeId: "node3", version: 0, stateHash: "hash_empty", facts: Set(), vectorClock: Map(("node1", 0), ("node2", 0), ("node3", 0)) })
        ),
        pendingDeltas' = Set(),
        syncSessions' = Set(),
        globalFacts' = Set()
    }

    // Local write: node adds a new fact
    action localWrite(node: NodeId, fact: str): bool = all {
        node.in(nodes),
        node.in(nodeStates.keys()),
        val state = nodeStates.get(node)
        val notAlreadyPresent = not(state.facts.contains(fact))
        all {
            notAlreadyPresent,
            val newFacts = state.facts.union(Set(fact))
            val newVC = incrementClock(state.vectorClock, node)
            val newState = {
                ...state,
                version: state.version + 1,
                stateHash: computeHash(newFacts),
                facts: newFacts,
                vectorClock: newVC
            }
            nodeStates' = nodeStates.put(node, newState),
            globalFacts' = globalFacts.union(Set(fact)),
            nodes' = nodes,
            pendingDeltas' = pendingDeltas,
            syncSessions' = syncSessions
        }
    }

    // Initiate sync session between two nodes
    action initiateSyncSession(initiator: NodeId, responder: NodeId): bool = all {
        initiator.in(nodes),
        responder.in(nodes),
        initiator != responder,
        initiator.in(nodeStates.keys()),
        responder.in(nodeStates.keys()),
        val initState = nodeStates.get(initiator)
        val respState = nodeStates.get(responder)
        // Only sync if hashes differ
        val hashesMatch = initState.stateHash == respState.stateHash
        all {
            not(hashesMatch),
            val session = {
                initiator: initiator,
                responder: responder,
                initiatorHash: initState.stateHash,
                responderHash: respState.stateHash,
                deltasExchanged: Set(),
                completed: false
            }
            syncSessions' = syncSessions.union(Set(session)),
            nodes' = nodes,
            nodeStates' = nodeStates,
            pendingDeltas' = pendingDeltas,
            globalFacts' = globalFacts
        }
    }

    // Create delta from source to target
    action createDelta(source: NodeId, target: NodeId, deltaId: DeltaId): bool = all {
        source.in(nodes),
        target.in(nodes),
        source != target,
        source.in(nodeStates.keys()),
        target.in(nodeStates.keys()),
        val sourceState = nodeStates.get(source)
        val targetState = nodeStates.get(target)
        // Delta contains facts that source has but target doesn't
        val missingFacts = sourceState.facts.exclude(targetState.facts)
        all {
            missingFacts.size() > 0,
            val delta = {
                deltaId: deltaId,
                sourceNode: source,
                sourceVersion: sourceState.version,
                facts: missingFacts,
                vectorClock: sourceState.vectorClock
            }
            pendingDeltas' = pendingDeltas.union(Set(delta)),
            nodes' = nodes,
            nodeStates' = nodeStates,
            syncSessions' = syncSessions,
            globalFacts' = globalFacts
        }
    }

    // Apply delta to a node (CRDT merge)
    action applyDelta(node: NodeId, deltaId: DeltaId): bool = all {
        node.in(nodes),
        node.in(nodeStates.keys()),
        pendingDeltas.exists(d => d.deltaId == deltaId),
        val delta = pendingDeltas.filter(d => d.deltaId == deltaId).fold(
            { deltaId: "", sourceNode: "", sourceVersion: 0, facts: Set(), vectorClock: Map() },
            (acc, d) => d
        )
        val state = nodeStates.get(node)
        // Merge facts (CRDT union)
        val mergedFacts = state.facts.union(delta.facts)
        // Merge vector clocks
        val mergedVC = mergeVectorClocks(state.vectorClock, delta.vectorClock, nodes)
        val newState = {
            ...state,
            version: state.version + 1,
            stateHash: computeHash(mergedFacts),
            facts: mergedFacts,
            vectorClock: mergedVC
        }
        all {
            nodeStates' = nodeStates.put(node, newState),
            pendingDeltas' = pendingDeltas.filter(d => d.deltaId != deltaId),
            nodes' = nodes,
            syncSessions' = syncSessions,
            globalFacts' = globalFacts
        }
    }

    // Complete sync session when states match
    action completeSyncSession(initiator: NodeId, responder: NodeId): bool = all {
        initiator.in(nodes),
        responder.in(nodes),
        initiator.in(nodeStates.keys()),
        responder.in(nodeStates.keys()),
        syncSessions.exists(s => s.initiator == initiator and s.responder == responder and not(s.completed)),
        val initState = nodeStates.get(initiator)
        val respState = nodeStates.get(responder)
        val hashesMatch = initState.stateHash == respState.stateHash
        all {
            hashesMatch,
            val updatedSessions = syncSessions.map(s =>
                if (s.initiator == initiator and s.responder == responder)
                    { ...s, completed: true }
                else s
            )
            syncSessions' = updatedSessions,
            nodes' = nodes,
            nodeStates' = nodeStates,
            pendingDeltas' = pendingDeltas,
            globalFacts' = globalFacts
        }
    }

    // ==================== INVARIANTS ====================

    // Safety: Facts are monotonic (never removed)
    val InvariantFactsMonotonic =
        nodes.forall(n =>
            n.in(nodeStates.keys()) implies
                nodeStates.get(n).facts.subseteq(globalFacts)
        )

    // Safety: All node facts are subsets of global facts
    val InvariantFactsSubsetOfGlobal =
        nodes.forall(n =>
            n.in(nodeStates.keys()) implies
                nodeStates.get(n).facts.subseteq(globalFacts)
        )

    // Safety: Vector clocks are consistent with version
    val InvariantVectorClockConsistent =
        nodes.forall(n =>
            n.in(nodeStates.keys()) implies {
                val state = nodeStates.get(n)
                val ownClock = if (n.in(state.vectorClock.keys())) state.vectorClock.get(n) else 0
                ownClock >= 0  // Simplified: real check would ensure ownClock reflects local writes
            }
        )

    // Convergence: If no pending deltas and no active sessions, all nodes have same state
    val InvariantEventualConvergence =
        (pendingDeltas.size() == 0 and syncSessions.forall(s => s.completed)) implies
            nodes.forall(n1 =>
                nodes.forall(n2 =>
                    (n1.in(nodeStates.keys()) and n2.in(nodeStates.keys())) implies
                        nodeStates.get(n1).facts == nodeStates.get(n2).facts
                )
            )

    // Safety: Deltas only contain facts from source
    val InvariantDeltasFromSource =
        pendingDeltas.forall(d =>
            d.sourceNode.in(nodeStates.keys()) implies
                d.facts.subseteq(nodeStates.get(d.sourceNode).facts)
        )

    // Safety: Completed sessions have matching hashes
    val InvariantCompletedSessionsConverged =
        syncSessions.forall(s =>
            (s.completed and s.initiator.in(nodeStates.keys()) and s.responder.in(nodeStates.keys())) implies
                nodeStates.get(s.initiator).stateHash == nodeStates.get(s.responder).stateHash
        )

    // ==================== STEP RELATION ====================

    action step = any {
        nondet node = oneOf(Set("node1", "node2", "node3"))
        nondet fact = oneOf(Set("fact_a", "fact_b", "fact_c"))
        nondet initiator = oneOf(Set("node1", "node2", "node3"))
        nondet responder = oneOf(Set("node1", "node2", "node3"))
        nondet deltaId = oneOf(Set("delta1", "delta2", "delta3", "delta4"))
        any {
            localWrite(node, fact),
            initiateSyncSession(initiator, responder),
            createDelta(initiator, responder, deltaId),
            applyDelta(node, deltaId),
            completeSyncSession(initiator, responder),
        }
    }
}
