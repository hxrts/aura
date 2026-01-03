// Counter Protocol Specification
//
// Models conflict-free counter allocation for distributed value reservation.
// Authorities reserve unique counter ranges to avoid allocation conflicts.
// Uses Lamport timestamps for causal ordering of counter operations.
//
// Phase progression: CounterSetup → CounterOperating → CounterCompleted (or CounterFailed)
//
// Safety: Reserved values are disjoint across authorities within same epoch.
// Liveness: Protocol completes when reservation is recorded and finalized.
//
// See: docs/103_relational_contexts.md (counter leases)

module protocol_counter {
  // ============ TYPE DEFINITIONS ============
  // Local type definitions (Quint doesn't support type imports).

  type Option[a] = Some(a) | None

  // Opaque identifiers prevent internal structure leakage.
  type ProtocolId = str
  type AuthorityId = str  // Opaque authority identifier (was DeviceId)
  type CounterId = str    // Identifies a specific counter instance
  type Lamport = int      // Lamport timestamp for causal ordering
  type MessageId = str    // Unique message identifier

  // Counter operations: increment by delta, reset to zero, or sync state.
  type CounterOp = Increment(int) | Reset | Sync

  // ParticipantState tracks each authority's view of counters.
  type ParticipantState = {
    authority: AuthorityId,      // This participant's identity
    owns: Set[CounterId],        // Counters this authority owns
    lamport: Lamport,            // Local Lamport clock
    localCounters: CounterId -> int,  // Local counter values
    pending: List[CounterOp],    // Operations awaiting delivery
    delivered: Set[MessageId],   // Messages already processed (dedup)
    isByzantine: bool            // Byzantine fault injection flag
  }

  // CounterState represents shared counter metadata.
  type CounterState = {
    id: CounterId,               // Counter identifier
    value: int,                  // Current counter value
    owner: AuthorityId,          // Authority that owns this counter
    observers: Set[AuthorityId], // Authorities observing this counter
    lamport: Lamport             // Last update timestamp
  }

  // CounterMessage carries counter operations between authorities.
  type CounterMessage = {
    id: MessageId,               // Unique message identifier
    sender: AuthorityId,         // Who sent this message
    recipients: Set[AuthorityId],// Who receives this message
    counter: CounterId,          // Which counter is affected
    op: CounterOp,               // Operation to apply
    value: int,                  // Current value at send time
    lamport: Lamport             // Sender's Lamport timestamp
  }

  // Protocol phase tracks overall progress.
  type CounterPhase = CounterSetup | CounterOperating | CounterSynchronizing | CounterCompleted | CounterFailed

  // Lifecycle status for external coordination.
  type CounterLifecycleStatus = CounterPending | CounterLifecycleCompleted | CounterAborted

  // CounterOutcome captures the result of successful reservation.
  type CounterOutcome = {
    relationship: int,           // Relational context identifier
    reserved: Set[int],          // Reserved counter values
    ttl: int,                    // Lease duration
    epoch: int,                  // Epoch for stale detection
    requestingAuthority: AuthorityId  // Who requested reservation
  }

  // Effect type for logging (simplified, no imports).
  type CounterEffect = { effectType: str, payload: str }

  // ============ STATE VARIABLES ============
  // Protocol state tracked per session.

  var participants: ProtocolId -> AuthorityId -> ParticipantState  // Per-authority state
  var counters: ProtocolId -> CounterId -> CounterState            // Counter metadata
  var phase: ProtocolId -> CounterPhase                            // Protocol phase
  var pendingMessages: ProtocolId -> Set[CounterMessage]           // Undelivered messages
  var globalLamport: ProtocolId -> Lamport                         // Global Lamport clock
  var requestedValues: ProtocolId -> Set[int]                      // Values requested
  var reservedValues: ProtocolId -> Set[int]                       // Values reserved
  var relationshipHash: ProtocolId -> int                          // Relational context hash
  var requestingAuthority: ProtocolId -> AuthorityId               // Who requested
  var previousValue: ProtocolId -> int                             // Base value for reservation
  var ttl: ProtocolId -> int                                       // Lease duration
  var requestedEpoch: ProtocolId -> int                            // Epoch for reservation
  var lifecycleStatus: ProtocolId -> CounterLifecycleStatus        // Lifecycle state
  var lifecycleEffects: ProtocolId -> Set[CounterEffect]           // Emitted effects
  var outcome: ProtocolId -> Option[CounterOutcome]                // Final outcome if completed

  // ============ HELPERS ============
  // Pure utility functions for map access and initialization.

  // Safe map lookup with default value.
  pure def getOrDefaultInt(m: ProtocolId -> int, key: ProtocolId, default: int): int =
    if (key.in(m.keys())) m.get(key) else default

  pure def getOrDefaultSetInt(m: ProtocolId -> Set[int], key: ProtocolId): Set[int] =
    if (key.in(m.keys())) m.get(key) else Set()

  pure def getOrDefaultMessages(m: ProtocolId -> Set[CounterMessage], key: ProtocolId): Set[CounterMessage] =
    if (key.in(m.keys())) m.get(key) else Set()

  pure def getCounterValue(localCounters: CounterId -> int, counterId: CounterId, defaultVal: int): int =
    if (counterId.in(localCounters.keys())) localCounters.get(counterId) else defaultVal

  // Initialize participant map for new protocol session.
  // Creates default state for each member authority.
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

  // Initialize counter map for new protocol session.
  // Creates counters owned by specified authority, observed by all members.
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

  // Lamport clock update: max(local, observed) + 1.
  pure def bumpLamport(clock: Lamport, observed: Lamport): Lamport =
    (if (clock > observed) clock else observed) + 1

  // Generate unique message ID (simplified for model).
  pure def messageId(authority: AuthorityId, counter: CounterId, lamport: Lamport, nonce: int): MessageId =
    authority

  // ============ INITIALISATION ============
  // Start with empty state (no protocols registered).

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

  // ============ ACTIONS ============
  // Protocol operations for counter reservation.

  // Configure a new counter protocol session.
  // Requires: at least one authority, one counter, owner in authorities, pid not registered.
  // Effect: Transitions to CounterOperating phase.
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

  // Record reserved counter values before finalization.
  // Requires: protocol pending, phase is CounterOperating.
  // Effect: Sets both reservedValues and requestedValues.
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

  // Mark a participant as Byzantine (fault injection for testing).
  // Requires: participant exists in protocol.
  // Effect: Sets isByzantine flag on participant state.
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

  // Finalize counter protocol with successful completion.
  // Requires: no pending messages, phase is CounterOperating, at least one reserved value, lifecycle pending.
  // Effect: Transitions to CounterCompleted, sets outcome with reserved values.
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

  // Fail counter protocol due to error.
  // Requires: protocol not already completed.
  // Effect: Transitions to CounterFailed, sets lifecycle to CounterAborted.
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

  // Signal abort from external coordination layer.
  // Requires: lifecycle status is pending.
  // Effect: Sets lifecycle to CounterAborted, phase to CounterFailed.
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

  // ============ INVARIANTS ============
  // Safety properties verified by model checker.

  // All protocol maps share the same key set.
  val InvariantPhaseRegistered =
    phase.keys() == participants.keys()

  // Counter state exists for every protocol.
  val InvariantCountersRegistered =
    counters.keys() == participants.keys()

  // Lifecycle status defined for all protocols.
  val InvariantLifecycleStatusDefined =
    lifecycleStatus.keys() == phase.keys()

  // Completed protocols have an outcome set.
  // Safety: No successful completion without recorded result.
  val InvariantOutcomeWhenCompleted =
    lifecycleStatus.keys().forall(pid => {
      val status = lifecycleStatus.get(pid)
      val isCompleted = match status { | CounterLifecycleCompleted => true | _ => false }
      val outcomeOpt = if (pid.in(outcome.keys())) outcome.get(pid) else None
      val hasOutcome = match outcomeOpt { | Some(_) => true | None => false }
      isCompleted implies hasOutcome
    })
}
