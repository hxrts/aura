module harness_locking {
  // Local type definitions (imports don't work for types in Quint)
  type ProtocolId = str
  type AuthorityId = str

  // Import modules for actions (not types)
  import protocol_locking from "./protocol_locking"

  // Constants for signals
  pure val SIGNAL_COMPLETE = "complete"
  pure val SIGNAL_ABORT = "abort"

  // Harness state - tracks which protocols have been registered via harness
  var harnessRegistered: Set[ProtocolId]

  action init: bool = all {
    harnessRegistered' = Set()
  }

  /// Register a locking lifecycle.
  action register(
    pid: ProtocolId,
    members: Set[AuthorityId],
    quorum: int
  ): bool = all {
    not(pid.in(harnessRegistered)),
    protocol_locking::configureLock(pid, members, quorum),
    harnessRegistered' = harnessRegistered.union(Set(pid))
  }

  /// Open the lottery phase.
  action openLottery(pid: ProtocolId): bool = all {
    pid.in(harnessRegistered),
    protocol_locking::openLottery(pid),
    harnessRegistered' = harnessRegistered
  }

  /// Submit a ballot for a contender.
  action submitBallot(
    pid: ProtocolId,
    member: AuthorityId,
    ballot: int
  ): bool = all {
    pid.in(harnessRegistered),
    protocol_locking::submitBallot(pid, member, ballot),
    harnessRegistered' = harnessRegistered
  }

  /// Cast a release vote.
  action voteRelease(
    pid: ProtocolId,
    member: AuthorityId
  ): bool = all {
    pid.in(harnessRegistered),
    protocol_locking::voteRelease(pid, member),
    harnessRegistered' = harnessRegistered
  }

  /// Finalize release before completion.
  action finalizeRelease(pid: ProtocolId): bool = all {
    pid.in(harnessRegistered),
    protocol_locking::finalizeRelease(pid),
    harnessRegistered' = harnessRegistered
  }

  /// Complete the locking lifecycle.
  action complete(pid: ProtocolId): bool = all {
    pid.in(harnessRegistered),
    protocol_locking::signalComplete(pid),
    harnessRegistered' = harnessRegistered
  }

  /// Abort the locking lifecycle.
  action abort(pid: ProtocolId): bool = all {
    pid.in(harnessRegistered),
    protocol_locking::signalAbort(pid),
    harnessRegistered' = harnessRegistered
  }

  // ============ INVARIANTS ============

  val InvariantHarnessTracksRegistrations =
    harnessRegistered.forall(pid => pid != "")
}
