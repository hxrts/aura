// Protocol Core - Aura Protocol State Machine
//
// This module defines the core types and state machine for Aura protocols.
// It serves as the foundation for all protocol specifications: DKG, FROST,
// recovery, consensus, etc. Each protocol instance progresses through a
// well-defined lifecycle (Initialized → Active → Completed/Failed).
//
// Key design principles:
// - AuthorityId is opaque (no device internals exposed)
// - Effects capture all observable protocol actions (messages, journal, timers)
// - State transitions are validated against an allowed transition graph
// - Evidence records enable protocol checkpoint/rehydration after crash
//
// See: docs/100_authority_and_identity.md, docs/103_relational_contexts.md

module protocol_core {

  // ==================== TYPE DEFINITIONS ====================
  // These types form the vocabulary shared by all protocol modules.

  type Option[a] = Some(a) | None

  // Identifiers are opaque strings to avoid leaking internal structure.
  type ProtocolId = str
  type SessionId = str
  type AuthorityId = str   // Opaque authority identifier (was DeviceId)
  type TimerId = str
  type ContextId = str     // Relational context identifier (was RelationshipId)

  // ProtocolKind tags which distributed algorithm is running.
  // Each kind has its own module with detailed state machine.
  type ProtocolKind =
    | Dkd              // Distributed Key Derivation (new key from existing shares)
    | Resharing        // Re-share existing key to new participant set
    | Recovery         // Guardian-assisted account recovery
    | Locking          // Distributed lock acquisition
    | CounterReservation  // Lease counter ranges for conflict-free allocation
    | CustomKind(str)

  // OperationKind mirrors ProtocolKind for effect routing.
  type OperationKind =
    | DkdOp
    | ResharingOp
    | RecoveryOp
    | LockingOp
    | CounterOp
    | CustomOp(str)

  // Priority affects scheduling when multiple protocols compete for resources.
  type ProtocolPriority = Low | Normal | High | Critical
  // Mode affects how the runtime executes the protocol.
  type ProtocolMode = Asynchronous | Interactive | Deterministic

  // ProtocolDescriptor captures immutable metadata set at registration time.
  type ProtocolDescriptor = {
    session: SessionId,
    authority: AuthorityId,  // The local authority running this protocol instance
    protocol: ProtocolKind,
    operation: OperationKind,
    priority: ProtocolPriority,
    mode: ProtocolMode
  }

  // ProtocolState models the lifecycle of a protocol instance.
  // Transitions follow ALLOWED_TRANSITIONS graph (no backward moves).
  type ProtocolState =
    | Initialized       // Registered but not yet started
    | Active            // Processing messages/inputs
    | AwaitingEvidence  // Waiting for external proof (e.g., consensus finality)
    | Completed         // Successfully finished
    | Failed            // Unrecoverable error
    | Cancelled         // Aborted by user or timeout

  // MessageEffect records an outgoing protocol message.
  // Note: 'from' is reserved in Quint, using 'sender' instead.
  type MessageEffect = {
    sender: AuthorityId,
    recipients: Set[AuthorityId],  // Who receives this message
    session: SessionId,            // Scoped to this session only
    payloadHash: int               // Abstract hash (no actual crypto here)
  }

  // JournalEffect records a fact append to the local journal.
  type JournalEffect = {
    session: SessionId,
    eventType: str,
    payloadHash: int
  }

  // TimerEffect schedules a timeout callback.
  type TimerEffect = {
    timer: TimerId,
    timeout: int  // Abstract time units
  }

  // CounterEffect leases a range of counter values for conflict-free allocation.
  type CounterEffect = {
    context: ContextId,  // Which relational context owns this counter
    previous: int,       // Last known counter value
    reserved: Set[int],  // Newly leased values
    ttl: int,            // Lease duration
    epoch: int,          // Epoch for stale lease detection
    requestingAuthority: AuthorityId
  }

  // TraceEffect is debug output (no semantic effect).
  type TraceEffect = {
    message: str
  }

  // ProtocolEffect enumerates all observable side effects a protocol can emit.
  // The guard chain validates these before they reach the network/journal.
  type ProtocolEffect =
    | Send(MessageEffect)         // Point-to-point message
    | Broadcast(MessageEffect)    // Multi-recipient message
    | AppendJournal(JournalEffect)  // Persist fact to journal
    | ScheduleTimer(TimerEffect)  // Set timeout
    | CancelTimer(TimerId)        // Cancel pending timeout
    | Trace(TraceEffect)          // Debug logging
    | UpdateCounter(CounterEffect)  // Lease counter range

  // ProtocolInput enumerates all events that can drive protocol state forward.
  type ProtocolInput =
    | TransportIn(MessageEffect)    // Incoming message from peer
    | JournalIn(JournalEffect)      // Replayed fact from journal
    | TimerFired({timer: TimerId})  // Timeout elapsed
    | LocalSignal({signal: str})    // User/system request

  // OutputState tracks whether the protocol has produced a final result.
  type OutputState =
    | NotReady                      // Still in progress
    | Success({commitHash: int})    // Completed with this outcome
    | Failure({reason: str})        // Failed with this error

  // Evidence lifecycle: Pending → Valid/Rejected after witness check.
  type EvidenceStatus = Pending | Valid | Rejected

  // EvidenceRecord snapshots protocol state for crash recovery.
  // After crash, valid evidence allows rehydrating to the saved state.
  type EvidenceRecord = {
    state: ProtocolState,
    witnessValid: bool,   // Did external witness confirm this checkpoint?
    payloadHash: int,
    status: EvidenceStatus
  }

  // TransitionEvent logs state changes for audit/debugging.
  // Note: 'from'/'to' are reserved, using 'prevState'/'nextState'.
  type TransitionEvent = {
    protocol: ProtocolId,
    prevState: ProtocolState,
    nextState: ProtocolState
  }

  // EffectRecord logs emitted effects for invariant checking.
  type EffectRecord = {
    protocol: ProtocolId,
    effect: ProtocolEffect
  }

  // InputRecord logs received inputs for replay/debugging.
  type InputRecord = {
    protocol: ProtocolId,
    input: ProtocolInput
  }

  // TimerEntry tracks a single scheduled timeout.
  type TimerEntry = {
    protocol: ProtocolId,  // Which protocol owns this timer
    timeout: int,
    active: bool           // False if cancelled or fired
  }

  // TimerState aggregates all timers with ownership tracking.
  type TimerState = {
    timers: TimerId -> TimerEntry,
    ownership: ProtocolId -> Set[TimerId]  // Active timers per protocol
  }

  // ==================== PURE HELPERS ====================
  // These define the protocol lifecycle rules.

  // Final states: once reached, no further progress is possible.
  pure val FINAL_STATES: Set[ProtocolState] = Set(Completed, Failed, Cancelled)
  // Termination-accepting: can produce output and potentially finalize.
  pure val TERMINATION_ACCEPTING: Set[ProtocolState] = FINAL_STATES.union(Set(AwaitingEvidence))

  // Allowed state transitions form a DAG (no backward moves except self-loops).
  // This prevents protocols from "un-completing" or "un-failing".
  pure val ALLOWED_TRANSITIONS: Set[(ProtocolState, ProtocolState)] = Set(
    (Initialized, Active),
    (Initialized, Cancelled),
    (Active, Active),              // Self-loop: processing multiple messages
    (Active, AwaitingEvidence),
    (Active, Completed),
    (Active, Failed),
    (Active, Cancelled),
    (AwaitingEvidence, AwaitingEvidence),  // Self-loop: waiting for witness
    (AwaitingEvidence, Completed),
    (AwaitingEvidence, Failed),
    (AwaitingEvidence, Cancelled)
  )

  // True if state is terminal (no further transitions possible).
  pure def isFinal(state: ProtocolState): bool =
    FINAL_STATES.contains(state)

  // True if state can emit output and potentially finalize.
  pure def canTerminate(state: ProtocolState): bool =
    TERMINATION_ACCEPTING.contains(state)

  // True if transitioning from prevState to nextState is valid.
  // Note: renamed params from 'from'/'to' (reserved), 'next' is also built-in.
  pure def transitionAllowed(prevState: ProtocolState, nextState: ProtocolState): bool =
    prevState == nextState or ALLOWED_TRANSITIONS.contains((prevState, nextState))

  // Map lookup with default value if key missing.
  pure def getOrDefault(m, key, default) =
    if (key.in(m.keys())) m.get(key) else default

  // Ensure protocol has an ownership entry (initialize to empty set if missing).
  pure def ensureOwnership(
    ownership: ProtocolId -> Set[TimerId],
    pid: ProtocolId
  ): ProtocolId -> Set[TimerId] =
    if (pid.in(ownership.keys())) ownership
    else ownership.set(pid, Set())

  // Process a single timer effect (schedule or cancel).
  // Validates ownership: only the protocol that scheduled a timer can cancel it.
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
      // CancelTimer: mark as inactive and remove from ownership set.
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
      | _ => acc  // Non-timer effects are ignored
    }

  // Apply all timer effects from a set (used during processStep).
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

  // True if sender is the protocol owner or a registered participant.
  pure def isParticipantsSender(
    participantSet: Set[AuthorityId],
    descriptor: ProtocolDescriptor,
    senderAuth: AuthorityId
  ): bool =
    senderAuth == descriptor.authority or senderAuth.in(participantSet)

  // Validate whether an effect is legal given current state.
  // This is the core guard logic: prevents invalid messages, unauthorized
  // journal writes, and timer conflicts.
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

  // Effect type predicates for invariant checking.
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

  // Output state predicates.
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

  // Evidence accessors (safely extract from Option).
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
  // Global mutable state tracked by the model checker.

  var descriptors: ProtocolId -> ProtocolDescriptor   // Immutable metadata per protocol
  var lifecycleStates: ProtocolId -> ProtocolState    // Current state per protocol
  var sessionIndex: SessionId -> Set[ProtocolId]      // Reverse index: session → protocols
  var participants: ProtocolId -> Set[AuthorityId]    // Registered participants per protocol
  var outputs: ProtocolId -> OutputState              // Final result (if any)
  var effectLog: Set[EffectRecord]                    // All emitted effects (for invariants)
  var inputLog: Set[InputRecord]                      // All received inputs (for replay)
  var transitionLog: Set[TransitionEvent]             // All state transitions (for audit)
  var timers: TimerId -> TimerEntry                   // Active and inactive timers
  var timerOwnership: ProtocolId -> Set[TimerId]      // Active timers per protocol
  var evidence: ProtocolId -> Option[EvidenceRecord]  // Checkpoint for crash recovery
  var rehydrated: Set[ProtocolId]                     // Protocols restored from evidence
  var journalEffects: ProtocolId -> Set[JournalEffect]  // Journal effects per protocol

  // ==================== INITIALISATION ====================
  // Start with empty state (no protocols registered).

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
  // These actions model the protocol runtime's API.

  // Register a new protocol instance. Fails if pid already exists.
  // Sets initial state to Initialized and output to NotReady.
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

  // Record an incoming event (message, timer, signal) for a protocol.
  // Validates that the input matches the protocol's session.
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

  // Core state machine step: transition to targetState, emit effects, set outcome.
  // This is the workhorse action that drives protocol progress.
  // Note: 'next' is a built-in temporal operator, renamed to 'targetState'.
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

  // Snapshot protocol state for crash recovery.
  // Creates a pending evidence record; must be validated before rehydration.
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

  // Validate pending evidence: sets status to Valid or Rejected based on witness.
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

  // Restore protocol from validated evidence after crash.
  // Only succeeds if evidence is Valid and protocol hasn't been rehydrated yet.
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
  // These properties must hold in every reachable state.
  // The model checker explores all executions to verify them.

  // All protocol maps are consistently keyed.
  val InvariantDescriptorStateAlignment =
    descriptors.keys() == lifecycleStates.keys() and
    descriptors.keys() == outputs.keys() and
    descriptors.keys() == participants.keys()

  // Every protocol is indexed by its session.
  val InvariantSessionIndexCoversProtocols =
    descriptors.keys().forall(pid =>
      sessionIndex.keys().exists(session =>
        sessionIndex.get(session).contains(pid)
      )
    )

  // Session index is accurate: indexed protocols actually have that session.
  val InvariantSessionIndexAccurate =
    sessionIndex.keys().forall(session =>
      sessionIndex.get(session).forall(pid =>
        pid.in(descriptors.keys()) and descriptors.get(pid).session == session
      )
    )

  // Outputs are only set when protocol can terminate.
  val InvariantOutputsOnlyWhenTerminable =
    outputs.keys().forall(pid =>
      match outputs.get(pid) {
        | NotReady => true
        | Success(_) => canTerminate(lifecycleStates.get(pid))
        | Failure(_) => canTerminate(lifecycleStates.get(pid))
      }
    )

  // Success output ⟹ state is Completed.
  val InvariantSuccessImpliesCompleted =
    outputs.keys().forall(pid =>
      match outputs.get(pid) {
        | Success(_) => lifecycleStates.get(pid) == Completed
        | _ => true
      }
    )

  // Failure output ⟹ state is Failed or Cancelled.
  val InvariantFailureImpliesFailureState =
    outputs.keys().forall(pid =>
      match outputs.get(pid) {
        | Failure(_) => lifecycleStates.get(pid) == Failed or lifecycleStates.get(pid) == Cancelled
        | _ => true
      }
    )

  // All logged effects come from registered protocols.
  val InvariantEffectProtocolsRegistered =
    effectLog.forall(record => record.protocol.in(descriptors.keys()))

  // All logged transitions follow the allowed transition graph.
  val InvariantTransitionsAllowed =
    transitionLog.forall(t =>
      t.protocol.in(descriptors.keys()) and
      transitionAllowed(t.prevState, t.nextState)
    )

  // Final states are absorbing: no transition changes them.
  val InvariantNoTransitionsAfterFinal =
    transitionLog.forall(t =>
      isFinal(t.prevState) implies t.prevState == t.nextState
    )

  // Every timer is owned by a registered protocol.
  val InvariantTimersBelongToProtocols =
    timers.keys().forall(timer =>
      timers.get(timer).protocol.in(descriptors.keys())
    )

  // Ownership set matches active timers for each protocol.
  val InvariantTimerOwnershipMatchesTimers =
    descriptors.keys().forall(pid =>
      getOrDefault(timerOwnership, pid, Set()) ==
        timers.keys().filter(timer =>
          timers.get(timer).protocol == pid and timers.get(timer).active
        )
    )

  // No active timers for protocols in final states (cleanup guarantee).
  val InvariantNoActiveTimersInFinalState =
    descriptors.keys().forall(pid =>
      isFinal(lifecycleStates.get(pid)) implies
        getOrDefault(timerOwnership, pid, Set()).size() == 0
    )

  // Journal writes only happen after output is set (charge-before-send).
  val InvariantJournalRequiresOutput =
    effectLog.forall(record =>
      effectIsJournal(record.effect) implies
        outputs.get(record.protocol) != NotReady
    )

  // Messages only go to registered participants (no message leakage).
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

  // Journal effects are scoped to their protocol's session.
  val InvariantJournalEffectsMatchSession =
    journalEffects.keys().forall(pid =>
      journalEffects.get(pid).forall(entry =>
        entry.session == descriptors.get(pid).session
      )
    )

  // Evidence can only be captured in terminable states.
  val InvariantEvidenceTerminatesState =
    evidence.keys().forall(pid =>
      match evidence.get(pid) {
        | Some(record) =>
            canTerminate(record.state) and
            record.state == lifecycleStates.get(pid)
        | None => true
      }
    )

  // Valid evidence requires a valid witness (external confirmation).
  val InvariantValidEvidenceHasWitness =
    evidence.keys().forall(pid =>
      match evidence.get(pid) {
        | Some(record) =>
            record.status == Valid implies record.witnessValid
        | None => true
      }
    )

  // Rehydrated protocols have valid evidence matching current state.
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

  // Counter updates only allowed for Recovery/Locking protocols.
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
