module protocol_locking {
  import protocol_core as core from "protocol_core"

  type Option[a] = Some(a) | None

  type ProtocolId = core.ProtocolId
  type DeviceId = core.DeviceId
  type LifecycleStatus = Pending | Completed | Aborted

type LockPhase =
  | Idle
  | AwaitingQuorum
  | Held
  | Releasing
  | Released
  | Failed

type LockOutcome = {
  winner: DeviceId,
  granted: bool
}

  // Lock ticket assigned to winning participant
  type LockTicket = {
    holder: DeviceId,
    issuedAt: int,
    ttl: int
  }

  // ============ STATE ============

  var participants: ProtocolId -> Set[DeviceId]
  var phase: ProtocolId -> LockPhase
  var quorumSize: ProtocolId -> int
  var ballots: ProtocolId -> DeviceId -> Option[int]
var ticket: ProtocolId -> Option[LockTicket]
var releaseVotes: ProtocolId -> Set[DeviceId]
var lifecycleStatus: ProtocolId -> LifecycleStatus
var lifecycleEffects: ProtocolId -> Set[core.ProtocolEffect]
var outcome: ProtocolId -> Option[LockOutcome]

  // ============ HELPERS ============

  pure def getOrDefault[K, V](m: K -> V, key: K, default: V): V =
    if key.in(m.keys()) then m.get(key) else default

  pure def optionIsSome[a](opt: Option[a]): bool =
    match opt {
      | Some(_) => true
      | None => false
    }

  pure def ensureBallotMap(
    mapping: ProtocolId -> DeviceId -> Option[int],
    pid: ProtocolId,
    members: Set[DeviceId]
  ): ProtocolId -> DeviceId -> Option[int] =
    if pid.in(mapping.keys()) then mapping
    else
      mapping.set(
        pid,
        members.fold(Map(), (acc, member) => acc.set(member, None))
      )

  pure def countBallots(pid: ProtocolId): int =
    getOrDefault(participants, pid, Set()).fold(
      0,
      (acc, member) =>
        acc +
        (if optionIsSome(getOrDefault(ballots.get(pid), member, None)) then 1 else 0)
    )

  pure def winningTicket(pid: ProtocolId): Option[LockTicket] =
    ticket.get(pid)

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
    members: Set[DeviceId],
    quorum: int
  ): bool = all {
    members.size() >= 1,
    quorum >= 1,
    quorum <= members.size(),
    not pid.in(participants.keys()),
    participants' = participants.set(pid, members),
    quorumSize' = quorumSize.set(pid, quorum),
    phase' = phase.set(pid, Idle),
    ballots' = ensureBallotMap(ballots, pid, members),
    ticket' = ticket.set(pid, None),
    releaseVotes' = releaseVotes.set(pid, Set()),
    lifecycleStatus' = lifecycleStatus.set(pid, Pending),
    lifecycleEffects' = lifecycleEffects.set(pid, Set()),
    outcome' = outcome.set(pid, None)
  }

  action openLottery(pid: ProtocolId): bool =
    val current = phase.get(pid) {
      current = Idle {
        all {
          phase' = phase.set(pid, AwaitingQuorum),
          participants' = participants,
          quorumSize' = quorumSize,
          ballots' = ballots,
          ticket' = ticket,
          releaseVotes' = releaseVotes
        }
      }
    }

  action submitBallot(
    pid: ProtocolId,
    member: DeviceId,
    ballot: int
  ): bool =
    val current = phase.get(pid) {
    val members = participants.get(pid) {
      current = AwaitingQuorum and
      members.contains(member) and
      not optionIsSome(getOrDefault(ballots.get(pid), member, None)) {
        val updatedBallots =
          ensureBallotMap(ballots, pid, members).set(
            pid,
            ensureBallotMap(ballots, pid, members).get(pid).set(member, Some(ballot))
          ) {
        val hasQuorum = countBallots(pid) + 1 >= quorumSize.get(pid) {
        val nextPhase =
          if hasQuorum then Held else AwaitingQuorum {
          val newTicket =
            if hasQuorum then
              Some({
                holder: member,
                issuedAt: ballot,
                ttl: members.size()
              })
            else
              ticket.get(pid) {
            all {
              ballots' = updatedBallots,
              ticket' = ticket.set(pid, newTicket),
              phase' = phase.set(pid, nextPhase),
              participants' = participants,
              quorumSize' = quorumSize,
              releaseVotes' = releaseVotes
            }
          }
        }
      }
    }}

  action voteRelease(pid: ProtocolId, member: DeviceId): bool =
    val current = phase.get(pid) {
    val members = participants.get(pid) {
      current = Held and members.contains(member) {
        val updatedVotes =
          getOrDefault(releaseVotes, pid, Set()).union(Set(member)) {
        val release =
          updatedVotes.size() >= quorumSize.get(pid) {
          all {
            releaseVotes' = releaseVotes.set(pid, updatedVotes),
            phase' = (if release then phase.set(pid, Releasing) else phase),
            participants' = participants,
            quorumSize' = quorumSize,
            ballots' = ballots,
            ticket' = ticket
          }
        }
      }
    }}

  action finalizeRelease(pid: ProtocolId): bool =
    val current = phase.get(pid) {
      current = Releasing {
        all {
          phase' = phase.set(pid, Released),
          ticket' = ticket.set(pid, None),
          releaseVotes' = releaseVotes.set(pid, Set()),
          participants' = participants,
          quorumSize' = quorumSize,
          ballots' = ballots
        }
      }
    }

  action failLock(pid: ProtocolId): bool =
    val current = phase.get(pid) {
      not current = Released {
        all {
          phase' = phase.set(pid, Failed),
          participants' = participants,
          quorumSize' = quorumSize,
          ballots' = ballots,
          ticket' = ticket,
          releaseVotes' = releaseVotes,
          lifecycleStatus' = lifecycleStatus.set(pid, Aborted),
          lifecycleEffects' = lifecycleEffects
        }
      }
    }

  action signalComplete(pid: ProtocolId): bool =
    val status = lifecycleStatus.get(pid) {
    val current = phase.get(pid) {
    val ticketOpt = ticket.get(pid) {
    val hasWinner = match ticketOpt {
      | Some(_) => true
      | None => false
    } {
      status = Pending and current = Released and hasWinner {
        lifecycleStatus' = lifecycleStatus.set(pid, Completed),
        lifecycleEffects' = lifecycleEffects.set(pid, Set()),
        participants' = participants,
        quorumSize' = quorumSize,
        phase' = phase,
        ballots' = ballots,
        ticket' = ticket,
        releaseVotes' = releaseVotes,
        outcome' = outcome.set(pid, Some({
          winner: match ticketOpt {
            | Some(ticket) => ticket.holder
            | None => participants.get(pid).oneOf()
          },
          granted: true
        }))
      }
    }}}}

  action signalAbort(pid: ProtocolId): bool =
    val status = lifecycleStatus.get(pid) {
      status = Pending {
        lifecycleStatus' = lifecycleStatus.set(pid, Aborted),
        lifecycleEffects' = lifecycleEffects,
        phase' = phase.set(pid, Failed),
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
      quorumSize.get(pid) <= getOrDefault(participants, pid, Set()).size()
    )

  val InvariantBallotsFromParticipants =
    ballots.keys().forall(pid =>
      ensureBallotMap(ballots, pid, getOrDefault(participants, pid, Set()))
        .get(pid)
        .keys()
        .subseteq(getOrDefault(participants, pid, Set()))
    )

  val InvariantTicketOnlyWhenHeld =
    ticket.keys().forall(pid =>
      match ticket.get(pid) {
        | Some(_) => phase.get(pid) = Held or phase.get(pid) = Releasing
        | None => true
      }
    )

  val InvariantReleaseVotesSubsetParticipants =
    releaseVotes.keys().forall(pid =>
      getOrDefault(releaseVotes, pid, Set()).subseteq(getOrDefault(participants, pid, Set()))
    )

  val InvariantPhaseProgression =
    phase.keys().forall(pid =>
      match phase.get(pid) {
        | Idle => true
        | AwaitingQuorum =>
            ticket.get(pid) = None
        | Held =>
            match ticket.get(pid) {
              | Some(_) => true
              | None => false
            }
        | Releasing =>
            getOrDefault(releaseVotes, pid, Set()).size() >= quorumSize.get(pid)
        | Released =>
            ticket.get(pid) = None
        | Failed => true
      }
    )

  val InvariantLifecycleStatusDefined =
    lifecycleStatus.keys() = phase.keys()

  val InvariantLifecycleCompletionRequiresPhase =
    lifecycleStatus.keys().forall(pid =>
      lifecycleStatus.get(pid) = Completed implies phase.get(pid) = Released
    )

  val InvariantLifecycleEffectsEmpty =
    lifecycleEffects.keys().forall(pid => lifecycleEffects.get(pid).size() = 0)

  val InvariantOutcomeWhenCompleted =
    lifecycleStatus.keys().forall(pid =>
      lifecycleStatus.get(pid) = Completed implies outcome.get(pid).isSome()
    )
}
