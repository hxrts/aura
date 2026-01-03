// Group Membership Protocol Specification
//
// Models dynamic group membership management with epoch tracking.
// Supports adding/removing members through a propose-confirm flow.
// Each membership change increments the group epoch for versioning.
//
// Phase progression: Uninitialized → GroupActive ↔ GroupPendingChange
//                    → Archived (or GroupFailed)
//
// Safety: Members can only be added/removed through the proposal flow.
// Liveness: Pending changes must be confirmed or the proposal expires.
//
// See: docs/114_social_architecture.md

module protocol_groups {
  // ============ TYPE DEFINITIONS ============
  // Base types for group membership management.

  type Option[a] = Some(a) | None
  type ProtocolId = str       // Unique group session identifier
  type AuthorityId = str      // Participant identity
  type GroupId = str          // Group identifier

  // Group phase (state machine progression).
  type GroupPhase =
    | Uninitialized       // Not yet configured
    | GroupActive         // Ready for operations
    | GroupPendingChange  // Awaiting confirmation of add/remove
    | Archived            // Group deactivated (terminal)
    | GroupFailed         // Protocol error (terminal)

  // Operation types for audit logging.
  type OperationKind =
    | OpCreate        // Initial group creation
    | OpAdd           // Add members to group
    | OpRemove        // Remove members from group
    | OpRotateEpoch   // Rotate epoch without membership change
    | OpArchive       // Archive/deactivate the group

  // Lifecycle status for external coordination.
  type GroupLifecycleStatus = GroupPending | GroupLifecycleCompleted | GroupAborted

  // Result of a completed group operation.
  type GroupOutcome = {
    operation: OperationKind,    // What operation completed
    epoch: int,                  // Epoch after operation
    members: Set[AuthorityId]    // Final membership roster
  }

  // Audit log entry for membership operations.
  type OperationRecord = {
    protocol: ProtocolId,
    group: GroupId,
    operation: OperationKind,
    members: Set[AuthorityId],
    epoch: int
  }

  // Simplified effect type (no imports)
  type GroupEffect = { effectType: str, payload: str }

  // ============ STATE VARIABLES ============
  // Global mutable state tracked by the model checker.

  var groupIds: ProtocolId -> GroupId                   // Group ID per protocol
  var phase: ProtocolId -> GroupPhase                   // Current protocol phase
  var members: ProtocolId -> Set[AuthorityId]           // Current membership roster
  var pendingAdds: ProtocolId -> Set[AuthorityId]       // Proposed additions
  var pendingRemoves: ProtocolId -> Set[AuthorityId]    // Proposed removals
  var epoch: ProtocolId -> int                          // Membership version number
  var operationLog: Set[OperationRecord]                // Audit trail of operations
  var lifecycleStatus: ProtocolId -> GroupLifecycleStatus  // External lifecycle
  var lifecycleEffects: ProtocolId -> Set[GroupEffect]  // Pending effects
  var lastOperation: ProtocolId -> OperationKind        // Most recent operation
  var outcome: ProtocolId -> Option[GroupOutcome]       // Final result

  // ============ HELPERS ============
  // Pure functions for safe state access.

  pure def getOrDefaultSet(m: ProtocolId -> Set[AuthorityId], key: ProtocolId): Set[AuthorityId] =
    if (key.in(m.keys())) m.get(key) else Set()

  pure def getOrDefaultInt(m: ProtocolId -> int, key: ProtocolId): int =
    if (key.in(m.keys())) m.get(key) else 0

  // Check if two sets have no common elements.
  pure def disjoint(left: Set[AuthorityId], right: Set[AuthorityId]): bool =
    left.intersect(right).size() == 0

  // Append operation to audit log.
  pure def logOperation(
    log: Set[OperationRecord],
    pid: ProtocolId,
    gid: GroupId,
    kind: OperationKind,
    targets: Set[AuthorityId],
    groupEpoch: int
  ): Set[OperationRecord] =
    log.union(Set({
      protocol: pid,
      group: gid,
      operation: kind,
      members: targets,
      epoch: groupEpoch
    }))

  pure def makeOutcome(
    kind: OperationKind,
    groupEpoch: int,
    roster: Set[AuthorityId]
  ): GroupOutcome = {
    operation: kind,
    epoch: groupEpoch,
    members: roster
  }

  // ============ INITIALISATION ============
  // Start with empty state (no group sessions).

  action init: bool = all {
    groupIds' = Map(),
    phase' = Map(),
    members' = Map(),
    pendingAdds' = Map(),
    pendingRemoves' = Map(),
    epoch' = Map(),
    operationLog' = Set(),
    lifecycleStatus' = Map(),
    lifecycleEffects' = Map(),
    lastOperation' = Map(),
    outcome' = Map()
  }

  // ============ ACTIONS ============
  // State transitions modeling the group membership protocol.

  // Create a new group with initial members.
  action configureGroup(
    pid: ProtocolId,
    gid: GroupId,
    initialMembers: Set[AuthorityId]
  ): bool = all {
    not(pid.in(groupIds.keys())),
    phase' = phase.set(pid, GroupActive),
    groupIds' = groupIds.set(pid, gid),
    members' = members.set(pid, initialMembers),
    pendingAdds' = pendingAdds.set(pid, Set()),
    pendingRemoves' = pendingRemoves.set(pid, Set()),
    epoch' = epoch.set(pid, 0),
    operationLog' = logOperation(operationLog, pid, gid, OpCreate, initialMembers, 0),
    lifecycleStatus' = lifecycleStatus.set(pid, GroupPending),
    lifecycleEffects' = lifecycleEffects.set(pid, Set()),
    lastOperation' = lastOperation.set(pid, OpCreate),
    outcome' = outcome.set(pid, Some(makeOutcome(OpCreate, 0, initialMembers)))
  }

  // Propose adding new members to the group.
  // Requires: group is active, additions are not current members.
  action proposeAddMembers(pid: ProtocolId, additions: Set[AuthorityId]): bool = {
    val currentPhase = if (pid.in(phase.keys())) phase.get(pid) else Uninitialized
    val isActive = match currentPhase { | GroupActive => true | _ => false }
    val currentMembers = getOrDefaultSet(members, pid)
    all {
      isActive,
      additions.size() >= 1,
      disjoint(additions, currentMembers),
      pendingAdds' = pendingAdds.set(pid, additions),
      pendingRemoves' = pendingRemoves,
      phase' = phase.set(pid, GroupPendingChange),
      groupIds' = groupIds,
      members' = members,
      epoch' = epoch,
      operationLog' = operationLog,
      lifecycleStatus' = lifecycleStatus,
      lifecycleEffects' = lifecycleEffects,
      lastOperation' = lastOperation,
      outcome' = outcome
    }
  }

  // Confirm pending member additions, bump epoch.
  action confirmAddMembers(pid: ProtocolId): bool = {
    val currentPhase = if (pid.in(phase.keys())) phase.get(pid) else Uninitialized
    val isPending = match currentPhase { | GroupPendingChange => true | _ => false }
    val gid = if (pid.in(groupIds.keys())) groupIds.get(pid) else ""
    val additions = getOrDefaultSet(pendingAdds, pid)
    val currentMembers = getOrDefaultSet(members, pid)
    val updatedMembers = currentMembers.union(additions)
    val newEpoch = getOrDefaultInt(epoch, pid) + 1
    all {
      isPending,
      additions.size() >= 1,
      members' = members.set(pid, updatedMembers),
      pendingAdds' = pendingAdds.set(pid, Set()),
      pendingRemoves' = pendingRemoves.set(pid, Set()),
      phase' = phase.set(pid, GroupActive),
      epoch' = epoch.set(pid, newEpoch),
      groupIds' = groupIds,
      operationLog' = logOperation(operationLog, pid, gid, OpAdd, additions, newEpoch),
      lifecycleStatus' = lifecycleStatus,
      lifecycleEffects' = lifecycleEffects,
      lastOperation' = lastOperation.set(pid, OpAdd),
      outcome' = outcome.set(pid, Some(makeOutcome(OpAdd, newEpoch, updatedMembers)))
    }
  }

  // Propose removing members from the group.
  // Requires: group is active, removals are current members.
  action proposeRemoveMembers(pid: ProtocolId, removals: Set[AuthorityId]): bool = {
    val currentPhase = if (pid.in(phase.keys())) phase.get(pid) else Uninitialized
    val isActive = match currentPhase { | GroupActive => true | _ => false }
    val currentMembers = getOrDefaultSet(members, pid)
    all {
      isActive,
      removals.size() >= 1,
      removals.subseteq(currentMembers),
      pendingRemoves' = pendingRemoves.set(pid, removals),
      pendingAdds' = pendingAdds,
      phase' = phase.set(pid, GroupPendingChange),
      groupIds' = groupIds,
      members' = members,
      epoch' = epoch,
      operationLog' = operationLog,
      lifecycleStatus' = lifecycleStatus,
      lifecycleEffects' = lifecycleEffects,
      lastOperation' = lastOperation,
      outcome' = outcome
    }
  }

  // Confirm pending member removals, bump epoch.
  action confirmRemoveMembers(pid: ProtocolId): bool = {
    val currentPhase = if (pid.in(phase.keys())) phase.get(pid) else Uninitialized
    val isPending = match currentPhase { | GroupPendingChange => true | _ => false }
    val gid = if (pid.in(groupIds.keys())) groupIds.get(pid) else ""
    val removals = getOrDefaultSet(pendingRemoves, pid)
    val currentMembers = getOrDefaultSet(members, pid)
    val updatedMembers = currentMembers.exclude(removals)
    val newEpoch = getOrDefaultInt(epoch, pid) + 1
    all {
      isPending,
      removals.size() >= 1,
      members' = members.set(pid, updatedMembers),
      pendingAdds' = pendingAdds.set(pid, Set()),
      pendingRemoves' = pendingRemoves.set(pid, Set()),
      phase' = phase.set(pid, GroupActive),
      epoch' = epoch.set(pid, newEpoch),
      groupIds' = groupIds,
      operationLog' = logOperation(operationLog, pid, gid, OpRemove, removals, newEpoch),
      lifecycleStatus' = lifecycleStatus,
      lifecycleEffects' = lifecycleEffects,
      lastOperation' = lastOperation.set(pid, OpRemove),
      outcome' = outcome.set(pid, Some(makeOutcome(OpRemove, newEpoch, updatedMembers)))
    }
  }

  // Rotate epoch without membership change (key rotation trigger).
  action advanceEpoch(pid: ProtocolId): bool = {
    val currentPhase = if (pid.in(phase.keys())) phase.get(pid) else Uninitialized
    val isActive = match currentPhase { | GroupActive => true | _ => false }
    val gid = if (pid.in(groupIds.keys())) groupIds.get(pid) else ""
    val roster = getOrDefaultSet(members, pid)
    val newEpoch = getOrDefaultInt(epoch, pid) + 1
    all {
      isActive,
      epoch' = epoch.set(pid, newEpoch),
      phase' = phase,
      groupIds' = groupIds,
      members' = members,
      pendingAdds' = pendingAdds,
      pendingRemoves' = pendingRemoves,
      operationLog' = logOperation(operationLog, pid, gid, OpRotateEpoch, Set(), newEpoch),
      lifecycleStatus' = lifecycleStatus,
      lifecycleEffects' = lifecycleEffects,
      lastOperation' = lastOperation.set(pid, OpRotateEpoch),
      outcome' = outcome.set(pid, Some(makeOutcome(OpRotateEpoch, newEpoch, roster)))
    }
  }

  // Archive group (deactivate, terminal state).
  action archiveGroup(pid: ProtocolId): bool = {
    val currentPhase = if (pid.in(phase.keys())) phase.get(pid) else Uninitialized
    val isNotArchived = match currentPhase { | Archived => false | _ => true }
    val gid = if (pid.in(groupIds.keys())) groupIds.get(pid) else ""
    val roster = getOrDefaultSet(members, pid)
    val currentEpoch = getOrDefaultInt(epoch, pid)
    all {
      isNotArchived,
      phase' = phase.set(pid, Archived),
      pendingAdds' = pendingAdds.set(pid, Set()),
      pendingRemoves' = pendingRemoves.set(pid, Set()),
      epoch' = epoch,
      groupIds' = groupIds,
      members' = members,
      operationLog' = logOperation(operationLog, pid, gid, OpArchive, Set(), currentEpoch),
      lifecycleStatus' = lifecycleStatus,
      lifecycleEffects' = lifecycleEffects,
      lastOperation' = lastOperation.set(pid, OpArchive),
      outcome' = outcome.set(pid, Some(makeOutcome(OpArchive, currentEpoch, roster)))
    }
  }

  // Fail group protocol (error condition).
  action failGroup(pid: ProtocolId): bool = {
    val currentPhase = if (pid.in(phase.keys())) phase.get(pid) else Uninitialized
    val isNotArchived = match currentPhase { | Archived => false | _ => true }
    all {
      isNotArchived,
      phase' = phase.set(pid, GroupFailed),
      lifecycleStatus' = lifecycleStatus.set(pid, GroupAborted),
      groupIds' = groupIds,
      members' = members,
      pendingAdds' = pendingAdds,
      pendingRemoves' = pendingRemoves,
      epoch' = epoch,
      operationLog' = operationLog,
      lifecycleEffects' = lifecycleEffects,
      lastOperation' = lastOperation,
      outcome' = outcome
    }
  }

  // Signal external completion (lifecycle status update).
  action signalSuccess(pid: ProtocolId): bool = {
    val status = if (pid.in(lifecycleStatus.keys())) lifecycleStatus.get(pid) else GroupPending
    val isPending = match status { | GroupPending => true | _ => false }
    val outcomeOpt = if (pid.in(outcome.keys())) outcome.get(pid) else None
    val hasOutcome = match outcomeOpt { | Some(_) => true | None => false }
    all {
      isPending,
      hasOutcome,
      lifecycleStatus' = lifecycleStatus.set(pid, GroupLifecycleCompleted),
      lifecycleEffects' = lifecycleEffects.set(pid, Set()),
      phase' = phase,
      groupIds' = groupIds,
      members' = members,
      pendingAdds' = pendingAdds,
      pendingRemoves' = pendingRemoves,
      epoch' = epoch,
      operationLog' = operationLog,
      lastOperation' = lastOperation,
      outcome' = outcome
    }
  }

  // Signal external abort (lifecycle status update).
  action signalFailure(pid: ProtocolId): bool = {
    val status = if (pid.in(lifecycleStatus.keys())) lifecycleStatus.get(pid) else GroupPending
    val isPending = match status { | GroupPending => true | _ => false }
    all {
      isPending,
      lifecycleStatus' = lifecycleStatus.set(pid, GroupAborted),
      lifecycleEffects' = lifecycleEffects.set(pid, Set()),
      phase' = phase.set(pid, GroupFailed),
      groupIds' = groupIds,
      members' = members,
      pendingAdds' = pendingAdds,
      pendingRemoves' = pendingRemoves,
      epoch' = epoch,
      operationLog' = operationLog,
      lastOperation' = lastOperation,
      outcome' = outcome
    }
  }

  // ============ INVARIANTS ============
  // Safety properties that must hold in every reachable state.

  // Members only exist for configured groups.
  val InvariantMembersOnlyWhenConfigured =
    members.keys().subseteq(groupIds.keys())

  // Phase is defined for all configured groups.
  val InvariantPhaseDefined =
    phase.keys() == groupIds.keys()

  // Pending adds and removes are disjoint (can't add and remove same member).
  val InvariantPendingDisjoint =
    pendingAdds.keys().forall(pid =>
      disjoint(getOrDefaultSet(pendingAdds, pid), getOrDefaultSet(pendingRemoves, pid))
    )

  // Epoch is never negative.
  val InvariantEpochNonNegative =
    epoch.keys().forall(pid => epoch.get(pid) >= 0)

  // Archived groups have no pending changes.
  val InvariantArchivedNoPending =
    phase.keys().forall(pid => {
      val currentPhase = phase.get(pid)
      val isArchived = match currentPhase { | Archived => true | _ => false }
      isArchived implies (
        getOrDefaultSet(pendingAdds, pid).size() == 0 and
        getOrDefaultSet(pendingRemoves, pid).size() == 0
      )
    })

  // Audit log entries reference existing groups.
  val InvariantLogMatchesGroup =
    operationLog.forall(entry =>
      entry.protocol.in(groupIds.keys()) and entry.group == groupIds.get(entry.protocol)
    )

  // Pending changes only exist in GroupPendingChange phase.
  val InvariantPendingRequiresPendingPhase =
    pendingAdds.keys().forall(pid => {
      val currentPhase = if (pid.in(phase.keys())) phase.get(pid) else Uninitialized
      val isPendingPhase = match currentPhase { | GroupPendingChange => true | _ => false }
      getOrDefaultSet(pendingAdds, pid).size() == 0 or isPendingPhase
    }) and
    pendingRemoves.keys().forall(pid => {
      val currentPhase = if (pid.in(phase.keys())) phase.get(pid) else Uninitialized
      val isPendingPhase = match currentPhase { | GroupPendingChange => true | _ => false }
      getOrDefaultSet(pendingRemoves, pid).size() == 0 or isPendingPhase
    })

  // Active groups have at least one member.
  val InvariantActiveMembersNonEmpty =
    phase.keys().forall(pid => {
      val currentPhase = phase.get(pid)
      val isActive = match currentPhase { | GroupActive => true | _ => false }
      isActive implies getOrDefaultSet(members, pid).size() >= 1
    })

  // Lifecycle status defined for all configured groups.
  val InvariantLifecycleStatusDefined =
    lifecycleStatus.keys() == groupIds.keys()

  // No pending effects (simplified model).
  val InvariantLifecycleEffectsEmpty =
    lifecycleEffects.keys().forall(pid => lifecycleEffects.get(pid).size() == 0)

  // Completed lifecycle implies outcome is present.
  val InvariantLifecycleCompletionHasOutcome =
    lifecycleStatus.keys().forall(pid => {
      val status = lifecycleStatus.get(pid)
      val isCompleted = match status { | GroupLifecycleCompleted => true | _ => false }
      val outcomeOpt = if (pid.in(outcome.keys())) outcome.get(pid) else None
      val hasOutcome = match outcomeOpt { | Some(_) => true | None => false }
      isCompleted implies hasOutcome
    })
}
