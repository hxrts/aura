// Protocol Core - Aura Protocol State Machine
//
// This module defines the core types and state machine for Aura protocols.
// Updated 2024-12-05: Fixed Quint 0.25.x syntax and aligned with authority model.
//
// Key changes from legacy version:
// - DeviceId → AuthorityId (opaque authority, not device exposure)
// - RelationshipId → ContextId (consistent with relational context terminology)
// - 'from' → 'sender' (reserved keyword)
// - Fixed nested val binding syntax
//
// See: docs/100_authority_and_identity.md, docs/103_relational_contexts.md

module protocol_core {

  // ==================== TYPE DEFINITIONS ====================

  type Option[a] = Some(a) | None

  type ProtocolId = str
  type SessionId = str
  type AuthorityId = str   // Opaque authority identifier (was DeviceId)
  type TimerId = str
  type ContextId = str     // Relational context identifier (was RelationshipId)

  type ProtocolKind =
    | Dkd
    | Resharing
    | Recovery
    | Locking
    | CounterReservation
    | CustomKind(str)

  type OperationKind =
    | DkdOp
    | ResharingOp
    | RecoveryOp
    | LockingOp
    | CounterOp
    | CustomOp(str)

  type ProtocolPriority = Low | Normal | High | Critical
  type ProtocolMode = Asynchronous | Interactive | Deterministic

  type ProtocolDescriptor = {
    session: SessionId,
    authority: AuthorityId,  // was: device
    protocol: ProtocolKind,
    operation: OperationKind,
    priority: ProtocolPriority,
    mode: ProtocolMode
  }

  type ProtocolState =
    | Initialized
    | Active
    | AwaitingEvidence
    | Completed
    | Failed
    | Cancelled

  // Note: 'from' is reserved in Quint, using 'sender' instead
  type MessageEffect = {
    sender: AuthorityId,
    recipients: Set[AuthorityId],
    session: SessionId,
    payloadHash: int
  }

  type JournalEffect = {
    session: SessionId,
    eventType: str,
    payloadHash: int
  }

  type TimerEffect = {
    timer: TimerId,
    timeout: int
  }

  type CounterEffect = {
    context: ContextId,  // was: relationship
    previous: int,
    reserved: Set[int],
    ttl: int,
    epoch: int,
    requestingAuthority: AuthorityId  // was: requestingDevice
  }

  type TraceEffect = {
    message: str
  }

  type ProtocolEffect =
    | Send(MessageEffect)
    | Broadcast(MessageEffect)
    | AppendJournal(JournalEffect)
    | ScheduleTimer(TimerEffect)
    | CancelTimer(TimerId)
    | Trace(TraceEffect)
    | UpdateCounter(CounterEffect)

  type ProtocolInput =
    | TransportIn(MessageEffect)
    | JournalIn(JournalEffect)
    | TimerFired({timer: TimerId})
    | LocalSignal({signal: str})

  type OutputState =
    | NotReady
    | Success({commitHash: int})
    | Failure({reason: str})

  type EvidenceStatus = Pending | Valid | Rejected

  type EvidenceRecord = {
    state: ProtocolState,
    witnessValid: bool,
    payloadHash: int,
    status: EvidenceStatus
  }

  // Note: 'from'/'to' are reserved, using 'prevState'/'nextState'
  type TransitionEvent = {
    protocol: ProtocolId,
    prevState: ProtocolState,
    nextState: ProtocolState
  }

  type EffectRecord = {
    protocol: ProtocolId,
    effect: ProtocolEffect
  }

  type InputRecord = {
    protocol: ProtocolId,
    input: ProtocolInput
  }

  type TimerEntry = {
    protocol: ProtocolId,
    timeout: int,
    active: bool
  }

  type TimerState = {
    timers: TimerId -> TimerEntry,
    ownership: ProtocolId -> Set[TimerId]
  }

  // ==================== PURE HELPERS ====================

  pure val FINAL_STATES: Set[ProtocolState] = Set(Completed, Failed, Cancelled)
  pure val TERMINATION_ACCEPTING: Set[ProtocolState] = FINAL_STATES.union(Set(AwaitingEvidence))
  pure val ALLOWED_TRANSITIONS: Set[(ProtocolState, ProtocolState)] = Set(
    (Initialized, Active),
    (Initialized, Cancelled),
    (Active, Active),
    (Active, AwaitingEvidence),
    (Active, Completed),
    (Active, Failed),
    (Active, Cancelled),
    (AwaitingEvidence, AwaitingEvidence),
    (AwaitingEvidence, Completed),
    (AwaitingEvidence, Failed),
    (AwaitingEvidence, Cancelled)
  )

  pure def isFinal(state: ProtocolState): bool =
    FINAL_STATES.contains(state)

  pure def canTerminate(state: ProtocolState): bool =
    TERMINATION_ACCEPTING.contains(state)

  // Note: renamed params from 'from'/'to' (reserved), 'next' is also built-in
  pure def transitionAllowed(prevState: ProtocolState, nextState: ProtocolState): bool =
    prevState == nextState or ALLOWED_TRANSITIONS.contains((prevState, nextState))

  // Generic helper using type inference (no [K, V] syntax in Quint)
  pure def getOrDefault(m, key, default) =
    if (key.in(m.keys())) m.get(key) else default

  pure def ensureOwnership(
    ownership: ProtocolId -> Set[TimerId],
    pid: ProtocolId
  ): ProtocolId -> Set[TimerId] =
    if (pid.in(ownership.keys())) ownership
    else ownership.set(pid, Set())

  // Simplified timer state update - handles ScheduleTimer and CancelTimer effects
  pure def updateTimerState(
    acc: TimerState,
    pid: ProtocolId,
    effect: ProtocolEffect
  ): TimerState =
    match effect {
      | ScheduleTimer(info) => {
          val baseOwnership = ensureOwnership(acc.ownership, pid)
          val newEntry = {
            protocol: pid,
            timeout: info.timeout,
            active: true
          }
          val canSchedule = not(info.timer.in(acc.timers.keys())) or
            (acc.timers.get(info.timer).protocol == pid and
             not(acc.timers.get(info.timer).active))
          if (canSchedule) {
            timers: acc.timers.set(info.timer, newEntry),
            ownership: baseOwnership.set(pid, baseOwnership.get(pid).union(Set(info.timer)))
          } else acc
        }
      | CancelTimer(timer) =>
          if (timer.in(acc.timers.keys()) and acc.timers.get(timer).protocol == pid) {
            timers: acc.timers.set(timer, {
              protocol: pid,
              timeout: acc.timers.get(timer).timeout,
              active: false
            }),
            ownership: ensureOwnership(acc.ownership, pid).set(
              pid,
              getOrDefault(acc.ownership, pid, Set()).filter(t => t != timer)
            )
          } else acc
      | _ => acc
    }

  pure def applyEffectTimers(
    timersMap: TimerId -> TimerEntry,
    ownership: ProtocolId -> Set[TimerId],
    pid: ProtocolId,
    effects: Set[ProtocolEffect]
  ): TimerState =
    effects.fold(
      {timers: timersMap, ownership: ensureOwnership(ownership, pid)},
      (acc, effect) => updateTimerState(acc, pid, effect)
    )

  pure def isParticipantsSender(
    participantSet: Set[AuthorityId],
    descriptor: ProtocolDescriptor,
    senderAuth: AuthorityId
  ): bool =
    senderAuth == descriptor.authority or senderAuth.in(participantSet)

  pure def effectAllowed(
    pid: ProtocolId,
    descriptor: ProtocolDescriptor,
    participantSet: Set[AuthorityId],
    state: ProtocolState,
    effect: ProtocolEffect,
    timerState: TimerState
  ): bool =
    match effect {
      | Send(msg) =>
          msg.recipients.size() >= 1 and
          msg.session == descriptor.session and
          isParticipantsSender(participantSet, descriptor, msg.sender)
      | Broadcast(msg) =>
          msg.recipients.size() >= 1 and
          msg.session == descriptor.session and
          isParticipantsSender(participantSet, descriptor, msg.sender)
      | AppendJournal(event) =>
          event.session == descriptor.session and
          canTerminate(state)
      | ScheduleTimer(info) =>
          not(info.timer.in(timerState.timers.keys())) or (
            timerState.timers.get(info.timer).protocol == pid and
            not(timerState.timers.get(info.timer).active)
          )
      | CancelTimer(timer) =>
          timer.in(timerState.timers.keys()) and
          timerState.timers.get(timer).protocol == pid and
          timerState.timers.get(timer).active
      | Trace(_) => true
      | UpdateCounter(counter) =>
          (descriptor.protocol == Recovery or descriptor.protocol == Locking) and
          canTerminate(state) and
          counter.reserved.size() >= 1
    }

  pure def effectIsJournal(effect: ProtocolEffect): bool =
    match effect {
      | AppendJournal(_) => true
      | _ => false
    }

  pure def effectIsMessage(effect: ProtocolEffect): bool =
    match effect {
      | Send(_) => true
      | Broadcast(_) => true
      | _ => false
    }

  pure def effectTimerId(effect: ProtocolEffect): Option[TimerId] =
    match effect {
      | ScheduleTimer(info) => Some(info.timer)
      | CancelTimer(timer) => Some(timer)
      | _ => None
    }

  pure def successfulOutput(output: OutputState): bool =
    match output {
      | Success(_) => true
      | _ => false
    }

  pure def failedOutput(output: OutputState): bool =
    match output {
      | Failure(_) => true
      | _ => false
    }

  pure def evidenceStatus(e: Option[EvidenceRecord]): Option[EvidenceStatus] =
    match e {
      | Some(record) => Some(record.status)
      | None => None
    }

  pure def evidenceState(e: Option[EvidenceRecord]): Option[ProtocolState] =
    match e {
      | Some(record) => Some(record.state)
      | None => None
    }

  // ==================== STATE VARIABLES ====================

  var descriptors: ProtocolId -> ProtocolDescriptor
  var lifecycleStates: ProtocolId -> ProtocolState
  var sessionIndex: SessionId -> Set[ProtocolId]
  var participants: ProtocolId -> Set[AuthorityId]
  var outputs: ProtocolId -> OutputState
  var effectLog: Set[EffectRecord]
  var inputLog: Set[InputRecord]
  var transitionLog: Set[TransitionEvent]
  var timers: TimerId -> TimerEntry
  var timerOwnership: ProtocolId -> Set[TimerId]
  var evidence: ProtocolId -> Option[EvidenceRecord]
  var rehydrated: Set[ProtocolId]
  var journalEffects: ProtocolId -> Set[JournalEffect]

  // ==================== INITIALISATION ====================

  action init: bool = all {
    descriptors' = Map(),
    lifecycleStates' = Map(),
    sessionIndex' = Map(),
    participants' = Map(),
    outputs' = Map(),
    effectLog' = Set(),
    inputLog' = Set(),
    transitionLog' = Set(),
    timers' = Map(),
    timerOwnership' = Map(),
    evidence' = Map(),
    rehydrated' = Set(),
    journalEffects' = Map()
  }

  // ==================== CORE ACTIONS ====================

  action registerProtocol(
    pid: ProtocolId,
    descriptor: ProtocolDescriptor,
    members: Set[AuthorityId]
  ): bool = all {
    not(pid.in(descriptors.keys())),
    descriptors' = descriptors.set(pid, descriptor),
    lifecycleStates' = lifecycleStates.set(pid, Initialized),
    sessionIndex' = sessionIndex.set(
      descriptor.session,
      getOrDefault(sessionIndex, descriptor.session, Set()).union(Set(pid))
    ),
    participants' = participants.set(pid, members),
    outputs' = outputs.set(pid, NotReady),
    effectLog' = effectLog,
    inputLog' = inputLog,
    transitionLog' = transitionLog.union(Set({
      protocol: pid,
      prevState: Initialized,
      nextState: Initialized
    })),
    timers' = timers,
    timerOwnership' = timerOwnership.set(pid, Set()),
    evidence' = evidence.set(pid, None),
    rehydrated' = rehydrated,
    journalEffects' = journalEffects.set(pid, Set())
  }

  // Simplified recordInput action
  action recordInput(pid: ProtocolId, input: ProtocolInput): bool = all {
    pid.in(descriptors.keys()),
    val descriptor = descriptors.get(pid)
    val inputValid = match input {
      | TransportIn(msg) =>
          msg.session == descriptor.session and msg.recipients.size() >= 1
      | JournalIn(event) =>
          event.session == descriptor.session
      | TimerFired(info) =>
          info.timer.in(timers.keys()) and
          timers.get(info.timer).protocol == pid and
          timers.get(info.timer).active
      | LocalSignal(_) => true
    }
    inputValid,
    lifecycleStates' = lifecycleStates,
    descriptors' = descriptors,
    participants' = participants,
    outputs' = outputs,
    effectLog' = effectLog,
    inputLog' = inputLog.union(Set({protocol: pid, input: input})),
    transitionLog' = transitionLog,
    timers' = timers,
    timerOwnership' = timerOwnership,
    sessionIndex' = sessionIndex,
    evidence' = evidence,
    rehydrated' = rehydrated,
    journalEffects' = journalEffects
  }

  // Simplified processStep action
  // Note: 'next' is a built-in temporal operator, renamed to 'targetState'
  action processStep(
    pid: ProtocolId,
    targetState: ProtocolState,
    effects: Set[ProtocolEffect],
    outcome: Option[OutputState]
  ): bool = {
    val current = lifecycleStates.get(pid)
    val descriptor = descriptors.get(pid)
    val memberSet = participants.get(pid)
    val newTimerState = applyEffectTimers(timers, timerOwnership, pid, effects)
    val effectValidity = effects.forall(effect =>
      effectAllowed(pid, descriptor, memberSet, current, effect,
        {timers: timers, ownership: timerOwnership})
    )
    val outputValid = match outcome {
      | Some(result) =>
          canTerminate(targetState) and
          (successfulOutput(result) implies targetState == Completed) and
          (failedOutput(result) implies (targetState == Failed or targetState == Cancelled))
      | None => true
    }
    val newOutputs = match outcome {
      | Some(result) => outputs.set(pid, result)
      | None => outputs
    }
    all {
      pid.in(descriptors.keys()),
      transitionAllowed(current, targetState),
      isFinal(current) implies current == targetState,
      effectValidity,
      outputValid,
      lifecycleStates' = lifecycleStates.set(pid, targetState),
      descriptors' = descriptors,
      participants' = participants,
      outputs' = newOutputs,
      effectLog' = effects.fold(
        effectLog,
        (acc, effect) => acc.union(Set({protocol: pid, effect: effect}))
      ),
      journalEffects' = journalEffects,
      inputLog' = inputLog,
      transitionLog' = transitionLog.union(Set({
        protocol: pid,
        prevState: current,
        nextState: targetState
      })),
      timers' = newTimerState.timers,
      timerOwnership' = newTimerState.ownership,
      sessionIndex' = sessionIndex,
      evidence' = evidence,
      rehydrated' = rehydrated
    }
  }

  action captureEvidence(
    pid: ProtocolId,
    state: ProtocolState,
    witnessValid: bool,
    payloadHash: int
  ): bool = all {
    pid.in(descriptors.keys()),
    lifecycleStates.get(pid) == state,
    canTerminate(state),
    evidence' = evidence.set(pid, Some({
      state: state,
      witnessValid: witnessValid,
      payloadHash: payloadHash,
      status: Pending
    })),
    descriptors' = descriptors,
    lifecycleStates' = lifecycleStates,
    participants' = participants,
    outputs' = outputs,
    effectLog' = effectLog,
    inputLog' = inputLog,
    transitionLog' = transitionLog,
    timers' = timers,
    timerOwnership' = timerOwnership,
    sessionIndex' = sessionIndex,
    rehydrated' = rehydrated,
    journalEffects' = journalEffects
  }

  action validateEvidence(pid: ProtocolId): bool = {
    val recordOpt = evidence.get(pid)
    val isPresent = match recordOpt { | Some(_) => true | None => false }
    val record = match recordOpt {
      | Some(r) => r
      | None => { state: Initialized, witnessValid: false, payloadHash: 0, status: Pending }
    }
    val newStatus = if (record.witnessValid) Valid else Rejected
    all {
      pid.in(descriptors.keys()),
      isPresent,
      evidence' = evidence.set(pid, Some({
        state: record.state,
        witnessValid: record.witnessValid,
        payloadHash: record.payloadHash,
        status: newStatus
      })),
      descriptors' = descriptors,
      lifecycleStates' = lifecycleStates,
      participants' = participants,
      outputs' = outputs,
      effectLog' = effectLog,
      inputLog' = inputLog,
      transitionLog' = transitionLog,
      timers' = timers,
      timerOwnership' = timerOwnership,
      sessionIndex' = sessionIndex,
      rehydrated' = rehydrated,
      journalEffects' = journalEffects
    }
  }

  action rehydrateProtocol(pid: ProtocolId): bool = {
    val recordOpt = evidence.get(pid)
    val isPresent = match recordOpt { | Some(_) => true | None => false }
    val record = match recordOpt {
      | Some(r) => r
      | None => { state: Initialized, witnessValid: false, payloadHash: 0, status: Pending }
    }
    all {
      pid.in(descriptors.keys()),
      isPresent,
      record.status == Valid,
      lifecycleStates.get(pid) == record.state,
      not(pid.in(rehydrated)),
      rehydrated' = rehydrated.union(Set(pid)),
      lifecycleStates' = lifecycleStates.set(pid, record.state),
      evidence' = evidence,
      descriptors' = descriptors,
      participants' = participants,
      outputs' = outputs,
      effectLog' = effectLog,
      inputLog' = inputLog,
      transitionLog' = transitionLog,
      timers' = timers,
      timerOwnership' = timerOwnership,
      sessionIndex' = sessionIndex,
      journalEffects' = journalEffects
    }
  }

  // ==================== INVARIANTS ====================

  val InvariantDescriptorStateAlignment =
    descriptors.keys() == lifecycleStates.keys() and
    descriptors.keys() == outputs.keys() and
    descriptors.keys() == participants.keys()

  val InvariantSessionIndexCoversProtocols =
    descriptors.keys().forall(pid =>
      sessionIndex.keys().exists(session =>
        sessionIndex.get(session).contains(pid)
      )
    )

  val InvariantSessionIndexAccurate =
    sessionIndex.keys().forall(session =>
      sessionIndex.get(session).forall(pid =>
        pid.in(descriptors.keys()) and descriptors.get(pid).session == session
      )
    )

  val InvariantOutputsOnlyWhenTerminable =
    outputs.keys().forall(pid =>
      match outputs.get(pid) {
        | NotReady => true
        | Success(_) => canTerminate(lifecycleStates.get(pid))
        | Failure(_) => canTerminate(lifecycleStates.get(pid))
      }
    )

  val InvariantSuccessImpliesCompleted =
    outputs.keys().forall(pid =>
      match outputs.get(pid) {
        | Success(_) => lifecycleStates.get(pid) == Completed
        | _ => true
      }
    )

  val InvariantFailureImpliesFailureState =
    outputs.keys().forall(pid =>
      match outputs.get(pid) {
        | Failure(_) => lifecycleStates.get(pid) == Failed or lifecycleStates.get(pid) == Cancelled
        | _ => true
      }
    )

  val InvariantEffectProtocolsRegistered =
    effectLog.forall(record => record.protocol.in(descriptors.keys()))

  val InvariantTransitionsAllowed =
    transitionLog.forall(t =>
      t.protocol.in(descriptors.keys()) and
      transitionAllowed(t.prevState, t.nextState)
    )

  val InvariantNoTransitionsAfterFinal =
    transitionLog.forall(t =>
      isFinal(t.prevState) implies t.prevState == t.nextState
    )

  val InvariantTimersBelongToProtocols =
    timers.keys().forall(timer =>
      timers.get(timer).protocol.in(descriptors.keys())
    )

  val InvariantTimerOwnershipMatchesTimers =
    descriptors.keys().forall(pid =>
      getOrDefault(timerOwnership, pid, Set()) ==
        timers.keys().filter(timer =>
          timers.get(timer).protocol == pid and timers.get(timer).active
        )
    )

  val InvariantNoActiveTimersInFinalState =
    descriptors.keys().forall(pid =>
      isFinal(lifecycleStates.get(pid)) implies
        getOrDefault(timerOwnership, pid, Set()).size() == 0
    )

  val InvariantJournalRequiresOutput =
    effectLog.forall(record =>
      effectIsJournal(record.effect) implies
        outputs.get(record.protocol) != NotReady
    )

  val InvariantMessagesRespectParticipants =
    effectLog.forall(record =>
      effectIsMessage(record.effect) implies
        match record.effect {
          | Send(msg) =>
              msg.recipients.subseteq(participants.get(record.protocol)) or
              msg.recipients == Set()
          | Broadcast(msg) =>
              msg.recipients.subseteq(participants.get(record.protocol)) or
              msg.recipients == Set()
          | _ => true
        }
    )

  val InvariantJournalEffectsMatchSession =
    journalEffects.keys().forall(pid =>
      journalEffects.get(pid).forall(entry =>
        entry.session == descriptors.get(pid).session
      )
    )

  val InvariantEvidenceTerminatesState =
    evidence.keys().forall(pid =>
      match evidence.get(pid) {
        | Some(record) =>
            canTerminate(record.state) and
            record.state == lifecycleStates.get(pid)
        | None => true
      }
    )

  val InvariantValidEvidenceHasWitness =
    evidence.keys().forall(pid =>
      match evidence.get(pid) {
        | Some(record) =>
            record.status == Valid implies record.witnessValid
        | None => true
      }
    )

  val InvariantRehydratedProtocolsHaveValidEvidence =
    rehydrated.forall(pid =>
      pid.in(descriptors.keys()) and
      match evidence.get(pid) {
        | Some(record) =>
            record.status == Valid and
            lifecycleStates.get(pid) == record.state
        | None => false
      }
    )

  val InvariantCounterEffectsRestricted =
    effectLog.forall(record =>
      match record.effect {
        | UpdateCounter(_) =>
            descriptors.get(record.protocol).protocol == Recovery or
            descriptors.get(record.protocol).protocol == Locking
        | _ => true
      }
    )
}
