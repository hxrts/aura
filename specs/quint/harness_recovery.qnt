module harness_recovery {
  // Local type definitions (imports don't work for types in Quint)
  type ProtocolId = str
  type AuthorityId = str

  // Import modules for actions (not types)
  import protocol_recovery from "./protocol_recovery"

  // Constants for signals
  pure val SIGNAL_COMPLETE = "complete"
  pure val SIGNAL_ABORT = "abort"

  // Harness state - tracks which protocols have been registered via harness
  var harnessRegistered: Set[ProtocolId]

  action init: bool = all {
    harnessRegistered' = Set()
  }

  /// Register a recovery lifecycle across core + spec.
  action register(
    pid: ProtocolId,
    guardians: Set[AuthorityId],
    threshold: int
  ): bool = all {
    not(pid.in(harnessRegistered)),
    protocol_recovery::configureRecovery(pid, guardians, threshold),
    harnessRegistered' = harnessRegistered.union(Set(pid))
  }

  /// Submit a guardian share.
  action submitShare(
    pid: ProtocolId,
    guardian: AuthorityId,
    share: int
  ): bool = all {
    pid.in(harnessRegistered),
    protocol_recovery::submitShare(pid, guardian, share),
    harnessRegistered' = harnessRegistered
  }

  /// Complete the recovery lifecycle.
  action complete(pid: ProtocolId): bool = all {
    pid.in(harnessRegistered),
    protocol_recovery::signalComplete(pid),
    harnessRegistered' = harnessRegistered
  }

  /// Abort the recovery lifecycle.
  action abort(pid: ProtocolId): bool = all {
    pid.in(harnessRegistered),
    protocol_recovery::signalAbort(pid),
    harnessRegistered' = harnessRegistered
  }

  // ============ INVARIANTS ============

  val InvariantHarnessTracksRegistrations =
    harnessRegistered.forall(pid => pid != "")
}
