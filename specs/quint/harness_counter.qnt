module harness_counter {
  // Local type definitions (imports don't work for types in Quint)
  type ProtocolId = str
  type AuthorityId = str
  type CounterId = str

  // Import modules for actions (not types)
  import protocol_counter from "./protocol_counter"

  // Constants for signals
  pure val SIGNAL_COMPLETE = "complete"
  pure val SIGNAL_ABORT = "abort"

  // Harness state - tracks which protocols have been registered via harness
  var harnessRegistered: Set[ProtocolId]

  action init: bool = all {
    harnessRegistered' = Set()
  }

  /// Register a counter lifecycle and configure underlying metadata.
  action register(
    pid: ProtocolId,
    members: Set[AuthorityId],
    counterIds: Set[CounterId],
    owner: AuthorityId,
    relationship: int,
    requester: AuthorityId,
    base: int,
    ttlValue: int,
    epochValue: int
  ): bool = all {
    not(pid.in(harnessRegistered)),
    protocol_counter::configureCounters(pid, members, counterIds, owner, relationship, requester, base, ttlValue, epochValue),
    harnessRegistered' = harnessRegistered.union(Set(pid))
  }

  /// Record the reservation outcome prior to completion.
  action recordReservation(
    pid: ProtocolId,
    values: Set[int]
  ): bool = all {
    pid.in(harnessRegistered),
    protocol_counter::recordReservation(pid, values),
    harnessRegistered' = harnessRegistered
  }

  /// Complete the counter lifecycle.
  action complete(pid: ProtocolId): bool = all {
    pid.in(harnessRegistered),
    protocol_counter::finalize(pid),
    harnessRegistered' = harnessRegistered
  }

  /// Abort the counter lifecycle.
  action abort(pid: ProtocolId): bool = all {
    pid.in(harnessRegistered),
    protocol_counter::signalAbort(pid),
    harnessRegistered' = harnessRegistered
  }

  // ============ INVARIANTS ============

  val InvariantHarnessTracksRegistrations =
    harnessRegistered.forall(pid => pid != "")
}
