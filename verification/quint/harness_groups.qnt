// Groups Test Harness
//
// Test driver for group membership management protocol.
// Wraps protocol_groups actions for integration testing.
// Sequences propose → confirm flows for membership changes.

module harness_groups {
  // ============ TYPE DEFINITIONS ============
  type ProtocolId = str
  type AuthorityId = str
  type GroupId = str

  // ============ IMPORTS ============
  import protocol_groups from "./protocol_groups"

  // ============ CONSTANTS ============
  pure val SIGNAL_COMPLETE = "complete"
  pure val SIGNAL_ABORT = "abort"

  // ============ HARNESS STATE ============
  var harnessRegistered: Set[ProtocolId]

  action init: bool = all {
    harnessRegistered' = Set()
  }

  // ============ HARNESS ACTIONS ============

  // Configure and register a group with initial members.
  action register(
    pid: ProtocolId,
    groupId: GroupId,
    initialMembers: Set[AuthorityId]
  ): bool = all {
    not(pid.in(harnessRegistered)),
    protocol_groups::configureGroup(pid, groupId, initialMembers),
    harnessRegistered' = harnessRegistered.union(Set(pid))
  }

  // Propose adding members to the group.
  action proposeAdd(
    pid: ProtocolId,
    additions: Set[AuthorityId]
  ): bool = all {
    pid.in(harnessRegistered),
    protocol_groups::proposeAddMembers(pid, additions),
    harnessRegistered' = harnessRegistered
  }

  // Confirm pending add operation.
  action confirmAdd(pid: ProtocolId): bool = all {
    pid.in(harnessRegistered),
    protocol_groups::confirmAddMembers(pid),
    harnessRegistered' = harnessRegistered
  }

  // Propose removing members from the group.
  action proposeRemove(
    pid: ProtocolId,
    removals: Set[AuthorityId]
  ): bool = all {
    pid.in(harnessRegistered),
    protocol_groups::proposeRemoveMembers(pid, removals),
    harnessRegistered' = harnessRegistered
  }

  // Confirm pending remove operation.
  action confirmRemove(pid: ProtocolId): bool = all {
    pid.in(harnessRegistered),
    protocol_groups::confirmRemoveMembers(pid),
    harnessRegistered' = harnessRegistered
  }

  // Signal successful completion.
  action complete(pid: ProtocolId): bool = all {
    pid.in(harnessRegistered),
    protocol_groups::signalSuccess(pid),
    harnessRegistered' = harnessRegistered
  }

  // Signal abort/failure.
  action abort(pid: ProtocolId): bool = all {
    pid.in(harnessRegistered),
    protocol_groups::signalFailure(pid),
    harnessRegistered' = harnessRegistered
  }

  // ============ INVARIANTS ============

  // All registered protocol IDs are non-empty.
  val InvariantHarnessTracksRegistrations =
    harnessRegistered.forall(pid => pid != "")
}
