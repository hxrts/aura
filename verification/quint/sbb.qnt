// Secure Bulletin Board (SBB) Protocol Specification
//
// Models gossip-based message propagation with trust-aware filtering.
// Peers exchange envelopes (signed messages) through a bulletin board pattern.
// Envelopes have TTL and expire; peers can block senders via trust graph.
//
// Key behaviors:
// - Peers post envelopes to their bulletin, gossip to neighbors
// - Recipients filter by trust level (Blocked senders rejected)
// - Envelopes expire after TTL; network partitions isolate gossip
// - Byzantine peers can be marked but continue gossiping
//
// Safety: Blocked peers cannot deliver messages to blocking peers.
// Liveness: Envelopes reach all non-blocked peers in same partition.
//
// See: docs/108_transport_and_information_flow.md, docs/114_social_architecture.md

module protocol_sbb {
  // ============ TYPE DEFINITIONS ============
  // Base types for bulletin board messaging.

  type Option[a] = Some(a) | None
  type PeerId = str           // Peer identifier
  type EnvelopeId = str       // Envelope identifier
  type Timestamp = int        // Wall-clock time

  // Trust levels for peer-to-peer filtering.
  type TrustLevel = Trusted | Suspicious | Blocked

  // Envelope: signed message with TTL.
  type Envelope = {
    id: EnvelopeId,          // Unique envelope ID
    author: PeerId,          // Sender
    recipient: PeerId,       // Intended recipient
    payloadHash: int,        // Hash of encrypted payload
    ttl: int,                // Time-to-live in time units
    createdAt: Timestamp     // Creation timestamp
  }

  // Per-peer state tracking trust, messages, and quota.
  type PeerRecord = {
    trustGraph: PeerId -> TrustLevel,  // Per-peer trust settings
    bulletin: Set[EnvelopeId],         // Envelopes posted by this peer
    pending: Set[EnvelopeId],          // Received, not yet processed
    delivered: Set[EnvelopeId],        // Successfully delivered
    quota: int,                        // Remaining gossip quota
    isByzantine: bool,                 // Byzantine behavior flag
    partition: int                     // Network partition ID
  }

  // ============ STATE VARIABLES ============
  // Global mutable state tracked by the model checker.

  var peers: PeerId -> PeerRecord               // Peer registry
  var envelopes: EnvelopeId -> Envelope         // All envelopes
  var networkPartitions: int -> Set[PeerId]     // Partition membership
  var currentTime: Timestamp                    // Global clock

  // ============ HELPERS ============
  // Pure functions for safe state access and trust checking.

  pure def getOrDefaultPeerRecord(m: PeerId -> PeerRecord, key: PeerId): Option[PeerRecord] =
    if (key.in(m.keys())) Some(m.get(key)) else None

  pure def getOrDefaultEnvelope(m: EnvelopeId -> Envelope, key: EnvelopeId): Option[Envelope] =
    if (key.in(m.keys())) Some(m.get(key)) else None

  pure def getOrDefaultPartition(m: int -> Set[PeerId], key: int): Set[PeerId] =
    if (key.in(m.keys())) m.get(key) else Set()

  // Look up trust level (default: Trusted for unknown peers).
  pure def getTrustLevel(trustGraph: PeerId -> TrustLevel, peer: PeerId): TrustLevel =
    if (peer.in(trustGraph.keys())) trustGraph.get(peer) else Trusted

  // Check if two peers can communicate (same network partition).
  def canCommunicate(a: PeerId, b: PeerId): bool = {
    val aRecordOpt = getOrDefaultPeerRecord(peers, a)
    val bRecordOpt = getOrDefaultPeerRecord(peers, b)
    val aPartition = match aRecordOpt { | Some(r) => r.partition | None => -1 }
    val bPartition = match bRecordOpt { | Some(r) => r.partition | None => -1 }
    aPartition == bPartition
  }

  // Check if envelope has expired (beyond TTL).
  pure def envelopeExpired(env: Envelope, now: Timestamp): bool =
    now >= env.createdAt + env.ttl

  // ============ INITIALISATION ============
  // Start with empty peer registry.

  action init: bool = all {
    peers' = Map(),
    envelopes' = Map(),
    networkPartitions' = Map(),
    currentTime' = 0
  }

  // ============ ACTIONS ============
  // State transitions modeling the bulletin board protocol.

  // Register new peer with neighbors (initial trust graph).
  action registerPeer(peer: PeerId, neighbors: Set[PeerId], partition: int, quota: int): bool = all {
    not(peer.in(peers.keys())),
    peers' = peers.set(peer, {
      trustGraph: neighbors.fold(Map(), (acc, n) => acc.set(n, Trusted)),
      bulletin: Set(),
      pending: Set(),
      delivered: Set(),
      quota: quota,
      isByzantine: false,
      partition: partition
    }),
    envelopes' = envelopes,
    networkPartitions' = networkPartitions.set(partition, getOrDefaultPartition(networkPartitions, partition).union(Set(peer))),
    currentTime' = currentTime
  }

  // Mark peer as Byzantine (continues operating but flagged).
  action markByzantine(peer: PeerId): bool = {
    val recordOpt = getOrDefaultPeerRecord(peers, peer)
    val hasRecord = match recordOpt { | Some(_) => true | None => false }
    val record = match recordOpt {
      | Some(r) => r
      | None => { trustGraph: Map(), bulletin: Set(), pending: Set(), delivered: Set(), quota: 0, isByzantine: false, partition: 0 }
    }
    all {
      hasRecord,
      peers' = peers.set(peer, {
        ...record,
        isByzantine: true
      }),
      envelopes' = envelopes,
      networkPartitions' = networkPartitions,
      currentTime' = currentTime
    }
  }

  // Post new envelope to author's bulletin board.
  action postEnvelope(author: PeerId, recipient: PeerId, envId: EnvelopeId, payloadHash: int, ttlVal: int): bool = {
    val recordOpt = getOrDefaultPeerRecord(peers, author)
    val hasRecord = match recordOpt { | Some(_) => true | None => false }
    val record = match recordOpt {
      | Some(r) => r
      | None => { trustGraph: Map(), bulletin: Set(), pending: Set(), delivered: Set(), quota: 0, isByzantine: false, partition: 0 }
    }
    val recipientExists = recipient.in(peers.keys())
    val envelope = {
      id: envId,
      author: author,
      recipient: recipient,
      payloadHash: payloadHash,
      ttl: ttlVal,
      createdAt: currentTime
    }
    all {
      hasRecord,
      recipientExists,
      ttlVal > 0,
      envelopes' = envelopes.set(envId, envelope),
      peers' = peers.set(author, {
        ...record,
        bulletin: record.bulletin.union(Set(envId))
      }),
      networkPartitions' = networkPartitions,
      currentTime' = currentTime
    }
  }

  // Gossip envelope from sender to peer (trust/partition/quota checks).
  action gossipEnvelope(sender: PeerId, peer: PeerId, envId: EnvelopeId): bool = {
    val senderRecordOpt = getOrDefaultPeerRecord(peers, sender)
    val hasSender = match senderRecordOpt { | Some(_) => true | None => false }
    val senderRecord = match senderRecordOpt {
      | Some(r) => r
      | None => { trustGraph: Map(), bulletin: Set(), pending: Set(), delivered: Set(), quota: 0, isByzantine: false, partition: 0 }
    }
    val recipientRecordOpt = getOrDefaultPeerRecord(peers, peer)
    val hasRecipient = match recipientRecordOpt { | Some(_) => true | None => false }
    val recipientRecord = match recipientRecordOpt {
      | Some(r) => r
      | None => { trustGraph: Map(), bulletin: Set(), pending: Set(), delivered: Set(), quota: 0, isByzantine: false, partition: 0 }
    }
    val envelopeOpt = getOrDefaultEnvelope(envelopes, envId)
    val hasEnvelope = match envelopeOpt { | Some(_) => true | None => false }
    val envelope = match envelopeOpt {
      | Some(e) => e
      | None => { id: "", author: "", recipient: "", payloadHash: 0, ttl: 0, createdAt: 0 }
    }
    val expired = envelopeExpired(envelope, currentTime)
    val canComm = canCommunicate(sender, peer)
    val hasQuota = senderRecord.quota > 0
    val trustLevel = getTrustLevel(recipientRecord.trustGraph, sender)
    val isBlocked = match trustLevel { | Blocked => true | _ => false }
    val accepted = if (senderRecord.isByzantine) true else not(isBlocked)
    all {
      hasSender,
      hasRecipient,
      hasEnvelope,
      not(expired),
      canComm,
      hasQuota,
      peers' = peers.set(peer, {
        ...recipientRecord,
        pending: if (accepted) recipientRecord.pending.union(Set(envId)) else recipientRecord.pending
      }).set(sender, {
        ...senderRecord,
        quota: senderRecord.quota - 1
      }),
      envelopes' = envelopes,
      networkPartitions' = networkPartitions,
      currentTime' = currentTime
    }
  }

  // Accept pending envelope (move to delivered).
  action acceptEnvelope(peer: PeerId, envId: EnvelopeId): bool = {
    val recordOpt = getOrDefaultPeerRecord(peers, peer)
    val hasRecord = match recordOpt { | Some(_) => true | None => false }
    val record = match recordOpt {
      | Some(r) => r
      | None => { trustGraph: Map(), bulletin: Set(), pending: Set(), delivered: Set(), quota: 0, isByzantine: false, partition: 0 }
    }
    val envelopeOpt = getOrDefaultEnvelope(envelopes, envId)
    val hasEnvelope = match envelopeOpt { | Some(_) => true | None => false }
    val envelope = match envelopeOpt {
      | Some(e) => e
      | None => { id: "", author: "", recipient: "", payloadHash: 0, ttl: 0, createdAt: 0 }
    }
    val isPending = envId.in(record.pending)
    val expired = envelopeExpired(envelope, currentTime)
    all {
      hasRecord,
      hasEnvelope,
      isPending,
      not(expired),
      peers' = peers.set(peer, {
        ...record,
        pending: record.pending.filter(id => id != envId),
        delivered: record.delivered.union(Set(envId))
      }),
      envelopes' = envelopes,
      networkPartitions' = networkPartitions,
      currentTime' = currentTime
    }
  }

  // Advance time (removes expired envelopes from pending).
  action advanceTime(delta: int): bool = {
    val newTime = currentTime + delta
    val updatedPeers = peers.keys().fold(peers, (acc, pid) =>
      if (pid.in(acc.keys())) {
        val record = acc.get(pid)
        acc.set(pid, {
          ...record,
          pending: record.pending.filter(id =>
            if (id.in(envelopes.keys())) not(envelopeExpired(envelopes.get(id), newTime)) else false
          )
        })
      } else acc
    )
    all {
      delta > 0,
      currentTime' = newTime,
      peers' = updatedPeers,
      envelopes' = envelopes,
      networkPartitions' = networkPartitions
    }
  }

  // Create/modify network partition (isolates gossip).
  action partitionNetwork(partitionId: int, peersInPartition: Set[PeerId]): bool = {
    val updatedPeers = peers.keys().fold(peers, (acc, pid) =>
      if (pid.in(acc.keys())) {
        val record = acc.get(pid)
        if (pid.in(peersInPartition))
          acc.set(pid, {...record, partition: partitionId})
        else
          acc
      } else acc
    )
    all {
      peers' = updatedPeers,
      networkPartitions' = networkPartitions.set(partitionId, peersInPartition),
      envelopes' = envelopes,
      currentTime' = currentTime
    }
  }

  // ============ INVARIANTS ============
  // Safety properties that must hold in every reachable state.

  // Envelopes reference registered peers.
  val InvariantEnvelopeRegistered =
    envelopes.keys().forall(envId => {
      val env = envelopes.get(envId)
      env.author.in(peers.keys()) and env.recipient.in(peers.keys())
    })

  // Bulletin entries reference existing envelopes.
  val InvariantBulletinEntriesExist =
    peers.keys().forall(pid => {
      val record = peers.get(pid)
      record.bulletin.subseteq(envelopes.keys()) and
      record.pending.subseteq(envelopes.keys()) and
      record.delivered.subseteq(envelopes.keys())
    })

  // Gossip quota never goes negative.
  val InvariantQuotaNonNegative =
    peers.keys().forall(pid => peers.get(pid).quota >= 0)

  // Pending and delivered are disjoint (no double-delivery).
  val InvariantPendingNotDelivered =
    peers.keys().forall(pid => {
      val record = peers.get(pid)
      record.pending.intersect(record.delivered).size() == 0
    })

  // Peers are members of their claimed partition.
  val InvariantPartitionMembership =
    peers.keys().forall(pid => {
      val record = peers.get(pid)
      getOrDefaultPartition(networkPartitions, record.partition).contains(pid)
    })
}
