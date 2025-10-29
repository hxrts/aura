module protocol_sbb {
  import protocol_core as core from "protocol_core"

  type PeerId = core.DeviceId
  type EnvelopeId = str
  type Timestamp = int

  type TrustLevel = Trusted | Suspicious | Blocked

  type Envelope = {
    id: EnvelopeId,
    author: PeerId,
    recipient: PeerId,
    payloadHash: int,
    ttl: int,
    createdAt: Timestamp
  }

  type PeerRecord = {
    trustGraph: PeerId -> TrustLevel,
    bulletin: Set[EnvelopeId],
    pending: Set[EnvelopeId],
    delivered: Set[EnvelopeId],
    quota: int,
    isByzantine: bool,
    partition: int
  }

  var peers: PeerId -> PeerRecord
  var envelopes: EnvelopeId -> Envelope
  var networkPartitions: Map[int, Set[PeerId]]
  var currentTime: Timestamp

  pure def getOrDefault[K, V](m: K -> V, key: K, default: V): V =
    if key.in(m.keys()) then m.get(key) else default

  pure def canCommunicate(a: PeerId, b: PeerId): bool =
    peers.get(a).partition = peers.get(b).partition

  pure def envelopeExpired(env: Envelope, now: Timestamp): bool =
    now >= env.createdAt + env.ttl

  action init: bool = all {
    peers' = Map(),
    envelopes' = Map(),
    networkPartitions' = Map(),
    currentTime' = 0
  }

  action registerPeer(peer: PeerId, neighbors: Set[PeerId], partition: int, quota: int): bool = all {
    not peer.in(peers.keys()),
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
    networkPartitions' = networkPartitions.set(partition, getOrDefault(networkPartitions, partition, Set()).union(Set(peer))),
    currentTime' = currentTime
  }

  action markByzantine(peer: PeerId): bool =
    val record = peers.get(peer) {
      peer.in(peers.keys()) {
        peers' = peers.set(peer, {
          ...record,
          isByzantine: true
        }),
        envelopes' = envelopes,
        networkPartitions' = networkPartitions,
        currentTime' = currentTime
      }
    }

  action postEnvelope(author: PeerId, recipient: PeerId, envId: EnvelopeId, payloadHash: int, ttl: int): bool =
    val record = peers.get(author) {
      author.in(peers.keys()) and recipient.in(peers.keys()) and ttl > 0 {
        val envelope = {
          id: envId,
          author: author,
          recipient: recipient,
          payloadHash: payloadHash,
          ttl: ttl,
          createdAt: currentTime
        }
        envelopes' = envelopes.set(envId, envelope),
        peers' = peers.set(author, {
          ...record,
          bulletin: record.bulletin.union(Set(envId))
        }),
        networkPartitions' = networkPartitions,
        currentTime' = currentTime
      }
    }

  action gossipEnvelope(sender: PeerId, peer: PeerId, envId: EnvelopeId): bool =
    val senderRecord = peers.get(sender) {
    val recipientRecord = peers.get(peer) {
    val envelope = envelopes.get(envId) {
      sender.in(peers.keys()) and
      peer.in(peers.keys()) and
      envId.in(envelopes.keys()) and
      not envelopeExpired(envelope, currentTime) and
      canCommunicate(sender, peer) and
      senderRecord.quota > 0 {
        val accepted =
          if senderRecord.isByzantine then true
          else recipientRecord.trustGraph.get(sender).default(Trusted) != Blocked {
        peers' = peers.set(peer, {
          ...recipientRecord,
          pending: if accepted then recipientRecord.pending.union(Set(envId)) else recipientRecord.pending,
          quota: recipientRecord.quota
        }).set(sender, {
          ...senderRecord,
          quota: senderRecord.quota - 1
        }),
        envelopes' = envelopes,
        networkPartitions' = networkPartitions,
        currentTime' = currentTime
      }
    }}}

  action acceptEnvelope(peer: PeerId, envId: EnvelopeId): bool =
    val record = peers.get(peer) {
    val envelope = envelopes.get(envId) {
      peer.in(peers.keys()) and envId.in(record.pending) and not envelopeExpired(envelope, currentTime) {
        peers' = peers.set(peer, {
          ...record,
          pending: record.pending.filter(id => id != envId),
          delivered: record.delivered.union(Set(envId))
        }),
        envelopes' = envelopes,
        networkPartitions' = networkPartitions,
        currentTime' = currentTime
      }
    }}

  action advanceTime(delta: int): bool = all {
    delta > 0,
    currentTime' = currentTime + delta,
    peers' = peers.keys().fold(peers, (acc, pid) =>
      val record = acc.get(pid) {
        acc.set(pid, {
          ...record,
          pending: record.pending.filter(id => not envelopeExpired(envelopes.get(id), currentTime + delta))
        })
      }
    ),
    envelopes' = envelopes,
    networkPartitions' = networkPartitions
  }

  action partitionNetwork(partitionId: int, peersInPartition: Set[PeerId]): bool = all {
    peers' = peers.keys().fold(peers, (acc, pid) =>
      val record = acc.get(pid) {
        if pid.in(peersInPartition)
          acc.set(pid, {...record, partition: partitionId})
        else
          acc.set(pid, {...record, partition: record.partition})
      }
    ),
    networkPartitions' = networkPartitions.set(partitionId, peersInPartition),
    envelopes' = envelopes,
    currentTime' = currentTime
  }

  // ==================== INVARIANTS ====================

  val InvariantEnvelopeRegistered =
    envelopes.values().forall(env =>
      env.author.in(peers.keys()) and env.recipient.in(peers.keys())
    )

  val InvariantBulletinEntriesExist =
    peers.values().forall(record =>
      record.bulletin.subseteq(envelopes.keys()) and
      record.pending.subseteq(envelopes.keys()) and
      record.delivered.subseteq(envelopes.keys())
    )

  val InvariantQuotaNonNegative =
    peers.values().forall(record => record.quota >= 0)

  val InvariantPendingNotDelivered =
    peers.values().forall(record =>
      record.pending.intersect(record.delivered).size() = 0
    )

  val InvariantPartitionMembership =
    peers.keys().forall(pid =>
      getOrDefault(networkPartitions, peers.get(pid).partition, Set()).contains(pid)
    )
}
