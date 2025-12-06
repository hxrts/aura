// Recovery Test Harness
//
// Test driver for guardian-based social recovery protocol.
// Wraps protocol_recovery actions for integration testing.
// Sequences: register → submitShare (threshold) → complete.

module harness_recovery {
  // ============ TYPE DEFINITIONS ============
  type ProtocolId = str
  type AuthorityId = str

  // ============ IMPORTS ============
  import protocol_recovery from "./protocol_recovery"

  // ============ CONSTANTS ============
  pure val SIGNAL_COMPLETE = "complete"
  pure val SIGNAL_ABORT = "abort"

  // ============ HARNESS STATE ============
  var harnessRegistered: Set[ProtocolId]

  action init: bool = all {
    harnessRegistered' = Set()
  }

  // ============ HARNESS ACTIONS ============

  // Configure recovery with guardians and threshold.
  action register(
    pid: ProtocolId,
    guardians: Set[AuthorityId],
    threshold: int
  ): bool = all {
    not(pid.in(harnessRegistered)),
    protocol_recovery::configureRecovery(pid, guardians, threshold),
    harnessRegistered' = harnessRegistered.union(Set(pid))
  }

  // Guardian submits their recovery share.
  action submitShare(
    pid: ProtocolId,
    guardian: AuthorityId,
    share: int
  ): bool = all {
    pid.in(harnessRegistered),
    protocol_recovery::submitShare(pid, guardian, share),
    harnessRegistered' = harnessRegistered
  }

  // Signal successful completion.
  action complete(pid: ProtocolId): bool = all {
    pid.in(harnessRegistered),
    protocol_recovery::signalComplete(pid),
    harnessRegistered' = harnessRegistered
  }

  // Signal abort/failure.
  action abort(pid: ProtocolId): bool = all {
    pid.in(harnessRegistered),
    protocol_recovery::signalAbort(pid),
    harnessRegistered' = harnessRegistered
  }

  // ============ INVARIANTS ============

  // All registered protocol IDs are non-empty.
  val InvariantHarnessTracksRegistrations =
    harnessRegistered.forall(pid => pid != "")
}
