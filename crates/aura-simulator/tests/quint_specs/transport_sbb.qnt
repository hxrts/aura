// Social Bulletin Board (SBB) Transport Protocol
// Formal specification of Aura's gossip-based envelope transport

module TransportSBB {
    import SessionTypes.*

    // ========== SBB Types ==========

    type PeerId = str
    type Cid = str  // Content identifier
    type RoutingTag = bytes
    type RelationshipId = str

    // Trust levels for peer relationships
    type TrustLevel =
        | Direct      // Direct trust relationship
        | OneDegree   // Friend of friend
        | TwoDegree   // Two degrees of separation

    // Envelope header
    type EnvelopeHeader = {
        version: int,
        epoch: int,
        counter: int,
        routingTag: RoutingTag,
        ttlEpochs: int,
        cid: Cid
    }

    // Sealed envelope
    type Envelope = {
        header: EnvelopeHeader,
        ciphertext: bytes  // Encrypted payload
    }

    // Peer authentication info
    type PeerAuthentication = {
        deviceId: DeviceId,
        accountId: AccountId,
        lastAuthenticated: int,
        trustLevel: TrustLevel
    }

    // Peer permissions
    type PeerPermissions = {
        relayPermissions: Set[str],
        communicationPermissions: Set[str],
        storagePermissions: Set[str],
        grantedCapabilities: Set[bytes]
    }

    // Neighbor info for active gossip partners
    type NeighborInfo = {
        peerId: PeerId,
        authentication: PeerAuthentication,
        permissions: PeerPermissions,
        addedAt: int
    }

    // Merge history for rate limiting
    type MergeHistory = {
        lastMergeAt: int,
        mergeCount: int,
        consecutiveFailures: int,
        backoffUntil: int
    }

    // Envelope metadata
    type EnvelopeMetadata = {
        cid: Cid,
        addedAt: int,
        expiresAtEpoch: int,
        sizeBytes: int,
        receivedFrom: PeerId
    }

    // SBB node state
    type SbbNode = {
        nodeId: PeerId,
        activeNeighbors: PeerId -> NeighborInfo,
        knownPeers: PeerId -> PeerAuthentication,
        localEnvelopes: Cid -> EnvelopeMetadata,
        mergeHistory: PeerId -> MergeHistory,
        currentEpoch: int,
        maxActiveNeighbors: int,
        minMergeIntervalMs: int
    }

    // ========== State Variables ==========

    var nodes: PeerId -> SbbNode
    var envelopes: Cid -> Envelope
    var inTransit: Set[(Cid, PeerId, PeerId)]  // (envelope, from, to)
    var currentTime: int
    var currentEpoch: int

    // ========== Helper Functions ==========

    // Compute CID from envelope content
    pure def computeCid(envelope: Envelope): Cid = {
        // Blake3(header || ciphertext)
        "cid_" + envelope.header.counter.toString()
    }

    // Check if peer can merge (rate limiting)
    pure def canMerge(history: MergeHistory, now: int, minInterval: int): bool = {
        now >= history.backoffUntil and
        now - history.lastMergeAt >= minInterval
    }

    // Calculate exponential backoff
    pure def calculateBackoff(failures: int, now: int): int = {
        val backoffMs = 1000 * (2 ^ min(failures, 10)),
        now + backoffMs
    }

    // Check if envelope is expired
    pure def isExpired(metadata: EnvelopeMetadata, epoch: int): bool = {
        epoch > metadata.expiresAtEpoch
    }

    // Get envelope routing scope
    pure def getRoutingScope(routingTag: RoutingTag): Set[PeerId] = {
        // TODO fix - Simplified: routing tag determines which peers should receive
        Set()  // Would compute based on tag
    }

    // Check trust transitivity
    pure def computeTrustLevel(direct: TrustLevel, intermediate: TrustLevel): TrustLevel = {
        match (direct, intermediate) {
            | (Direct, Direct) => OneDegree
            | (Direct, OneDegree) => TwoDegree
            | (OneDegree, Direct) => TwoDegree
            | _ => TwoDegree  // Max distance
        }
    }

    // ========== Actions ==========

    // Publish new envelope
    action publishEnvelope(
        nodeId: PeerId,
        envelope: Envelope,
        ttlEpochs: int
    ): bool = {
        all {
            // Node exists
            nodeId.in(nodes.keys()),
            val node = nodes.get(nodeId),
            // Compute CID
            val cid = computeCid(envelope),
            not(cid.in(envelopes.keys())),
            // Store envelope globally
            envelopes' = envelopes.put(cid, envelope),
            // Add to node's local store
            val metadata = {
                cid: cid,
                addedAt: currentTime,
                expiresAtEpoch: currentEpoch + ttlEpochs,
                sizeBytes: 2048,  // Fixed size for simplicity
                receivedFrom: nodeId
            },
            val newLocalEnvelopes = node.localEnvelopes.put(cid, metadata),
            val newNode = node.with("localEnvelopes", newLocalEnvelopes),
            nodes' = nodes.put(nodeId, newNode),
            // Eager push to neighbors
            val pushTargets = node.activeNeighbors.keys(),
            inTransit' = pushTargets.fold(inTransit, (acc, target) =>
                acc.union(Set((cid, nodeId, target)))
            ),
            currentTime' = currentTime + 1,
            currentEpoch' = currentEpoch
        }
    }

    // Add active neighbor
    action addNeighbor(
        nodeId: PeerId,
        neighborId: PeerId,
        auth: PeerAuthentication,
        permissions: PeerPermissions
    ): bool = {
        all {
            // Both nodes exist
            nodeId.in(nodes.keys()),
            neighborId.in(nodes.keys()),
            val node = nodes.get(nodeId),
            // Not already a neighbor
            not(neighborId.in(node.activeNeighbors.keys())),
            // Under max neighbors limit
            node.activeNeighbors.keys().size() < node.maxActiveNeighbors,
            // Create neighbor info
            val neighbor = {
                peerId: neighborId,
                authentication: auth,
                permissions: permissions,
                addedAt: currentTime
            },
            // Update node
            val newNeighbors = node.activeNeighbors.put(neighborId, neighbor),
            val newHistory = node.mergeHistory.put(neighborId, {
                lastMergeAt: 0,
                mergeCount: 0,
                consecutiveFailures: 0,
                backoffUntil: 0
            }),
            val newNode = node
                .with("activeNeighbors", newNeighbors)
                .with("mergeHistory", newHistory),
            nodes' = nodes.put(nodeId, newNode),
            envelopes' = envelopes,
            inTransit' = inTransit,
            currentTime' = currentTime + 1,
            currentEpoch' = currentEpoch
        }
    }

    // Initiate merge with neighbor
    action initiateMerge(nodeId: PeerId, neighborId: PeerId): bool = {
        all {
            // Nodes exist and are neighbors
            nodeId.in(nodes.keys()),
            val node = nodes.get(nodeId),
            neighborId.in(node.activeNeighbors.keys()),
            val history = node.mergeHistory.get(neighborId),
            // Rate limiting check
            canMerge(history, currentTime, node.minMergeIntervalMs),
            // Get envelopes to sync
            val neighbor = nodes.get(neighborId),
            val toSync = node.localEnvelopes.keys().exclude(neighbor.localEnvelopes.keys()),
            // Add to transit
            inTransit' = toSync.fold(inTransit, (acc, cid) =>
                acc.union(Set((cid, nodeId, neighborId)))
            ),
            // Update merge history
            val newHistory = history
                .with("lastMergeAt", currentTime)
                .with("mergeCount", history.mergeCount + 1),
            val newMergeHistory = node.mergeHistory.put(neighborId, newHistory),
            val newNode = node.with("mergeHistory", newMergeHistory),
            nodes' = nodes.put(nodeId, newNode),
            envelopes' = envelopes,
            currentTime' = currentTime + 1,
            currentEpoch' = currentEpoch
        }
    }

    // Receive envelope from neighbor
    action receiveEnvelope(cid: Cid, fromPeer: PeerId, toPeer: PeerId): bool = {
        all {
            // In transit
            (cid, fromPeer, toPeer).in(inTransit),
            // Envelope exists
            cid.in(envelopes.keys()),
            val envelope = envelopes.get(cid),
            // Destination node exists
            toPeer.in(nodes.keys()),
            val node = nodes.get(toPeer),
            // Not already stored locally
            not(cid.in(node.localEnvelopes.keys())),
            // Store envelope
            val metadata = {
                cid: cid,
                addedAt: currentTime,
                expiresAtEpoch: envelope.header.ttlEpochs + currentEpoch,
                sizeBytes: 2048,
                receivedFrom: fromPeer
            },
            val newLocalEnvelopes = node.localEnvelopes.put(cid, metadata),
            val newNode = node.with("localEnvelopes", newLocalEnvelopes),
            nodes' = nodes.put(toPeer, newNode),
            // Remove from transit
            inTransit' = inTransit.exclude(Set((cid, fromPeer, toPeer))),
            envelopes' = envelopes,
            currentTime' = currentTime + 1,
            currentEpoch' = currentEpoch
        }
    }

    // Record merge failure
    action recordMergeFailure(nodeId: PeerId, neighborId: PeerId): bool = {
        all {
            // Node and neighbor exist
            nodeId.in(nodes.keys()),
            val node = nodes.get(nodeId),
            neighborId.in(node.mergeHistory.keys()),
            val history = node.mergeHistory.get(neighborId),
            // Update history
            val failures = history.consecutiveFailures + 1,
            val newHistory = history
                .with("consecutiveFailures", failures)
                .with("backoffUntil", calculateBackoff(failures, currentTime)),
            val newMergeHistory = node.mergeHistory.put(neighborId, newHistory),
            // Demote to known peer if too many failures
            val newNode = if (failures >= 3) {
                val newNeighbors = node.activeNeighbors.mapRemove(neighborId),
                val auth = node.activeNeighbors.get(neighborId).authentication,
                val newKnown = node.knownPeers.put(neighborId, auth),
                node.with("activeNeighbors", newNeighbors)
                    .with("knownPeers", newKnown)
                    .with("mergeHistory", newMergeHistory)
            } else {
                node.with("mergeHistory", newMergeHistory)
            },
            nodes' = nodes.put(nodeId, newNode),
            envelopes' = envelopes,
            inTransit' = inTransit,
            currentTime' = currentTime + 1,
            currentEpoch' = currentEpoch
        }
    }

    // Garbage collect expired envelopes
    action gcExpiredEnvelopes(nodeId: PeerId): bool = {
        all {
            // Node exists
            nodeId.in(nodes.keys()),
            val node = nodes.get(nodeId),
            // Filter out expired envelopes
            val activeEnvelopes = node.localEnvelopes.filter((cid, metadata) =>
                not(isExpired(metadata, currentEpoch))
            ),
            // Update node
            val newNode = node.with("localEnvelopes", activeEnvelopes),
            nodes' = nodes.put(nodeId, newNode),
            envelopes' = envelopes,
            inTransit' = inTransit,
            currentTime' = currentTime + 1,
            currentEpoch' = currentEpoch
        }
    }

    // Discover peers from neighbor
    action discoverPeers(nodeId: PeerId, neighborId: PeerId, discoveredPeers: Set[PeerId]): bool = {
        all {
            // Nodes exist and are neighbors
            nodeId.in(nodes.keys()),
            val node = nodes.get(nodeId),
            neighborId.in(node.activeNeighbors.keys()),
            val neighborAuth = node.activeNeighbors.get(neighborId).authentication,
            // Add discovered peers with transitive trust
            val newKnownPeers = discoveredPeers
                .exclude(node.activeNeighbors.keys())
                .exclude(node.knownPeers.keys())
                .exclude(Set(nodeId))
                .fold(node.knownPeers, (acc, peerId) =>
                    acc.put(peerId, {
                        deviceId: "",
                        accountId: "",
                        lastAuthenticated: currentTime,
                        trustLevel: computeTrustLevel(neighborAuth.trustLevel, Direct)
                    })
                ),
            // Update node
            val newNode = node.with("knownPeers", newKnownPeers),
            nodes' = nodes.put(nodeId, newNode),
            envelopes' = envelopes,
            inTransit' = inTransit,
            currentTime' = currentTime + 1,
            currentEpoch' = currentEpoch
        }
    }

    // ========== Invariants ==========

    // Envelope integrity: CIDs match content
    invariant envelopeIntegrity = {
        envelopes.keys().forall(cid =>
            computeCid(envelopes.get(cid)) == cid
        )
    }

    // Neighbor symmetry
    invariant neighborSymmetry = {
        nodes.values().forall(node =>
            node.activeNeighbors.keys().forall(neighborId =>
                nodes.keys().contains(neighborId) implies
                    nodes.get(neighborId).activeNeighbors.keys().contains(node.nodeId)
            )
        )
    }

    // Active neighbor limit
    invariant activeNeighborLimit = {
        nodes.values().forall(node =>
            node.activeNeighbors.keys().size() <= node.maxActiveNeighbors
        )
    }

    // No duplicate envelopes in transit
    invariant noDuplicateTransit = {
        inTransit.forall(transit1 =>
            inTransit.forall(transit2 =>
                (transit1 == transit2) or
                (transit1._1 != transit2._1 or transit1._3 != transit2._3)
            )
        )
    }

    // Trust level consistency
    invariant trustLevelConsistency = {
        nodes.values().forall(node =>
            node.knownPeers.values().forall(auth =>
                auth.trustLevel.in(Set(Direct, OneDegree, TwoDegree))
            )
        )
    }

    // ========== Properties ==========

    // Eventual delivery: envelopes reach all neighbors
    property eventualDelivery = {
        always(envelopes.keys().forall(cid =>
            nodes.values().forall(node =>
                // If envelope is published at a neighbor
                node.activeNeighbors.keys().exists(neighborId =>
                    nodes.get(neighborId).localEnvelopes.keys().contains(cid)
                ) implies
                    // Eventually node receives it (if not expired)
                    eventually(
                        node.localEnvelopes.keys().contains(cid) or
                        currentEpoch > envelopes.get(cid).header.ttlEpochs + currentEpoch
                    )
            )
        ))
    }

    // Rate limiting effectiveness
    property rateLimitingProperty = {
        always(nodes.values().forall(node =>
            node.mergeHistory.values().forall(history =>
                // Can't merge if within interval or in backoff
                (currentTime < history.lastMergeAt + node.minMergeIntervalMs or
                 currentTime < history.backoffUntil) implies
                    not(inTransit.exists(t => t._2 == node.nodeId))
            )
        ))
    }

    // Gossip convergence: all neighbors eventually have same envelopes
    property gossipConvergence = {
        always(nodes.keys().forall(nodeId1 =>
            nodes.keys().forall(nodeId2 =>
                val node1 = nodes.get(nodeId1),
                val node2 = nodes.get(nodeId2),
                // If they are neighbors
                nodeId2.in(node1.activeNeighbors.keys()) implies
                    // Their non-expired envelopes converge
                    eventually(
                        node1.localEnvelopes.filter((cid, m) => not(isExpired(m, currentEpoch))).keys() ==
                        node2.localEnvelopes.filter((cid, m) => not(isExpired(m, currentEpoch))).keys()
                    )
            )
        ))
    }

    // Exponential backoff on failures
    property exponentialBackoff = {
        always(nodes.values().forall(node =>
            node.mergeHistory.values().forall(history =>
                history.consecutiveFailures > 0 implies
                    history.backoffUntil >= currentTime + 1000 * (2 ^ min(history.consecutiveFailures, 10))
            )
        ))
    }

    // Peer discovery transitivity
    property peerDiscoveryTransitivity = {
        always(nodes.values().forall(node =>
            node.knownPeers.values().forall(peer =>
                // Trust level degrades with distance
                peer.trustLevel == OneDegree or peer.trustLevel == TwoDegree
            )
        ))
    }

    // ========== Initial State ==========

    action init = {
        all {
            nodes' = Map(
                "peer1" -> {
                    nodeId: "peer1",
                    activeNeighbors: Map(),
                    knownPeers: Map(),
                    localEnvelopes: Map(),
                    mergeHistory: Map(),
                    currentEpoch: 0,
                    maxActiveNeighbors: 8,
                    minMergeIntervalMs: 1000
                },
                "peer2" -> {
                    nodeId: "peer2",
                    activeNeighbors: Map(),
                    knownPeers: Map(),
                    localEnvelopes: Map(),
                    mergeHistory: Map(),
                    currentEpoch: 0,
                    maxActiveNeighbors: 8,
                    minMergeIntervalMs: 1000
                }
            ),
            envelopes' = Map(),
            inTransit' = Set(),
            currentTime' = 0,
            currentEpoch' = 0
        }
    }

    // ========== Utility Functions ==========

    pure def min(a: int, b: int): int = if (a < b) a else b
}
