module protocol_groups {
  // Base types (defined locally - type imports don't work in Quint)
  type Option[a] = Some(a) | None
  type ProtocolId = str
  type AuthorityId = str  // renamed from DeviceId for authority model
  type GroupId = str

  type GroupPhase =
    | Uninitialized
    | GroupActive
    | GroupPendingChange
    | Archived
    | GroupFailed

  type OperationKind =
    | OpCreate
    | OpAdd
    | OpRemove
    | OpRotateEpoch
    | OpArchive

  // Unique lifecycle variants for groups
  type GroupLifecycleStatus = GroupPending | GroupLifecycleCompleted | GroupAborted

  type GroupOutcome = {
    operation: OperationKind,
    epoch: int,
    members: Set[AuthorityId]
  }

  type OperationRecord = {
    protocol: ProtocolId,
    group: GroupId,
    operation: OperationKind,
    members: Set[AuthorityId],
    epoch: int
  }

  // Simplified effect type (no imports)
  type GroupEffect = { effectType: str, payload: str }

  // ============ STATE ============

  var groupIds: ProtocolId -> GroupId
  var phase: ProtocolId -> GroupPhase
  var members: ProtocolId -> Set[AuthorityId]
  var pendingAdds: ProtocolId -> Set[AuthorityId]
  var pendingRemoves: ProtocolId -> Set[AuthorityId]
  var epoch: ProtocolId -> int
  var operationLog: Set[OperationRecord]
  var lifecycleStatus: ProtocolId -> GroupLifecycleStatus
  var lifecycleEffects: ProtocolId -> Set[GroupEffect]
  var lastOperation: ProtocolId -> OperationKind
  var outcome: ProtocolId -> Option[GroupOutcome]

  // ============ HELPERS ============

  pure def getOrDefaultSet(m: ProtocolId -> Set[AuthorityId], key: ProtocolId): Set[AuthorityId] =
    if (key.in(m.keys())) m.get(key) else Set()

  pure def getOrDefaultInt(m: ProtocolId -> int, key: ProtocolId): int =
    if (key.in(m.keys())) m.get(key) else 0

  pure def disjoint(left: Set[AuthorityId], right: Set[AuthorityId]): bool =
    left.intersect(right).size() == 0

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

  val InvariantMembersOnlyWhenConfigured =
    members.keys().subseteq(groupIds.keys())

  val InvariantPhaseDefined =
    phase.keys() == groupIds.keys()

  val InvariantPendingDisjoint =
    pendingAdds.keys().forall(pid =>
      disjoint(getOrDefaultSet(pendingAdds, pid), getOrDefaultSet(pendingRemoves, pid))
    )

  val InvariantEpochNonNegative =
    epoch.keys().forall(pid => epoch.get(pid) >= 0)

  val InvariantArchivedNoPending =
    phase.keys().forall(pid => {
      val currentPhase = phase.get(pid)
      val isArchived = match currentPhase { | Archived => true | _ => false }
      isArchived implies (
        getOrDefaultSet(pendingAdds, pid).size() == 0 and
        getOrDefaultSet(pendingRemoves, pid).size() == 0
      )
    })

  val InvariantLogMatchesGroup =
    operationLog.forall(entry =>
      entry.protocol.in(groupIds.keys()) and entry.group == groupIds.get(entry.protocol)
    )

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

  val InvariantActiveMembersNonEmpty =
    phase.keys().forall(pid => {
      val currentPhase = phase.get(pid)
      val isActive = match currentPhase { | GroupActive => true | _ => false }
      isActive implies getOrDefaultSet(members, pid).size() >= 1
    })

  val InvariantLifecycleStatusDefined =
    lifecycleStatus.keys() == groupIds.keys()

  val InvariantLifecycleEffectsEmpty =
    lifecycleEffects.keys().forall(pid => lifecycleEffects.get(pid).size() == 0)

  val InvariantLifecycleCompletionHasOutcome =
    lifecycleStatus.keys().forall(pid => {
      val status = lifecycleStatus.get(pid)
      val isCompleted = match status { | GroupLifecycleCompleted => true | _ => false }
      val outcomeOpt = if (pid.in(outcome.keys())) outcome.get(pid) else None
      val hasOutcome = match outcomeOpt { | Some(_) => true | None => false }
      isCompleted implies hasOutcome
    })
}
