module harness_groups {
  // Local type definitions (imports don't work for types in Quint)
  type ProtocolId = str
  type AuthorityId = str
  type GroupId = str

  // Import modules for actions (not types)
  import protocol_groups from "./protocol_groups"

  // Constants for signals
  pure val SIGNAL_COMPLETE = "complete"
  pure val SIGNAL_ABORT = "abort"

  // Harness state - tracks which protocols have been registered via harness
  var harnessRegistered: Set[ProtocolId]

  action init: bool = all {
    harnessRegistered' = Set()
  }

  /// Register a group lifecycle with an initial roster.
  action register(
    pid: ProtocolId,
    groupId: GroupId,
    initialMembers: Set[AuthorityId]
  ): bool = all {
    not(pid.in(harnessRegistered)),
    protocol_groups::configureGroup(pid, groupId, initialMembers),
    harnessRegistered' = harnessRegistered.union(Set(pid))
  }

  /// Propose adding members to the group.
  action proposeAdd(
    pid: ProtocolId,
    additions: Set[AuthorityId]
  ): bool = all {
    pid.in(harnessRegistered),
    protocol_groups::proposeAddMembers(pid, additions),
    harnessRegistered' = harnessRegistered
  }

  /// Confirm the pending add operation.
  action confirmAdd(pid: ProtocolId): bool = all {
    pid.in(harnessRegistered),
    protocol_groups::confirmAddMembers(pid),
    harnessRegistered' = harnessRegistered
  }

  /// Propose removing members from the group.
  action proposeRemove(
    pid: ProtocolId,
    removals: Set[AuthorityId]
  ): bool = all {
    pid.in(harnessRegistered),
    protocol_groups::proposeRemoveMembers(pid, removals),
    harnessRegistered' = harnessRegistered
  }

  /// Confirm the pending remove operation.
  action confirmRemove(pid: ProtocolId): bool = all {
    pid.in(harnessRegistered),
    protocol_groups::confirmRemoveMembers(pid),
    harnessRegistered' = harnessRegistered
  }

  /// Complete the group lifecycle.
  action complete(pid: ProtocolId): bool = all {
    pid.in(harnessRegistered),
    protocol_groups::signalSuccess(pid),
    harnessRegistered' = harnessRegistered
  }

  /// Abort the group lifecycle.
  action abort(pid: ProtocolId): bool = all {
    pid.in(harnessRegistered),
    protocol_groups::signalFailure(pid),
    harnessRegistered' = harnessRegistered
  }

  // ============ INVARIANTS ============

  val InvariantHarnessTracksRegistrations =
    harnessRegistered.forall(pid => pid != "")
}
