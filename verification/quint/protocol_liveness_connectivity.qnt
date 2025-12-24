// Protocol Liveness Connectivity - Gossip Graph Model and Partition Actions
//
// This module defines the gossip graph connectivity model for slow path robustness
// analysis. The slow path can complete when k honest witnesses are in the same
// connected component, regardless of timing.
//
// See: docs/004_distributed_systems_contract.md §4, docs/104_consensus.md §16

module protocol_liveness_connectivity {
    import protocol_consensus as core from "protocol_consensus"
    import protocol_liveness_timing as timing from "protocol_liveness_timing"

    // ════════════════════════════════════════════════════════════════════════
    // CONNECTIVITY HELPERS
    //
    // Functions for analyzing the gossip graph topology.
    // Used to determine whether quorum of honest nodes can communicate.
    // ════════════════════════════════════════════════════════════════════════

    // Get neighbors of a node in the gossip graph
    pure def neighbors(
        graph: core::AuthorityId -> Set[core::AuthorityId],
        node: core::AuthorityId
    ): Set[core::AuthorityId] =
        if (node.in(graph.keys())) graph.get(node) else Set()

    // Check if two nodes are directly connected (edge exists)
    pure def areDirectlyConnected(
        graph: core::AuthorityId -> Set[core::AuthorityId],
        a: core::AuthorityId,
        b: core::AuthorityId
    ): bool =
        b.in(neighbors(graph, a)) or a.in(neighbors(graph, b))

    // Expand a set of nodes by one hop in the graph
    pure def expandOneHop(
        graph: core::AuthorityId -> Set[core::AuthorityId],
        current: Set[core::AuthorityId]
    ): Set[core::AuthorityId] =
        current.fold(current, (acc, n) => acc.union(neighbors(graph, n)))

    // Compute reachable nodes from a starting set (BFS-like, fixed iterations)
    // Note: This is an approximation - true transitive closure requires recursion
    // We use fixed iterations (sufficient for small witness sets)
    pure def reachableFrom(
        graph: core::AuthorityId -> Set[core::AuthorityId],
        startNodes: Set[core::AuthorityId],
        allNodes: Set[core::AuthorityId]
    ): Set[core::AuthorityId] = {
        // Expand reachability iteratively (3 hops covers most small graphs)
        val step1 = expandOneHop(graph, startNodes)
        val step2 = expandOneHop(graph, step1)
        val step3 = expandOneHop(graph, step2)
        step3.intersect(allNodes)
    }

    // Check if a set of nodes forms a connected component
    // (every node can reach every other node through the graph)
    pure def isConnectedSet(
        graph: core::AuthorityId -> Set[core::AuthorityId],
        nodes: Set[core::AuthorityId]
    ): bool =
        if (nodes.size() == 0) true
        else {
            // Pick any node, check if it can reach all others
            val anyNode = nodes.fold("", (acc, n) => if (acc == "") n else acc)
            val reachable = reachableFrom(graph, Set(anyNode), nodes)
            nodes.subseteq(reachable)
        }

    // Get the connected component containing a specific node
    pure def connectedComponent(
        graph: core::AuthorityId -> Set[core::AuthorityId],
        node: core::AuthorityId,
        allNodes: Set[core::AuthorityId]
    ): Set[core::AuthorityId] =
        reachableFrom(graph, Set(node), allNodes)

    // Check if the graph is fully connected (all nodes in one component)
    pure def isFullyConnected(
        graph: core::AuthorityId -> Set[core::AuthorityId],
        nodes: Set[core::AuthorityId]
    ): bool =
        isConnectedSet(graph, nodes)

    // ════════════════════════════════════════════════════════════════════════
    // QUORUM-IN-COMPONENT HELPERS
    //
    // Helpers for checking if sufficient honest nodes are connected.
    // ════════════════════════════════════════════════════════════════════════

    // Check if k honest witnesses are in the same connected component
    pure def hasQuorumInConnectedComponent(
        inst: core::ConsensusInstance,
        participation: core::AuthorityId -> timing::WitnessParticipation,
        graph: core::AuthorityId -> Set[core::AuthorityId]
    ): bool = {
        val onlineHonest = inst.witnesses.filter(w =>
            w.in(participation.keys()) and
            participation.get(w).isOnline and
            participation.get(w).isHonest
        )
        // Check if there's a connected subset of size >= threshold
        onlineHonest.size() >= inst.threshold and isConnectedSet(graph, onlineHonest)
    }

    // Find the largest connected component of honest online witnesses
    pure def largestHonestConnectedComponent(
        witnesses: Set[core::AuthorityId],
        participation: core::AuthorityId -> timing::WitnessParticipation,
        graph: core::AuthorityId -> Set[core::AuthorityId]
    ): Set[core::AuthorityId] = {
        val onlineHonest = witnesses.filter(w =>
            w.in(participation.keys()) and
            participation.get(w).isOnline and
            participation.get(w).isHonest
        )
        // Find largest component by checking each node's component
        // (For small graphs this is acceptable; larger graphs would need optimization)
        onlineHonest.fold(Set(), (largest, w) => {
            val component = connectedComponent(graph, w, onlineHonest)
            if (component.size() > largest.size()) component else largest
        })
    }

    // ════════════════════════════════════════════════════════════════════════
    // PARTITION ACTION HELPERS
    //
    // Pure functions for computing partition state changes.
    // The stateful actions are in the main module.
    // ════════════════════════════════════════════════════════════════════════

    // Compute graph after removing a link
    pure def graphWithoutLink(
        graph: core::AuthorityId -> Set[core::AuthorityId],
        a: core::AuthorityId,
        b: core::AuthorityId
    ): core::AuthorityId -> Set[core::AuthorityId] =
        graph.put(a, neighbors(graph, a).exclude(Set(b)))

    // Compute graph after adding a link
    pure def graphWithLink(
        graph: core::AuthorityId -> Set[core::AuthorityId],
        a: core::AuthorityId,
        b: core::AuthorityId
    ): core::AuthorityId -> Set[core::AuthorityId] =
        graph.put(a, neighbors(graph, a).union(Set(b)))

    // Compute graph after partitioning: isolate a set from the rest
    pure def graphWithPartition(
        graph: core::AuthorityId -> Set[core::AuthorityId],
        isolated: Set[core::AuthorityId]
    ): core::AuthorityId -> Set[core::AuthorityId] = {
        val remaining = graph.keys().exclude(isolated)
        graph.keys().fold(Map(), (acc, node) =>
            if (node.in(isolated))
                // Isolated nodes only connect to other isolated nodes
                acc.put(node, neighbors(graph, node).intersect(isolated))
            else
                // Non-isolated nodes only connect to other non-isolated nodes
                acc.put(node, neighbors(graph, node).intersect(remaining))
        )
    }

    // Compute fully connected graph
    pure def fullyConnectedGraph(
        nodes: Set[core::AuthorityId]
    ): core::AuthorityId -> Set[core::AuthorityId] =
        nodes.fold(Map(), (acc, w) =>
            acc.put(w, nodes.exclude(Set(w)))
        )
}
