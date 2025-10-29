module protocol_counter {
  import protocol_core as core from "protocol_core"

  type ProtocolId = core.ProtocolId
  type DeviceId = core.DeviceId
  type CounterId = str
  type Lamport = int
  type MessageId = str

  type CounterOp = Increment(int) | Reset | Sync

  type ParticipantState = {
    device: DeviceId,
    owns: Set[CounterId],
    lamport: Lamport,
    localCounters: CounterId -> int,
    pending: List[CounterOp],
    delivered: Set[MessageId],
    isByzantine: bool
  }

  type CounterState = {
    id: CounterId,
    value: int,
    owner: DeviceId,
    observers: Set[DeviceId],
    lamport: Lamport
  }

  type CounterMessage = {
    id: MessageId,
    sender: DeviceId,
    recipients: Set[DeviceId],
    counter: CounterId,
    op: CounterOp,
    value: int,
    lamport: Lamport
  }

type CounterPhase = Setup | Operating | Synchronizing | Completed | Failed
type LifecycleStatus = Pending | Completed | Aborted
type CounterOutcome = {
  relationship: int,
  reserved: Set[int],
  ttl: int,
  epoch: int,
  requestingDevice: DeviceId
}

  // ==================== STATE ====================

  var participants: ProtocolId -> DeviceId -> ParticipantState
  var counters: ProtocolId -> CounterId -> CounterState
  var phase: ProtocolId -> CounterPhase
  var pendingMessages: ProtocolId -> Set[CounterMessage]
var globalLamport: ProtocolId -> Lamport
var requestedValues: ProtocolId -> Set[int]
var reservedValues: ProtocolId -> Set[int]
var relationshipHash: ProtocolId -> int
var requestingDevice: ProtocolId -> DeviceId
var previousValue: ProtocolId -> int
var ttl: ProtocolId -> int
var requestedEpoch: ProtocolId -> int
var lifecycleStatus: ProtocolId -> LifecycleStatus
var lifecycleEffects: ProtocolId -> Set[core.ProtocolEffect]
var outcome: ProtocolId -> Option[CounterOutcome]

  // ==================== HELPERS ====================

  pure def getOrDefault[K, V](m: K -> V, key: K, default: V): V =
    if key.in(m.keys()) then m.get(key) else default

  pure def ensureParticipantMap(
    mapping: ProtocolId -> DeviceId -> ParticipantState,
    pid: ProtocolId,
    members: Set[DeviceId]
  ): ProtocolId -> DeviceId -> ParticipantState =
    if pid.in(mapping.keys()) then mapping
    else
      mapping.set(
        pid,
        members.fold(Map(), (acc, device) => acc.set(device, {
          device: device,
          owns: Set(),
          lamport: 0,
          localCounters: Map(),
          pending: List(),
          delivered: Set(),
          isByzantine: false
        }))
      )

  pure def ensureCounterMap(
    mapping: ProtocolId -> CounterId -> CounterState,
    pid: ProtocolId,
    counters: Set[CounterId],
    owner: DeviceId,
    observers: Set[DeviceId]
  ): ProtocolId -> CounterId -> CounterState =
    if pid.in(mapping.keys()) then mapping
    else
      mapping.set(
        pid,
        counters.fold(Map(), (acc, counterId) => acc.set(counterId, {
          id: counterId,
          value: 0,
          owner: owner,
          observers: observers,
          lamport: 0
        }))
      )

  pure def bumpLamport(clock: Lamport, observed: Lamport): Lamport =
    max(clock, observed) + 1

  pure def messageId(device: DeviceId, counter: CounterId, lamport: Lamport, nonce: int): MessageId =
    device ++ "-" ++ counter ++ "-" ++ lamport.toStr() ++ "-" ++ nonce.toStr()

  pure def updateParticipant(
    pid: ProtocolId,
    device: DeviceId,
    update: ParticipantState -> ParticipantState
  ): ProtocolId -> DeviceId -> ParticipantState =
    participants.set(pid, participants.get(pid).set(device, update(participants.get(pid).get(device))))

  // ==================== INITIALISATION ====================

  action init: bool = all {
    participants' = Map(),
    counters' = Map(),
    phase' = Map(),
    pendingMessages' = Map(),
    globalLamport' = Map(),
    requestedValues' = Map(),
    reservedValues' = Map(),
    relationshipHash' = Map(),
    requestingDevice' = Map(),
    previousValue' = Map(),
    ttl' = Map(),
    requestedEpoch' = Map(),
    lifecycleStatus' = Map(),
    lifecycleEffects' = Map(),
    outcome' = Map()
  }

  // ==================== ACTIONS ====================

  action configureCounters(
    pid: ProtocolId,
    devices: Set[DeviceId],
    counterIds: Set[CounterId],
    owner: DeviceId,
    relationship: int,
    requester: DeviceId,
    base: int,
    ttlValue: int,
    epochValue: int
  ): bool = all {
    devices.size() >= 1,
    counterIds.size() >= 1,
    owner.in(devices),
    not pid.in(phase.keys()),
    phase' = phase.set(pid, Operating),
    participants' = ensureParticipantMap(participants, pid, devices),
    counters' = ensureCounterMap(counters, pid, counterIds, owner, devices),
    pendingMessages' = pendingMessages.set(pid, Set()),
    globalLamport' = globalLamport.set(pid, 0),
    requestedValues' = requestedValues.set(pid, Set()),
    reservedValues' = reservedValues.set(pid, Set()),
    relationshipHash' = relationshipHash.set(pid, relationship),
    requestingDevice' = requestingDevice.set(pid, requester),
    previousValue' = previousValue.set(pid, base),
    ttl' = ttl.set(pid, ttlValue),
    requestedEpoch' = requestedEpoch.set(pid, epochValue),
    lifecycleStatus' = lifecycleStatus.set(pid, Pending),
    lifecycleEffects' = lifecycleEffects.set(pid, Set())
  }

  action recordReservation(pid: ProtocolId, values: Set[int]): bool = all {
    pid.in(phase.keys()),
    lifecycleStatus.get(pid).default(Pending) = Pending,
    phase.get(pid) = Operating,
    reservedValues' = reservedValues.set(pid, values),
    requestedValues' = requestedValues.set(pid, values),
    participants' = participants,
    counters' = counters,
    pendingMessages' = pendingMessages,
    globalLamport' = globalLamport,
    relationshipHash' = relationshipHash,
    requestingDevice' = requestingDevice,
    previousValue' = previousValue,
    ttl' = ttl,
    requestedEpoch' = requestedEpoch,
    lifecycleStatus' = lifecycleStatus,
    lifecycleEffects' = lifecycleEffects
  }

  action markByzantine(pid: ProtocolId, device: DeviceId): bool =
    val participantMap = ensureParticipantMap(participants, pid, Set()) {
      device.in(participantMap.get(pid).keys()) {
        participants' = participants.set(
          pid,
          participantMap.get(pid).set(device, {
            ...participantMap.get(pid).get(device),
            isByzantine: true
          })
        ),
        counters' = counters,
        phase' = phase,
        pendingMessages' = pendingMessages,
        globalLamport' = globalLamport
      }
    }

  action localIncrement(
    pid: ProtocolId,
    device: DeviceId,
    counterId: CounterId,
    delta: int
  ): bool =
    val participantMap = participants.get(pid) {
    val counterMap = counters.get(pid) {
      phase.get(pid) = Operating and
      device.in(participantMap.keys()) and
      counterId.in(counterMap.keys()) and
      delta > 0 {
        val participant = participantMap.get(device)
        val counter = counterMap.get(counterId)
        val currentLocal = getOrDefault(participant.localCounters, counterId, counter.value)
        val updatedValue = currentLocal + delta {
        val nextLamport = participant.lamport + 1 {
        participants' = participants.set(pid, participantMap.set(device, {
          ...participant,
          localCounters: participant.localCounters.set(counterId, updatedValue),
          lamport: nextLamport,
          pending: participant.pending.append(Increment(delta))
        })),
        counters' = counters,
        phase' = phase,
        pendingMessages' = pendingMessages,
        globalLamport' = globalLamport.set(pid, max(getOrDefault(globalLamport, pid, 0), nextLamport))
      }}}}}

  action broadcastOperation(
    pid: ProtocolId,
    device: DeviceId,
    counterId: CounterId,
    op: CounterOp,
    nonce: int
  ): bool =
    val participantMap = participants.get(pid) {
    val counterMap = counters.get(pid) {
      phase.get(pid) = Operating and
      device.in(participantMap.keys()) and
      counterId.in(counterMap.keys()) and
      op.in(participantMap.get(device).pending) {
        val participant = participantMap.get(device)
        val counter = counterMap.get(counterId)
        val localValue = getOrDefault(participant.localCounters, counterId, counter.value)
        val value =
          if participant.isByzantine then
            0.to(1000).oneOf()
          else
            localValue {
        val msg = {
          id: messageId(device, counterId, participant.lamport, nonce),
          sender: device,
          recipients: counter.observers,
          counter: counterId,
          op: op,
          value: value,
          lamport: participant.lamport
        } {
        pendingMessages' = pendingMessages.set(
          pid,
          getOrDefault(pendingMessages, pid, Set()).union(Set(msg))
        ),
        participants' = participants.set(pid, participantMap.set(device, {
          ...participant,
          pending: participant.pending.select(o => o != op)
        })),
        counters' = counters,
        phase' = phase,
        globalLamport' = globalLamport
      }}}}}

  action deliverMessage(
    pid: ProtocolId,
    msgId: MessageId,
    receiver: DeviceId
  ): bool =
    val msgSet = getOrDefault(pendingMessages, pid, Set()) {
      msgSet.exists(m => m.id = msgId) and
      receiver.in(participants.get(pid).keys()) {
        val msg = msgSet.filter(m => m.id = msgId).oneOf()
        receiver.in(msg.recipients) and
        not(participants.get(pid).get(receiver).delivered.contains(msgId)) {
          val participant = participants.get(pid).get(receiver)
          val counter = counters.get(pid).get(msg.counter)
          val updatedLamport = bumpLamport(participant.lamport, msg.lamport)
          val currentLocal = getOrDefault(participant.localCounters, msg.counter, counter.value)
          val updatedValue = match msg.op {
            | Increment(delta) => min(currentLocal + delta, 1000)
            | Reset => 0
            | Sync => msg.value
          } {
          participants' = participants.set(pid, participants.get(pid).set(receiver, {
            ...participant,
            lamport: updatedLamport,
            localCounters: participant.localCounters.set(msg.counter, updatedValue),
            delivered: participant.delivered.union(Set(msgId))
          })),
          counters' = counters.set(pid, counters.get(pid).set(msg.counter, {
            ...counter,
            value: updatedValue,
            lamport: max(counter.lamport, msg.lamport)
          })),
          pendingMessages' = pendingMessages.set(pid, msgSet),
          globalLamport' = globalLamport.set(pid, max(getOrDefault(globalLamport, pid, 0), updatedLamport)),
          phase' = phase
        }
      }
    }

  action synchronize(
    pid: ProtocolId,
    device: DeviceId,
    counterId: CounterId,
    snapshot: int
  ): bool =
    val participantMap = participants.get(pid) {
    val counterMap = counters.get(pid) {
      phase.get(pid) = Operating and
      device.in(participantMap.keys()) and
      counterId.in(counterMap.keys()) {
        val participant = participantMap.get(device)
        val counter = counterMap.get(counterId)
        participants' = participants.set(pid, participantMap.set(device, {
          ...participant,
          localCounters: participant.localCounters.set(counterId, snapshot)
        })),
        counters' = counters.set(pid, counterMap.set(counterId, {
          ...counter,
          value: snapshot
        })),
        phase' = phase,
        pendingMessages' = pendingMessages,
        globalLamport' = globalLamport
      }
    }}

  action finalize(pid: ProtocolId): bool =
    val msgSet = getOrDefault(pendingMessages, pid, Set()) {
    val reserved = reservedValues.get(pid) {
      msgSet.size() = 0 and
      phase.get(pid) = Operating and
      reserved.size() >= 1 and
      lifecycleStatus.get(pid).default(Pending) = Pending {
        phase' = phase.set(pid, Completed),
        lifecycleStatus' = lifecycleStatus.set(pid, Completed),
        lifecycleEffects' = lifecycleEffects.set(pid, Set(core.UpdateCounter({
          relationship: relationshipHash.get(pid),
          previous: previousValue.get(pid),
          reserved: reserved,
          ttl: ttl.get(pid),
          epoch: requestedEpoch.get(pid),
          requestingDevice: requestingDevice.get(pid)
        }))),
        outcome' = outcome.set(pid, Some({
          relationship: relationshipHash.get(pid),
          reserved: reserved,
          ttl: ttl.get(pid),
          epoch: requestedEpoch.get(pid),
          requestingDevice: requestingDevice.get(pid)
        })),
        participants' = participants,
        counters' = counters,
        pendingMessages' = pendingMessages,
        globalLamport' = globalLamport,
        relationshipHash' = relationshipHash,
        requestingDevice' = requestingDevice,
        previousValue' = previousValue,
        ttl' = ttl,
        requestedEpoch' = requestedEpoch,
        reservedValues' = reservedValues,
        requestedValues' = requestedValues
      }
    }}

  action fail(pid: ProtocolId): bool =
    phase.get(pid) != Completed {
      phase' = phase.set(pid, Failed),
      participants' = participants,
      counters' = counters,
      pendingMessages' = pendingMessages,
      globalLamport' = globalLamport,
      lifecycleStatus' = lifecycleStatus.set(pid, Aborted),
      lifecycleEffects' = lifecycleEffects,
      outcome' = outcome
    }

  action signalAbort(pid: ProtocolId): bool =
    lifecycleStatus.get(pid).default(Pending) = Pending {
      lifecycleStatus' = lifecycleStatus.set(pid, Aborted),
      lifecycleEffects' = lifecycleEffects,
      phase' = phase.set(pid, Failed),
      participants' = participants,
      counters' = counters,
      pendingMessages' = pendingMessages,
      globalLamport' = globalLamport,
      relationshipHash' = relationshipHash,
      requestingDevice' = requestingDevice,
      previousValue' = previousValue,
      ttl' = ttl,
      requestedEpoch' = requestedEpoch,
      reservedValues' = reservedValues,
      requestedValues' = requestedValues,
      outcome' = outcome
    }

  // ==================== INVARIANTS ====================

  val InvariantPhaseRegistered =
    phase.keys() = participants.keys()

  val InvariantCountersRegistered =
    counters.keys() = participants.keys()

  val InvariantLamportMonotonic =
    participants.keys().forall(pid =>
      participants.get(pid).values().forall(p =>
        p.lamport >= 0 and
        getOrDefault(globalLamport, pid, 0) >= p.lamport
      )
    )

  val InvariantMessagesFromParticipants =
    pendingMessages.keys().forall(pid =>
      pendingMessages.get(pid).forall(msg =>
        msg.sender.in(participants.get(pid).keys())
      )
    )

  val InvariantDeliveredRecorded =
    participants.keys().forall(pid =>
      participants.get(pid).values().forall(p =>
        p.delivered.subseteq(pendingMessages.get(pid).map(m => m.id))
      )
    )

  val InvariantCounterOwnership =
    counters.keys().forall(pid =>
      counters.get(pid).values().forall(c =>
        c.owner.in(c.observers)
      )
    )

  val InvariantLifecycleStatusDefined =
    lifecycleStatus.keys() = phase.keys()

  val InvariantLifecycleEffectsConsistency =
    lifecycleEffects.keys().forall(pid =>
      lifecycleEffects.get(pid).forall(effect =>
        match effect {
          | core.UpdateCounter(counter) =>
              counter.relationship = relationshipHash.get(pid) and
              counter.requestingDevice = requestingDevice.get(pid)
          | _ => false
        }
      )
    )

  val InvariantCompletedImpliesEffect =
    lifecycleStatus.keys().forall(pid =>
      lifecycleStatus.get(pid) = Completed implies lifecycleEffects.get(pid).size() = 1
    )

  val InvariantOutcomeWhenCompleted =
    lifecycleStatus.keys().forall(pid =>
      lifecycleStatus.get(pid) = Completed implies outcome.get(pid).isSome()
    )

  val InvariantOutcomeMetadataConsistency =
    outcome.keys().forall(pid =>
      match outcome.get(pid) {
        | Some(result) =>
            result.relationship = relationshipHash.get(pid) and
            result.requestingDevice = requestingDevice.get(pid) and
            result.ttl = ttl.get(pid) and
            result.epoch = requestedEpoch.get(pid)
        | None => true
      }
    )
}
