module protocol_counter {
  // Base types (defined locally - type imports don't work in Quint)
  type Option[a] = Some(a) | None
  type ProtocolId = str
  type AuthorityId = str  // renamed from DeviceId for authority model
  type CounterId = str
  type Lamport = int
  type MessageId = str

  type CounterOp = Increment(int) | Reset | Sync

  type ParticipantState = {
    authority: AuthorityId,
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
    owner: AuthorityId,
    observers: Set[AuthorityId],
    lamport: Lamport
  }

  type CounterMessage = {
    id: MessageId,
    sender: AuthorityId,
    recipients: Set[AuthorityId],
    counter: CounterId,
    op: CounterOp,
    value: int,
    lamport: Lamport
  }

  type CounterPhase = CounterSetup | CounterOperating | CounterSynchronizing | CounterCompleted | CounterFailed
  type CounterLifecycleStatus = CounterPending | CounterLifecycleCompleted | CounterAborted

  type CounterOutcome = {
    relationship: int,
    reserved: Set[int],
    ttl: int,
    epoch: int,
    requestingAuthority: AuthorityId
  }

  // Simplified effect type (no imports)
  type CounterEffect = { effectType: str, payload: str }

  // ==================== STATE ====================

  var participants: ProtocolId -> AuthorityId -> ParticipantState
  var counters: ProtocolId -> CounterId -> CounterState
  var phase: ProtocolId -> CounterPhase
  var pendingMessages: ProtocolId -> Set[CounterMessage]
  var globalLamport: ProtocolId -> Lamport
  var requestedValues: ProtocolId -> Set[int]
  var reservedValues: ProtocolId -> Set[int]
  var relationshipHash: ProtocolId -> int
  var requestingAuthority: ProtocolId -> AuthorityId
  var previousValue: ProtocolId -> int
  var ttl: ProtocolId -> int
  var requestedEpoch: ProtocolId -> int
  var lifecycleStatus: ProtocolId -> CounterLifecycleStatus
  var lifecycleEffects: ProtocolId -> Set[CounterEffect]
  var outcome: ProtocolId -> Option[CounterOutcome]

  // ==================== HELPERS ====================

  pure def getOrDefaultInt(m: ProtocolId -> int, key: ProtocolId, default: int): int =
    if (key.in(m.keys())) m.get(key) else default

  pure def getOrDefaultSetInt(m: ProtocolId -> Set[int], key: ProtocolId): Set[int] =
    if (key.in(m.keys())) m.get(key) else Set()

  pure def getOrDefaultMessages(m: ProtocolId -> Set[CounterMessage], key: ProtocolId): Set[CounterMessage] =
    if (key.in(m.keys())) m.get(key) else Set()

  pure def getCounterValue(localCounters: CounterId -> int, counterId: CounterId, defaultVal: int): int =
    if (counterId.in(localCounters.keys())) localCounters.get(counterId) else defaultVal

  pure def ensureParticipantMap(
    mapping: ProtocolId -> AuthorityId -> ParticipantState,
    pid: ProtocolId,
    members: Set[AuthorityId]
  ): ProtocolId -> AuthorityId -> ParticipantState =
    if (pid.in(mapping.keys())) mapping
    else
      mapping.set(
        pid,
        members.fold(Map(), (acc, authority) => acc.set(authority, {
          authority: authority,
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
    counterIds: Set[CounterId],
    owner: AuthorityId,
    observers: Set[AuthorityId]
  ): ProtocolId -> CounterId -> CounterState =
    if (pid.in(mapping.keys())) mapping
    else
      mapping.set(
        pid,
        counterIds.fold(Map(), (acc, counterId) => acc.set(counterId, {
          id: counterId,
          value: 0,
          owner: owner,
          observers: observers,
          lamport: 0
        }))
      )

  pure def bumpLamport(clock: Lamport, observed: Lamport): Lamport =
    (if (clock > observed) clock else observed) + 1

  pure def messageId(authority: AuthorityId, counter: CounterId, lamport: Lamport, nonce: int): MessageId =
    authority

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
    requestingAuthority' = Map(),
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
    authorities: Set[AuthorityId],
    counterIds: Set[CounterId],
    owner: AuthorityId,
    relationship: int,
    requester: AuthorityId,
    base: int,
    ttlValue: int,
    epochValue: int
  ): bool = all {
    authorities.size() >= 1,
    counterIds.size() >= 1,
    owner.in(authorities),
    not(pid.in(phase.keys())),
    phase' = phase.set(pid, CounterOperating),
    participants' = ensureParticipantMap(participants, pid, authorities),
    counters' = ensureCounterMap(counters, pid, counterIds, owner, authorities),
    pendingMessages' = pendingMessages.set(pid, Set()),
    globalLamport' = globalLamport.set(pid, 0),
    requestedValues' = requestedValues.set(pid, Set()),
    reservedValues' = reservedValues.set(pid, Set()),
    relationshipHash' = relationshipHash.set(pid, relationship),
    requestingAuthority' = requestingAuthority.set(pid, requester),
    previousValue' = previousValue.set(pid, base),
    ttl' = ttl.set(pid, ttlValue),
    requestedEpoch' = requestedEpoch.set(pid, epochValue),
    lifecycleStatus' = lifecycleStatus.set(pid, CounterPending),
    lifecycleEffects' = lifecycleEffects.set(pid, Set()),
    outcome' = outcome.set(pid, None)
  }

  action recordReservation(pid: ProtocolId, values: Set[int]): bool = {
    val status = if (pid.in(lifecycleStatus.keys())) lifecycleStatus.get(pid) else CounterPending
    val isPending = match status { | CounterPending => true | _ => false }
    val currentPhase = if (pid.in(phase.keys())) phase.get(pid) else CounterSetup
    val isOperating = match currentPhase { | CounterOperating => true | _ => false }
    all {
      pid.in(phase.keys()),
      isPending,
      isOperating,
      reservedValues' = reservedValues.set(pid, values),
      requestedValues' = requestedValues.set(pid, values),
      participants' = participants,
      counters' = counters,
      pendingMessages' = pendingMessages,
      globalLamport' = globalLamport,
      relationshipHash' = relationshipHash,
      requestingAuthority' = requestingAuthority,
      previousValue' = previousValue,
      ttl' = ttl,
      requestedEpoch' = requestedEpoch,
      lifecycleStatus' = lifecycleStatus,
      lifecycleEffects' = lifecycleEffects,
      phase' = phase,
      outcome' = outcome
    }
  }

  action markByzantine(pid: ProtocolId, authority: AuthorityId): bool = {
    val participantMap = ensureParticipantMap(participants, pid, Set())
    val hasParticipant = pid.in(participantMap.keys()) and authority.in(participantMap.get(pid).keys())
    val participant = if (hasParticipant) participantMap.get(pid).get(authority) else {
      authority: authority,
      owns: Set(),
      lamport: 0,
      localCounters: Map(),
      pending: List(),
      delivered: Set(),
      isByzantine: false
    }
    all {
      hasParticipant,
      participants' = participants.set(
        pid,
        participantMap.get(pid).set(authority, {
          ...participant,
          isByzantine: true
        })
      ),
      counters' = counters,
      phase' = phase,
      pendingMessages' = pendingMessages,
      globalLamport' = globalLamport,
      reservedValues' = reservedValues,
      requestedValues' = requestedValues,
      relationshipHash' = relationshipHash,
      requestingAuthority' = requestingAuthority,
      previousValue' = previousValue,
      ttl' = ttl,
      requestedEpoch' = requestedEpoch,
      lifecycleStatus' = lifecycleStatus,
      lifecycleEffects' = lifecycleEffects,
      outcome' = outcome
    }
  }

  action finalize(pid: ProtocolId): bool = {
    val msgSet = getOrDefaultMessages(pendingMessages, pid)
    val reserved = getOrDefaultSetInt(reservedValues, pid)
    val status = if (pid.in(lifecycleStatus.keys())) lifecycleStatus.get(pid) else CounterPending
    val isPending = match status { | CounterPending => true | _ => false }
    val currentPhase = if (pid.in(phase.keys())) phase.get(pid) else CounterSetup
    val isOperating = match currentPhase { | CounterOperating => true | _ => false }
    val relHash = getOrDefaultInt(relationshipHash, pid, 0)
    val prev = getOrDefaultInt(previousValue, pid, 0)
    val ttlVal = getOrDefaultInt(ttl, pid, 0)
    val epochVal = getOrDefaultInt(requestedEpoch, pid, 0)
    val requester = if (pid.in(requestingAuthority.keys())) requestingAuthority.get(pid) else ""
    all {
      msgSet.size() == 0,
      isOperating,
      reserved.size() >= 1,
      isPending,
      phase' = phase.set(pid, CounterCompleted),
      lifecycleStatus' = lifecycleStatus.set(pid, CounterLifecycleCompleted),
      lifecycleEffects' = lifecycleEffects.set(pid, Set()),
      outcome' = outcome.set(pid, Some({
        relationship: relHash,
        reserved: reserved,
        ttl: ttlVal,
        epoch: epochVal,
        requestingAuthority: requester
      })),
      participants' = participants,
      counters' = counters,
      pendingMessages' = pendingMessages,
      globalLamport' = globalLamport,
      relationshipHash' = relationshipHash,
      requestingAuthority' = requestingAuthority,
      previousValue' = previousValue,
      ttl' = ttl,
      requestedEpoch' = requestedEpoch,
      reservedValues' = reservedValues,
      requestedValues' = requestedValues
    }
  }

  action failCounter(pid: ProtocolId): bool = {
    val currentPhase = if (pid.in(phase.keys())) phase.get(pid) else CounterSetup
    val isNotCompleted = match currentPhase { | CounterCompleted => false | _ => true }
    all {
      isNotCompleted,
      phase' = phase.set(pid, CounterFailed),
      lifecycleStatus' = lifecycleStatus.set(pid, CounterAborted),
      participants' = participants,
      counters' = counters,
      pendingMessages' = pendingMessages,
      globalLamport' = globalLamport,
      lifecycleEffects' = lifecycleEffects,
      outcome' = outcome,
      reservedValues' = reservedValues,
      requestedValues' = requestedValues,
      relationshipHash' = relationshipHash,
      requestingAuthority' = requestingAuthority,
      previousValue' = previousValue,
      ttl' = ttl,
      requestedEpoch' = requestedEpoch
    }
  }

  action signalAbort(pid: ProtocolId): bool = {
    val status = if (pid.in(lifecycleStatus.keys())) lifecycleStatus.get(pid) else CounterPending
    val isPending = match status { | CounterPending => true | _ => false }
    all {
      isPending,
      lifecycleStatus' = lifecycleStatus.set(pid, CounterAborted),
      lifecycleEffects' = lifecycleEffects,
      phase' = phase.set(pid, CounterFailed),
      participants' = participants,
      counters' = counters,
      pendingMessages' = pendingMessages,
      globalLamport' = globalLamport,
      relationshipHash' = relationshipHash,
      requestingAuthority' = requestingAuthority,
      previousValue' = previousValue,
      ttl' = ttl,
      requestedEpoch' = requestedEpoch,
      reservedValues' = reservedValues,
      requestedValues' = requestedValues,
      outcome' = outcome
    }
  }

  // ==================== INVARIANTS ====================

  val InvariantPhaseRegistered =
    phase.keys() == participants.keys()

  val InvariantCountersRegistered =
    counters.keys() == participants.keys()

  val InvariantLifecycleStatusDefined =
    lifecycleStatus.keys() == phase.keys()

  val InvariantOutcomeWhenCompleted =
    lifecycleStatus.keys().forall(pid => {
      val status = lifecycleStatus.get(pid)
      val isCompleted = match status { | CounterLifecycleCompleted => true | _ => false }
      val outcomeOpt = if (pid.in(outcome.keys())) outcome.get(pid) else None
      val hasOutcome = match outcomeOpt { | Some(_) => true | None => false }
      isCompleted implies hasOutcome
    })
}
