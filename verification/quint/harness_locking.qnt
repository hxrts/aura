// Locking Test Harness
//
// Test driver for distributed locking protocol.
// Wraps protocol_locking actions for integration testing.
// Sequences: register → openLottery → submitBallot → voteRelease → complete.

module harness_locking {
  // ============ TYPE DEFINITIONS ============
  type ProtocolId = str
  type AuthorityId = str

  // ============ IMPORTS ============
  import protocol_locking from "./protocol_locking"

  // ============ CONSTANTS ============
  pure val SIGNAL_COMPLETE = "complete"
  pure val SIGNAL_ABORT = "abort"

  // ============ HARNESS STATE ============
  var harnessRegistered: Set[ProtocolId]

  action init: bool = all {
    harnessRegistered' = Set()
  }

  // ============ HARNESS ACTIONS ============

  // Configure locking protocol with participants and quorum.
  action register(
    pid: ProtocolId,
    members: Set[AuthorityId],
    quorum: int
  ): bool = all {
    not(pid.in(harnessRegistered)),
    protocol_locking::configureLock(pid, members, quorum),
    harnessRegistered' = harnessRegistered.union(Set(pid))
  }

  // Start lottery for lock acquisition.
  action openLottery(pid: ProtocolId): bool = all {
    pid.in(harnessRegistered),
    protocol_locking::openLottery(pid),
    harnessRegistered' = harnessRegistered
  }

  // Submit ballot from participant.
  action submitBallot(
    pid: ProtocolId,
    member: AuthorityId,
    ballot: int
  ): bool = all {
    pid.in(harnessRegistered),
    protocol_locking::submitBallot(pid, member, ballot),
    harnessRegistered' = harnessRegistered
  }

  // Vote to release the lock.
  action voteRelease(
    pid: ProtocolId,
    member: AuthorityId
  ): bool = all {
    pid.in(harnessRegistered),
    protocol_locking::voteRelease(pid, member),
    harnessRegistered' = harnessRegistered
  }

  // Finalize lock release (clear ticket).
  action finalizeRelease(pid: ProtocolId): bool = all {
    pid.in(harnessRegistered),
    protocol_locking::finalizeRelease(pid),
    harnessRegistered' = harnessRegistered
  }

  // Signal successful completion.
  action complete(pid: ProtocolId): bool = all {
    pid.in(harnessRegistered),
    protocol_locking::signalComplete(pid),
    harnessRegistered' = harnessRegistered
  }

  // Signal abort/failure.
  action abort(pid: ProtocolId): bool = all {
    pid.in(harnessRegistered),
    protocol_locking::signalAbort(pid),
    harnessRegistered' = harnessRegistered
  }

  // ============ INVARIANTS ============

  // All registered protocol IDs are non-empty.
  val InvariantHarnessTracksRegistrations =
    harnessRegistered.forall(pid => pid != "")
}
