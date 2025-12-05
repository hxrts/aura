module protocol_locking {
  // Base types (defined locally - type imports don't work in Quint)
  type Option[a] = Some(a) | None
  type ProtocolId = str
  type AuthorityId = str  // renamed from DeviceId for authority model

  // Unique lifecycle variants for locking
  type LockLifecycleStatus = LockPending | LockLifecycleCompleted | LockAborted

  type LockPhase =
    | LockIdle
    | AwaitingQuorum
    | Held
    | Releasing
    | Released
    | LockFailed

  type LockOutcome = {
    winner: AuthorityId,
    granted: bool
  }

  // Lock ticket assigned to winning participant
  type LockTicket = {
    holder: AuthorityId,
    issuedAt: int,
    ttl: int
  }

  // Simplified effect type (no imports)
  type LockEffect = { effectType: str, payload: str }

  // ============ STATE ============

  var participants: ProtocolId -> Set[AuthorityId]
  var phase: ProtocolId -> LockPhase
  var quorumSize: ProtocolId -> int
  var ballots: ProtocolId -> AuthorityId -> Option[int]
  var ticket: ProtocolId -> Option[LockTicket]
  var releaseVotes: ProtocolId -> Set[AuthorityId]
  var lifecycleStatus: ProtocolId -> LockLifecycleStatus
  var lifecycleEffects: ProtocolId -> Set[LockEffect]
  var outcome: ProtocolId -> Option[LockOutcome]

  // ============ HELPERS ============

  pure def getOrDefaultSet(m: ProtocolId -> Set[AuthorityId], key: ProtocolId): Set[AuthorityId] =
    if (key.in(m.keys())) m.get(key) else Set()

  pure def getOrDefaultInt(m: ProtocolId -> int, key: ProtocolId, default: int): int =
    if (key.in(m.keys())) m.get(key) else default

  pure def optionIsSome(opt: Option[int]): bool =
    match opt {
      | Some(_) => true
      | None => false
    }

  pure def optionIsSomeTicket(opt: Option[LockTicket]): bool =
    match opt {
      | Some(_) => true
      | None => false
    }

  pure def ensureBallotMap(
    mapping: ProtocolId -> AuthorityId -> Option[int],
    pid: ProtocolId,
    members: Set[AuthorityId]
  ): ProtocolId -> AuthorityId -> Option[int] =
    if (pid.in(mapping.keys())) mapping
    else
      mapping.set(
        pid,
        members.fold(Map(), (acc, member) => acc.set(member, None))
      )

  pure def getBallotOpt(mapping: ProtocolId -> AuthorityId -> Option[int], pid: ProtocolId, member: AuthorityId): Option[int] =
    if (pid.in(mapping.keys()))
      if (member.in(mapping.get(pid).keys())) mapping.get(pid).get(member) else None
    else None

  def countBallots(pid: ProtocolId): int =
    getOrDefaultSet(participants, pid).fold(0, (acc, member) =>
      acc + (if (optionIsSome(getBallotOpt(ballots, pid, member))) 1 else 0)
    )

  // ============ INITIALISATION ============

  action init: bool = all {
    participants' = Map(),
    phase' = Map(),
    quorumSize' = Map(),
    ballots' = Map(),
    ticket' = Map(),
    releaseVotes' = Map(),
    lifecycleStatus' = Map(),
    lifecycleEffects' = Map(),
    outcome' = Map()
  }

  // ============ ACTIONS ============

  action configureLock(
    pid: ProtocolId,
    members: Set[AuthorityId],
    quorum: int
  ): bool = all {
    members.size() >= 1,
    quorum >= 1,
    quorum <= members.size(),
    not(pid.in(participants.keys())),
    participants' = participants.set(pid, members),
    quorumSize' = quorumSize.set(pid, quorum),
    phase' = phase.set(pid, LockIdle),
    ballots' = ensureBallotMap(ballots, pid, members),
    ticket' = ticket.set(pid, None),
    releaseVotes' = releaseVotes.set(pid, Set()),
    lifecycleStatus' = lifecycleStatus.set(pid, LockPending),
    lifecycleEffects' = lifecycleEffects.set(pid, Set()),
    outcome' = outcome.set(pid, None)
  }

  action openLottery(pid: ProtocolId): bool = {
    val currentPhase = if (pid.in(phase.keys())) phase.get(pid) else LockIdle
    val isIdle = match currentPhase { | LockIdle => true | _ => false }
    all {
      isIdle,
      phase' = phase.set(pid, AwaitingQuorum),
      participants' = participants,
      quorumSize' = quorumSize,
      ballots' = ballots,
      ticket' = ticket,
      releaseVotes' = releaseVotes,
      lifecycleStatus' = lifecycleStatus,
      lifecycleEffects' = lifecycleEffects,
      outcome' = outcome
    }
  }

  action submitBallot(
    pid: ProtocolId,
    member: AuthorityId,
    ballot: int
  ): bool = {
    val currentPhase = if (pid.in(phase.keys())) phase.get(pid) else LockIdle
    val isAwaiting = match currentPhase { | AwaitingQuorum => true | _ => false }
    val members = getOrDefaultSet(participants, pid)
    val hasBallot = optionIsSome(getBallotOpt(ballots, pid, member))
    val updatedBallots = ensureBallotMap(ballots, pid, members).set(
      pid,
      ensureBallotMap(ballots, pid, members).get(pid).set(member, Some(ballot))
    )
    val newBallotCount = members.fold(0, (acc, m) =>
      acc + (if (optionIsSome(getBallotOpt(updatedBallots, pid, m))) 1 else 0)
    )
    val quorum = getOrDefaultInt(quorumSize, pid, 0)
    val hasQuorum = newBallotCount >= quorum
    val nextPhase = if (hasQuorum) Held else AwaitingQuorum
    val newTicket = if (hasQuorum) Some({
      holder: member,
      issuedAt: ballot,
      ttl: members.size()
    }) else (if (pid.in(ticket.keys())) ticket.get(pid) else None)
    all {
      isAwaiting,
      members.contains(member),
      not(hasBallot),
      ballots' = updatedBallots,
      ticket' = ticket.set(pid, newTicket),
      phase' = phase.set(pid, nextPhase),
      participants' = participants,
      quorumSize' = quorumSize,
      releaseVotes' = releaseVotes,
      lifecycleStatus' = lifecycleStatus,
      lifecycleEffects' = lifecycleEffects,
      outcome' = outcome
    }
  }

  action voteRelease(pid: ProtocolId, member: AuthorityId): bool = {
    val currentPhase = if (pid.in(phase.keys())) phase.get(pid) else LockIdle
    val isHeld = match currentPhase { | Held => true | _ => false }
    val members = getOrDefaultSet(participants, pid)
    val currentVotes = getOrDefaultSet(releaseVotes, pid)
    val updatedVotes = currentVotes.union(Set(member))
    val quorum = getOrDefaultInt(quorumSize, pid, 0)
    val shouldRelease = updatedVotes.size() >= quorum
    all {
      isHeld,
      members.contains(member),
      releaseVotes' = releaseVotes.set(pid, updatedVotes),
      phase' = if (shouldRelease) phase.set(pid, Releasing) else phase,
      participants' = participants,
      quorumSize' = quorumSize,
      ballots' = ballots,
      ticket' = ticket,
      lifecycleStatus' = lifecycleStatus,
      lifecycleEffects' = lifecycleEffects,
      outcome' = outcome
    }
  }

  action finalizeRelease(pid: ProtocolId): bool = {
    val currentPhase = if (pid.in(phase.keys())) phase.get(pid) else LockIdle
    val isReleasing = match currentPhase { | Releasing => true | _ => false }
    all {
      isReleasing,
      phase' = phase.set(pid, Released),
      ticket' = ticket.set(pid, None),
      releaseVotes' = releaseVotes.set(pid, Set()),
      participants' = participants,
      quorumSize' = quorumSize,
      ballots' = ballots,
      lifecycleStatus' = lifecycleStatus,
      lifecycleEffects' = lifecycleEffects,
      outcome' = outcome
    }
  }

  action failLock(pid: ProtocolId): bool = {
    val currentPhase = if (pid.in(phase.keys())) phase.get(pid) else LockIdle
    val isNotReleased = match currentPhase { | Released => false | _ => true }
    all {
      isNotReleased,
      phase' = phase.set(pid, LockFailed),
      lifecycleStatus' = lifecycleStatus.set(pid, LockAborted),
      participants' = participants,
      quorumSize' = quorumSize,
      ballots' = ballots,
      ticket' = ticket,
      releaseVotes' = releaseVotes,
      lifecycleEffects' = lifecycleEffects,
      outcome' = outcome
    }
  }

  action signalComplete(pid: ProtocolId): bool = {
    val status = if (pid.in(lifecycleStatus.keys())) lifecycleStatus.get(pid) else LockPending
    val isPending = match status { | LockPending => true | _ => false }
    val currentPhase = if (pid.in(phase.keys())) phase.get(pid) else LockIdle
    val isReleased = match currentPhase { | Released => true | _ => false }
    val ticketOpt = if (pid.in(ticket.keys())) ticket.get(pid) else None
    val hasWinner = match ticketOpt { | Some(_) => true | None => false }
    val winner = match ticketOpt { | Some(t) => t.holder | None => "" }
    val members = getOrDefaultSet(participants, pid)
    all {
      isPending,
      isReleased,
      lifecycleStatus' = lifecycleStatus.set(pid, LockLifecycleCompleted),
      lifecycleEffects' = lifecycleEffects.set(pid, Set()),
      outcome' = outcome.set(pid, Some({
        winner: winner,
        granted: true
      })),
      participants' = participants,
      quorumSize' = quorumSize,
      phase' = phase,
      ballots' = ballots,
      ticket' = ticket,
      releaseVotes' = releaseVotes
    }
  }

  action signalAbort(pid: ProtocolId): bool = {
    val status = if (pid.in(lifecycleStatus.keys())) lifecycleStatus.get(pid) else LockPending
    val isPending = match status { | LockPending => true | _ => false }
    all {
      isPending,
      lifecycleStatus' = lifecycleStatus.set(pid, LockAborted),
      lifecycleEffects' = lifecycleEffects,
      phase' = phase.set(pid, LockFailed),
      participants' = participants,
      quorumSize' = quorumSize,
      ballots' = ballots,
      ticket' = ticket,
      releaseVotes' = releaseVotes,
      outcome' = outcome
    }
  }

  // ============ INVARIANTS ============

  val InvariantQuorumWithinBounds =
    quorumSize.keys().forall(pid =>
      quorumSize.get(pid) >= 1 and
      quorumSize.get(pid) <= getOrDefaultSet(participants, pid).size()
    )

  val InvariantBallotsFromParticipants =
    ballots.keys().forall(pid =>
      ensureBallotMap(ballots, pid, getOrDefaultSet(participants, pid))
        .get(pid)
        .keys()
        .subseteq(getOrDefaultSet(participants, pid))
    )

  val InvariantTicketOnlyWhenHeld =
    ticket.keys().forall(pid => {
      val ticketOpt = ticket.get(pid)
      val hasTicket = match ticketOpt { | Some(_) => true | None => false }
      val currentPhase = if (pid.in(phase.keys())) phase.get(pid) else LockIdle
      val isHeldOrReleasing = match currentPhase { | Held => true | Releasing => true | _ => false }
      hasTicket implies isHeldOrReleasing
    })

  val InvariantReleaseVotesSubsetParticipants =
    releaseVotes.keys().forall(pid =>
      getOrDefaultSet(releaseVotes, pid).subseteq(getOrDefaultSet(participants, pid))
    )

  val InvariantPhaseProgression =
    phase.keys().forall(pid =>
      match phase.get(pid) {
        | LockIdle => true
        | AwaitingQuorum =>
            not(optionIsSomeTicket(if (pid.in(ticket.keys())) ticket.get(pid) else None))
        | Held =>
            optionIsSomeTicket(if (pid.in(ticket.keys())) ticket.get(pid) else None)
        | Releasing =>
            getOrDefaultSet(releaseVotes, pid).size() >= getOrDefaultInt(quorumSize, pid, 0)
        | Released =>
            not(optionIsSomeTicket(if (pid.in(ticket.keys())) ticket.get(pid) else None))
        | LockFailed => true
      }
    )

  val InvariantLifecycleStatusDefined =
    lifecycleStatus.keys() == phase.keys()

  val InvariantLifecycleCompletionRequiresPhase =
    lifecycleStatus.keys().forall(pid => {
      val status = lifecycleStatus.get(pid)
      val currentPhase = if (pid.in(phase.keys())) phase.get(pid) else LockIdle
      val isCompleted = match status { | LockLifecycleCompleted => true | _ => false }
      val isReleased = match currentPhase { | Released => true | _ => false }
      isCompleted implies isReleased
    })

  val InvariantLifecycleEffectsEmpty =
    lifecycleEffects.keys().forall(pid => lifecycleEffects.get(pid).size() == 0)

  val InvariantOutcomeWhenCompleted =
    lifecycleStatus.keys().forall(pid => {
      val status = lifecycleStatus.get(pid)
      val isCompleted = match status { | LockLifecycleCompleted => true | _ => false }
      val outcomeOpt = if (pid.in(outcome.keys())) outcome.get(pid) else None
      val hasOutcome = match outcomeOpt { | Some(_) => true | None => false }
      isCompleted implies hasOutcome
    })
}
