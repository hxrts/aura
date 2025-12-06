module protocol_sbb {
  // Base types (defined locally - type imports don't work in Quint)
  type Option[a] = Some(a) | None
  type PeerId = str
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
  var networkPartitions: int -> Set[PeerId]
  var currentTime: Timestamp

  // ============ HELPERS ============

  pure def getOrDefaultPeerRecord(m: PeerId -> PeerRecord, key: PeerId): Option[PeerRecord] =
    if (key.in(m.keys())) Some(m.get(key)) else None

  pure def getOrDefaultEnvelope(m: EnvelopeId -> Envelope, key: EnvelopeId): Option[Envelope] =
    if (key.in(m.keys())) Some(m.get(key)) else None

  pure def getOrDefaultPartition(m: int -> Set[PeerId], key: int): Set[PeerId] =
    if (key.in(m.keys())) m.get(key) else Set()

  pure def getTrustLevel(trustGraph: PeerId -> TrustLevel, peer: PeerId): TrustLevel =
    if (peer.in(trustGraph.keys())) trustGraph.get(peer) else Trusted

  def canCommunicate(a: PeerId, b: PeerId): bool = {
    val aRecordOpt = getOrDefaultPeerRecord(peers, a)
    val bRecordOpt = getOrDefaultPeerRecord(peers, b)
    val aPartition = match aRecordOpt { | Some(r) => r.partition | None => -1 }
    val bPartition = match bRecordOpt { | Some(r) => r.partition | None => -1 }
    aPartition == bPartition
  }

  pure def envelopeExpired(env: Envelope, now: Timestamp): bool =
    now >= env.createdAt + env.ttl

  // ============ INITIALISATION ============

  action init: bool = all {
    peers' = Map(),
    envelopes' = Map(),
    networkPartitions' = Map(),
    currentTime' = 0
  }

  // ============ ACTIONS ============

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

  // ==================== INVARIANTS ====================

  val InvariantEnvelopeRegistered =
    envelopes.keys().forall(envId => {
      val env = envelopes.get(envId)
      env.author.in(peers.keys()) and env.recipient.in(peers.keys())
    })

  val InvariantBulletinEntriesExist =
    peers.keys().forall(pid => {
      val record = peers.get(pid)
      record.bulletin.subseteq(envelopes.keys()) and
      record.pending.subseteq(envelopes.keys()) and
      record.delivered.subseteq(envelopes.keys())
    })

  val InvariantQuotaNonNegative =
    peers.keys().forall(pid => peers.get(pid).quota >= 0)

  val InvariantPendingNotDelivered =
    peers.keys().forall(pid => {
      val record = peers.get(pid)
      record.pending.intersect(record.delivered).size() == 0
    })

  val InvariantPartitionMembership =
    peers.keys().forall(pid => {
      val record = peers.get(pid)
      getOrDefaultPartition(networkPartitions, record.partition).contains(pid)
    })
}
