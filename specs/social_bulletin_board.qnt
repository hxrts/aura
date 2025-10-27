// Social Bulletin Board (SBB) Web-of-Trust and Peer Discovery Specification
// Models gossip-based peer discovery with trust propagation
module social_bulletin_board {
  // ==================== TYPES ====================
  
  type PeerId = str
  type Envelope = {
    id: str,
    author: PeerId,
    recipient: PeerId,
    content: str,
    timestamp: int,
    signature: int
  }
  
  type TrustLevel = 
    | Trusted
    | Known
    | Unknown
    | Blocked
  
  type PeerState = {
    id: PeerId,
    trustGraph: PeerId -> TrustLevel,
    bulletin: Set[Envelope],
    pendingGossip: Set[Envelope],
    gossipPartners: Set[PeerId],
    lastSeen: PeerId -> int,
    rateLimit: PeerId -> int,  // Messages received per time window
    isByzantine: bool
  }
  
  type GossipMessage = {
    from: PeerId,
    to: PeerId,
    envelopes: Set[Envelope],
    trustUpdates: PeerId -> TrustLevel
  }
  
  type NetworkState = {
    latency: str -> int,  // Combined peer ids as key
    partitioned: Set[Set[PeerId]],  // Network partitions
    droppedMessages: int
  }
  
  // ==================== STATE ====================
  
  var peers: PeerId -> PeerState
  var network: NetworkState
  var globalTime: int
  var gossipMessages: Set[GossipMessage]
  var deliveredEnvelopes: PeerId -> Set[str]  // envelope.id per peer

  pure def unchangedExcept(vars: Set[str]): bool = all {
    if ("peers".in(vars)) true else peers' = peers,
    if ("network".in(vars)) true else network' = network,
    if ("globalTime".in(vars)) true else globalTime' = globalTime,
    if ("gossipMessages".in(vars)) true else gossipMessages' = gossipMessages,
    if ("deliveredEnvelopes".in(vars)) true else deliveredEnvelopes' = deliveredEnvelopes
  }
  
  // ==================== CONSTANTS ====================
  
  pure val GOSSIP_INTERVAL = 10
  pure val RATE_LIMIT_WINDOW = 100
  pure val RATE_LIMIT_MAX = 50
  pure val TRUST_TRANSITIVITY_DEPTH = 3
  pure val MAX_ENVELOPE_AGE = 1000
  
  // ==================== INITIALIZATION ====================
  
  action init: bool = all {
    peers' = Map(),
    network' = {
      latency: Map(),
      partitioned: Set(),
      droppedMessages: 0
    },
    globalTime' = 0,
    gossipMessages' = Set(),
    deliveredEnvelopes' = Map()
  }
  
  // ==================== PEER MANAGEMENT ====================
  
  action addPeer(peerId: PeerId): bool = all {
    not(peerId.in(peers.keys())),
    peers' = peers.set(peerId, {
      id: peerId,
      trustGraph: Map().set(peerId, Trusted),  // Self-trust
      bulletin: Set(),
      pendingGossip: Set(),
      gossipPartners: Set(),
      lastSeen: Map(),
      rateLimit: Map(),
      isByzantine: false
    }),
    deliveredEnvelopes' = deliveredEnvelopes.set(peerId, Set()),
    unchangedExcept(Set("peers", "deliveredEnvelopes"))
  }
  
  action markByzantine(peerId: PeerId): bool = 
    val peer = peers.get(peerId) {
      all {
        peerId.in(peers.keys()),
        peers' = peers.set(peerId, {...peer, isByzantine: true}),
        unchangedExcept(Set("peers"))
      }
    }
  
  // ==================== TRUST MANAGEMENT ====================
  
  action updateTrust(fromPeer: PeerId, toPeer: PeerId, level: TrustLevel): bool = 
    val peer = peers.get(fromPeer) {
      all {
        fromPeer.in(peers.keys()),
        toPeer.in(peers.keys()),
        fromPeer != toPeer,
        peers' = peers.set(fromPeer, {
          ...peer,
          trustGraph: peer.trustGraph.set(toPeer, level)
        }),
        unchangedExcept(Set("peers"))
      }
    }
  
  action propagateTrust(peerId: PeerId): bool = 
    val peer = peers.get(peerId) {
    // Compute transitive trust
    val newTrust = peer.trustGraph.keys().fold(peer.trustGraph, (acc, p1) =>
      if (peer.trustGraph.get(p1) == Trusted)
        if (p1.in(peers.keys()))
          val p1Trust = peers.get(p1).trustGraph {
            p1Trust.keys().fold(acc, (acc2, p2) =>
              if (not(p2.in(acc2.keys())) and p1Trust.get(p2) == Trusted)
                acc2.set(p2, Known)  // Transitive trust becomes Known
              else
                acc2
            )
          }
        else
          acc
      else
        acc
    ) {
      all {
        peerId.in(peers.keys()),
        peers' = peers.set(peerId, {...peer, trustGraph: newTrust}),
        unchangedExcept(Set("peers"))
      }
    }}
  
  // ==================== ENVELOPE CREATION ====================
  
  action createEnvelope(author: PeerId, recipient: PeerId, content: str): bool = 
    val peer = peers.get(author) {
    val nonce = 1.to(10000).oneOf() {  // Random nonce for uniqueness
    val envelopeId = generateEnvelopeId(author, recipient, globalTime, nonce) {
    val envelope = {
      id: envelopeId,
      author: author,
      recipient: recipient,
      content: if (peer.isByzantine) 
        Set("spam", "legitimate", "malicious").oneOf()  // Byzantine content
      else 
        content,
      timestamp: globalTime,
      signature: 0  // Will be set below
    } {
    val secretKey = author.length() * 101 {  // Simulate per-peer secret key
    val signature = if (peer.isByzantine)
      0.to(1000).oneOf()  // Byzantine signature
    else
      signEnvelope(author, envelope, secretKey) {
    val signedEnvelope = {...envelope, signature: signature} {
      all {
        author.in(peers.keys()),
        recipient.in(peers.keys()),
        peers' = peers.set(author, {
          ...peer,
          bulletin: peer.bulletin.union(Set(signedEnvelope)),
          pendingGossip: peer.pendingGossip.union(Set(signedEnvelope))
        }),
        unchangedExcept(Set("peers"))
      }
    }}}}}}}}
  
  // ==================== GOSSIP PROTOCOL ====================
  
  action selectGossipPartners(peerId: PeerId): bool = 
    val peer = peers.get(peerId) {
    // Select trusted and known peers for gossip
    val candidates = peer.trustGraph.keys().filter(p =>
      peer.trustGraph.get(p).in(Set(Trusted, Known))
    ) {
    // Simple selection - take first 3 from filtered set
    val selected = candidates.fold(Set(), (acc, p) =>
      if (acc.size() < 3) acc.union(Set(p)) else acc
    ) {
      all {
        peerId.in(peers.keys()),
        peers' = peers.set(peerId, {
          ...peer,
          gossipPartners: selected
        }),
        unchangedExcept(Set("peers"))
      }
    }}}
  
  action initiateGossip(fromPeer: PeerId, toPeer: PeerId): bool = 
    val peer = peers.get(fromPeer) {
    // Check rate limit
    val currentRate = if (fromPeer.in(peer.rateLimit.keys()))
      peer.rateLimit.get(fromPeer)
    else 
      0 {
    // Create limited trust updates
    val trustedPeers = peer.trustGraph.keys()
      .filter(p => peer.trustGraph.get(p) == Trusted)
      .fold(Set(), (acc, p) =>
        if (acc.size() < 5) acc.union(Set(p)) else acc
      ) {
    val trustUpdates = trustedPeers.mapBy(p => peer.trustGraph.get(p)) {
    // Create gossip message
    val msg = {
      from: fromPeer,
      to: toPeer,
      envelopes: peer.pendingGossip,
      trustUpdates: trustUpdates
    } {
      all {
        fromPeer.in(peers.keys()),
        toPeer.in(peers.keys()),
        toPeer.in(peer.gossipPartners),
        peer.pendingGossip.size() > 0,
        currentRate < RATE_LIMIT_MAX,
        gossipMessages' = gossipMessages.union(Set(msg)),
        // Clear pending for gossiped envelopes
        peers' = peers.set(fromPeer, {
          ...peer,
          pendingGossip: Set()
        }),
        unchangedExcept(Set("gossipMessages", "peers"))
      }
    }}}}}}
  
  action receiveGossip(msg: GossipMessage): bool = 
    val receiver = peers.get(msg.to) {
    // Check trust level of sender
    val senderTrust = if (msg.from.in(receiver.trustGraph.keys()))
      receiver.trustGraph.get(msg.from)
    else
      Unknown {
    // Update rate limit
    val newRate = if (msg.from.in(receiver.rateLimit.keys()))
      receiver.rateLimit.get(msg.from) + msg.envelopes.size()
    else
      msg.envelopes.size() {
    // Filter envelopes based on trust and age
    val acceptedEnvelopes = msg.envelopes.filter(e =>
      globalTime - e.timestamp < MAX_ENVELOPE_AGE and
      (senderTrust == Trusted or verifyEnvelope(e))
    ) {
    // Update receiver state
    val newBulletin = receiver.bulletin.union(acceptedEnvelopes) {
    val newPending = receiver.pendingGossip.union(
      acceptedEnvelopes.exclude(receiver.bulletin)
    ) {
    val newLastSeen = receiver.lastSeen.set(msg.from, globalTime) {
    val newRateLimit = receiver.rateLimit.set(msg.from, newRate) {
    // Apply trust updates if sender is trusted
    val newTrustGraph = if (senderTrust == Trusted)
      msg.trustUpdates.keys().fold(receiver.trustGraph, (acc, p) =>
        if (not(p.in(acc.keys())))
          acc.set(p, Known)  // Recommended peers become Known
        else
          acc
      )
    else
      receiver.trustGraph {
      all {
        msg.in(gossipMessages),
        msg.to.in(peers.keys()),
        senderTrust != Blocked,
        newRate <= RATE_LIMIT_MAX,
        peers' = peers.set(msg.to, {
          ...receiver,
          bulletin: newBulletin,
          pendingGossip: newPending,
          lastSeen: newLastSeen,
          rateLimit: newRateLimit,
          trustGraph: newTrustGraph
        }),
        deliveredEnvelopes' = deliveredEnvelopes.set(
          msg.to,
          deliveredEnvelopes.get(msg.to).union(acceptedEnvelopes.map(e => e.id))
        ),
        gossipMessages' = gossipMessages.exclude(Set(msg)),
        unchangedExcept(Set("peers", "deliveredEnvelopes", "gossipMessages"))
      }
    }}}}}}}}}
  
  // ==================== NETWORK DYNAMICS ====================
  
  action createPartition(partition1: Set[PeerId], partition2: Set[PeerId]): bool = all {
    partition1.intersect(partition2).size() == 0,
    partition1.union(partition2).subseteq(peers.keys()),
    network' = {
      ...network,
      partitioned: network.partitioned.union(Set(partition1, partition2))
    },
    unchangedExcept(Set("network"))
  }

  action healPartition: bool = all {
    network.partitioned.size() > 0,
    network' = {...network, partitioned: Set()},
    unchangedExcept(Set("network"))
  }

  action advanceTime(delta: int): bool = all {
    delta > 0,
    globalTime' = globalTime + delta,
    // Decay rate limits
    peers' = peers.keys().mapBy(pId =>
      val peer = peers.get(pId) {
      val decayedRates = peer.rateLimit.keys().mapBy(p =>
        max(0, peer.rateLimit.get(p) - delta / RATE_LIMIT_WINDOW)
      ) {
        {...peer, rateLimit: decayedRates}
      }}
    ),
    unchangedExcept(Set("globalTime", "peers"))
  }
  
  // ==================== HELPER FUNCTIONS ====================
  
  // ==================== CRYPTOGRAPHIC FUNCTIONS ====================
  
  // Generate envelope ID using BLAKE3
  pure def generateEnvelopeId(author: PeerId, recipient: PeerId, timestamp: int, nonce: int): str = {
    // Real implementation: BLAKE3(author || recipient || timestamp || nonce)
    // Ensures globally unique envelope IDs
    author.concat("_").concat(recipient).concat("_").concat(timestamp.toString()).concat("_").concat(nonce.toString())
  }
  
  // Sign envelope using Ed25519
  pure def signEnvelope(author: PeerId, envelope: Envelope, secretKey: int): int = {
    // Real implementation: Ed25519_sign(secret_key, envelope_hash)
    // envelope_hash = BLAKE3(CBOR(envelope))
    val envelopeData = envelope.id.length() + envelope.content.length() + envelope.timestamp
    val authorKey = author.length() * 67  // Simulate author's key
    (envelopeData * 31 + authorKey * 17 + secretKey) % 2147483647
  }
  
  // Verify envelope signature
  pure def verifyEnvelope(envelope: Envelope): bool = {
    // Real implementation: Ed25519_verify(public_key, signature, envelope_hash)
    val envelopeData = envelope.id.length() + envelope.content.length() + envelope.timestamp
    val expectedSig = (envelopeData * 31 + envelope.author.length() * 17) % 2147483647
    // Check signature validity and timestamp
    abs(envelope.signature - expectedSig) < 1000 and
    envelope.timestamp <= globalTime and
    envelope.timestamp > globalTime - MAX_ENVELOPE_AGE
  }
  
  // Generate trust graph fingerprint for efficient sync
  pure def generateTrustFingerprint(trustGraph: PeerId -> TrustLevel): int = {
    // Real implementation: Merkle root of trust relationships
    // Allows efficient detection of trust graph differences
    trustGraph.keys().fold(0, (acc, peer) =>
      val trust = match trustGraph.get(peer) {
        | Trusted => 3
        | Known => 2
        | Unknown => 1
        | Blocked => 0
      }
      (acc * 7 + peer.length() * 13 + trust * 11) % 2147483647
    )
  }
  
  // Create proof of gossip eligibility
  pure def createGossipProof(fromPeer: PeerId, toPeer: PeerId, trustPath: List[PeerId]): int = {
    // Real implementation: Create proof showing trust path from toPeer to fromPeer
    // Prevents spam by proving gossip eligibility
    val pathHash = trustPath.fold(0, (acc, p) => (acc * 17 + p.length() * 23) % 2147483647)
    (fromPeer.length() * 31 + toPeer.length() * 37 + pathHash) % 2147483647
  }
  
  // Generate rate limit proof using VRF
  pure def generateRateLimitProof(peer: PeerId, timestamp: int, window: int): int = {
    // Real implementation: VRF(peer_key, timestamp/window)
    // Provides verifiable fair rate limiting
    val timeSlot = timestamp / window
    (peer.length() * 41 + timeSlot * 29) % RATE_LIMIT_MAX
  }
  
  pure def abs(x: int): int = if (x < 0) -x else x
  
  // Generate unlinkable pseudonym for privacy
  pure def generatePseudonym(peer: PeerId, context: str, epoch: int): str = {
    // Real implementation: H(peer_key || context || epoch)
    // Provides unlinkability across contexts
    val hash = (peer.length() * 47 + context.length() * 31 + epoch * 13) % 10000
    "anon_".concat(hash.toString())
  }
  
  // Create privacy-preserving bloom filter for content matching
  pure def createContentBloomFilter(content: str, filterSize: int): Set[int] = {
    // Real implementation: Bloom filter of content keywords
    // Allows privacy-preserving content discovery
    val hash1 = (content.length() * 31) % filterSize
    val hash2 = (content.length() * 37) % filterSize
    val hash3 = (content.length() * 41) % filterSize
    Set(hash1, hash2, hash3)
  }
  
  pure def max(a: int, b: int): int = 
    if (a > b) a else b
  
  pure def byzantineCount: int = 
    peers.keys().filter(pId => peers.get(pId).isByzantine).size()
  
  pure def isConnected(p1: PeerId, p2: PeerId): bool = {
    // Check if p1 can reach p2 through trust graph
    if (p1.in(peers.keys()) and p2.in(peers.keys())) {
      val peer1 = peers.get(p1)
      p2.in(peer1.trustGraph.keys()) and
      peer1.trustGraph.get(p2).in(Set(Trusted, Known))
    } else {
      false
    }
  }
  
  pure def deliveryRate(peerId: PeerId): int = {
    if (peerId.in(deliveredEnvelopes.keys())) {
      deliveredEnvelopes.get(peerId).size()
    } else {
      0
    }
  }
  
  // ==================== INVARIANTS ====================
  
  // Safety: Self-trust is maintained
  val SelfTrust = 
    peers.keys().forall(pId =>
      val p = peers.get(pId)
      p.id.in(p.trustGraph.keys()) implies
      p.trustGraph.get(p.id) == Trusted
    )
  
  // Safety: Trust levels are consistent
  val TrustConsistency = 
    peers.keys().forall(pId =>
      val p = peers.get(pId)
      p.trustGraph.keys().forall(trustPeer =>
        p.trustGraph.get(trustPeer).in(Set(Trusted, Known, Unknown, Blocked))
      )
    )
  
  // Safety: Blocked peers don't receive messages
  val BlockedIsolation = 
    gossipMessages.forall(msg =>
      val receiver = peers.get(msg.to)
      msg.from.in(receiver.trustGraph.keys()) implies
      receiver.trustGraph.get(msg.from) != Blocked
    )
  
  // Progress: Envelopes propagate through trust network
  val EnvelopePropagation = 
    peers.keys().forall(pId =>
      val p = peers.get(pId)
      p.pendingGossip.size() > 0 implies
      p.gossipPartners.size() > 0
    )
  
  // Safety: Rate limits are enforced
  val RateLimitEnforcement = 
    peers.keys().forall(pId =>
      val p = peers.get(pId)
      p.rateLimit.keys().forall(ratePeer =>
        val rate = p.rateLimit.get(ratePeer)
        rate >= 0 and rate <= RATE_LIMIT_MAX
      )
    )
  
  // ==================== TEMPORAL PROPERTIES ====================
  
  // Liveness: Trusted envelopes eventually propagate
  temporal TrustedPropagation = 
    always(
      peers.keys().forall(p1Id =>
        val p1 = peers.get(p1Id)
        p1.bulletin.forall(env =>
          peers.keys().forall(p2Id =>
            val p2 = peers.get(p2Id)
            (isConnected(p1.id, p2.id) and not(p2.isByzantine)) implies
            eventually(env.id.in(deliveredEnvelopes.get(p2.id)))
          )
        )
      )
    )
  
  // Safety: Byzantine messages have limited spread
  temporal ByzantineBound = 
    always(
      peers.keys().filter(pId => peers.get(pId).isByzantine).forall(byzId =>
        val byz = peers.get(byzId)
        byz.bulletin.forall(env =>
          // Byzantine envelopes spread only through direct trust
          deliveryRate(byz.id) <= peers.keys()
            .filter(pId => 
              val p = peers.get(pId)
              byz.id.in(p.trustGraph.keys()) and
              p.trustGraph.get(byz.id) == Trusted
            )
            .size()
        )
      )
    )
  
  // Convergence: Connected peers converge to same bulletin
  temporal BulletinConvergence = 
    always(
      eventually(
        peers.keys().forall(p1 =>
          peers.keys().forall(p2 =>
            (isConnected(p1, p2) and isConnected(p2, p1)) implies
            peers.get(p1).bulletin == peers.get(p2).bulletin
          )
        )
      )
    )
  
  // Liveness: Network partitions eventually heal
  temporal PartitionRecovery = 
    always(
      network.partitioned.size() > 0 implies
      eventually(network.partitioned.size() == 0)
    )
  
  // Fairness: All trusted peers get gossip opportunities
  temporal GossipFairness = 
    always(
      peers.keys().forall(pId =>
        val p = peers.get(pId)
        p.gossipPartners.size() > 0 implies
        eventually(
          gossipMessages.exists(msg => msg.from == p.id)
        )
      )
    )
}
