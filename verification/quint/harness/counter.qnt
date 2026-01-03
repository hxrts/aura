// Counter Test Harness
//
// Test driver for counter-based value reservation protocol.
// Wraps protocol_counter actions for integration testing.
// Tracks registered protocols and sequences operations correctly.

module harness_counter {
  // ============ TYPE DEFINITIONS ============
  type ProtocolId = str
  type AuthorityId = str
  type CounterId = str

  // ============ IMPORTS ============
  import protocol_counter from "../journal/counter"

  // ============ CONSTANTS ============
  pure val SIGNAL_COMPLETE = "complete"
  pure val SIGNAL_ABORT = "abort"

  // ============ HARNESS STATE ============
  var harnessRegistered: Set[ProtocolId]

  action init: bool = all {
    harnessRegistered' = Set()
  }

  // ============ HARNESS ACTIONS ============

  // Configure and register a counter protocol session.
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

  // Record reserved values before finalizing.
  action recordReservation(
    pid: ProtocolId,
    values: Set[int]
  ): bool = all {
    pid.in(harnessRegistered),
    protocol_counter::recordReservation(pid, values),
    harnessRegistered' = harnessRegistered
  }

  // Finalize the counter protocol.
  action complete(pid: ProtocolId): bool = all {
    pid.in(harnessRegistered),
    protocol_counter::finalize(pid),
    harnessRegistered' = harnessRegistered
  }

  // Abort the counter protocol.
  action abort(pid: ProtocolId): bool = all {
    pid.in(harnessRegistered),
    protocol_counter::signalAbort(pid),
    harnessRegistered' = harnessRegistered
  }

  // ============ INVARIANTS ============

  // All registered protocol IDs are non-empty.
  val InvariantHarnessTracksRegistrations =
    harnessRegistered.forall(pid => pid != "")
}
