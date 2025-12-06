module harness_dkg {
  // Local type definitions (imports don't work for types in Quint)
  type ProtocolId = str
  type AuthorityId = str

  // Import modules for actions (not types)
  import protocol_core from "./protocol_core"
  import protocol_dkg from "./protocol_dkg"

  // Constants for signals
  pure val SIGNAL_COMPLETE = "complete"
  pure val SIGNAL_ABORT = "abort"

  // Harness state - tracks which protocols have been registered via harness
  var harnessRegistered: Set[ProtocolId]

  action init: bool = all {
    harnessRegistered' = Set()
  }

  /// Register a DKG lifecycle - drives both core and DKG module setup.
  /// Note: This is a test harness action, so we track state locally.
  action register(
    pid: ProtocolId,
    members: Set[AuthorityId],
    threshold: int
  ): bool = all {
    not(pid.in(harnessRegistered)),
    protocol_dkg::configureDkg(pid, members, threshold),
    harnessRegistered' = harnessRegistered.union(Set(pid))
  }

  /// Drive a successful DKG completion.
  action complete(
    pid: ProtocolId,
    derivedKey: int,
    publicKey: int,
    transcript: int
  ): bool = all {
    pid.in(harnessRegistered),
    protocol_dkg::finalizeDkg(pid, derivedKey, publicKey, transcript),
    harnessRegistered' = harnessRegistered
  }

  /// Abort the DKG lifecycle.
  action abort(pid: ProtocolId): bool = all {
    pid.in(harnessRegistered),
    protocol_dkg::signalAbort(pid),
    harnessRegistered' = harnessRegistered
  }

  // ============ INVARIANTS ============

  val InvariantHarnessTracksRegistrations =
    harnessRegistered.forall(pid => pid != "")
}
