module protocol_groups {
  import protocol_core as core from "protocol_core"
  import protocol_signals as signals from "protocol_signals"

  type ProtocolId = core.ProtocolId
  type DeviceId = core.DeviceId
  type GroupId = str

  type GroupPhase =
    | Uninitialized
    | Active
    | PendingChange
    | Archived
    | Failed

  type OperationKind =
    | Create
    | Add
    | Remove
    | RotateEpoch
    | Archive

  type LifecycleStatus = Pending | Completed | Aborted

  type GroupOutcome = {
    operation: OperationKind,
    epoch: int,
    members: Set[DeviceId]
  }

  type OperationRecord = {
    protocol: ProtocolId,
    group: GroupId,
    operation: OperationKind,
    members: Set[DeviceId],
    epoch: int
  }

  // ============ STATE ============

  var groupIds: ProtocolId -> GroupId
  var phase: ProtocolId -> GroupPhase
  var members: ProtocolId -> Set[DeviceId]
  var pendingAdds: ProtocolId -> Set[DeviceId]
  var pendingRemoves: ProtocolId -> Set[DeviceId]
  var epoch: ProtocolId -> int
  var operationLog: Set[OperationRecord]
  var lifecycleStatus: ProtocolId -> LifecycleStatus
  var lifecycleEffects: ProtocolId -> Set[core.ProtocolEffect]
  var lastOperation: ProtocolId -> OperationKind
  var outcome: ProtocolId -> Option[GroupOutcome]

  // ============ HELPERS ============

  pure def getOrDefault[K, V](m: K -> V, key: K, default: V): V =
    if key.in(m.keys()) then m.get(key) else default

  pure def disjoint[A](left: Set[A], right: Set[A]): bool =
    left.intersect(right).size() = 0

  pure def logOperation(
    log: Set[OperationRecord],
    pid: ProtocolId,
    gid: GroupId,
    kind: OperationKind,
    targets: Set[DeviceId],
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
    roster: Set[DeviceId]
  ): GroupOutcome = {
    operation: kind,
    epoch: groupEpoch,
    members: roster
  }

  pure def messageFor(kind: OperationKind): str =
    match kind {
      | Create => signals.GROUP_CREATE
      | Add => signals.GROUP_ADD
      | Remove => signals.GROUP_REMOVE
      | RotateEpoch => signals.GROUP_ROTATE_EPOCH
      | Archive => signals.GROUP_ARCHIVE
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
    initialMembers: Set[DeviceId]
  ): bool = all {
    not pid.in(groupIds.keys()),
    phase' = phase.set(pid, Active),
    groupIds' = groupIds.set(pid, gid),
    members' = members.set(pid, initialMembers),
    pendingAdds' = pendingAdds.set(pid, Set()),
    pendingRemoves' = pendingRemoves.set(pid, Set()),
    epoch' = epoch.set(pid, 0),
    operationLog' = logOperation(operationLog, pid, gid, Create, initialMembers, 0),
    lifecycleStatus' = lifecycleStatus.set(pid, Pending),
    lifecycleEffects' = lifecycleEffects.set(pid, Set()),
    lastOperation' = lastOperation.set(pid, Create),
    outcome' = outcome.set(pid, Some(makeOutcome(Create, 0, initialMembers)))
  }

  action proposeAddMembers(pid: ProtocolId, additions: Set[DeviceId]): bool =
    val currentPhase = phase.get(pid) {
    val currentMembers = members.get(pid) {
      currentPhase = Active and additions.size() >= 1 and disjoint(additions, currentMembers) {
        all {
          pendingAdds' = pendingAdds.set(pid, additions),
          pendingRemoves' = pendingRemoves,
          phase' = phase.set(pid, PendingChange),
          groupIds' = groupIds,
          members' = members,
          epoch' = epoch,
          operationLog' = operationLog
        }
      }
    }}

  action confirmAddMembers(pid: ProtocolId): bool =
    val currentPhase = phase.get(pid) {
    val gid = groupIds.get(pid) {
    val additions = getOrDefault(pendingAdds, pid, Set()) {
      currentPhase = PendingChange and additions.size() >= 1 {
        val updatedMembers = members.get(pid).union(additions) {
        val newEpoch = epoch.get(pid) + 1 {
        all {
          members' = members.set(pid, updatedMembers),
          pendingAdds' = pendingAdds.set(pid, Set()),
          pendingRemoves' = pendingRemoves.set(pid, Set()),
          phase' = phase.set(pid, Active),
          epoch' = epoch.set(pid, newEpoch),
          groupIds' = groupIds,
          operationLog' = logOperation(operationLog, pid, gid, Add, additions, newEpoch),
          lifecycleStatus' = lifecycleStatus,
          lifecycleEffects' = lifecycleEffects,
          lastOperation' = lastOperation.set(pid, Add),
          outcome' = outcome.set(pid, Some(makeOutcome(Add, newEpoch, updatedMembers)))
        }
      }}}
    }}}}

  action proposeRemoveMembers(pid: ProtocolId, removals: Set[DeviceId]): bool =
    val currentPhase = phase.get(pid) {
    val currentMembers = members.get(pid) {
      currentPhase = Active and removals.size() >= 1 and removals.subseteq(currentMembers) {
        all {
          pendingRemoves' = pendingRemoves.set(pid, removals),
          pendingAdds' = pendingAdds,
          phase' = phase.set(pid, PendingChange),
          groupIds' = groupIds,
          members' = members,
          epoch' = epoch,
          operationLog' = operationLog
        }
      }
    }}

  action confirmRemoveMembers(pid: ProtocolId): bool =
    val currentPhase = phase.get(pid) {
    val gid = groupIds.get(pid) {
    val removals = getOrDefault(pendingRemoves, pid, Set()) {
      currentPhase = PendingChange and removals.size() >= 1 {
        val updatedMembers = members.get(pid).diff(removals) {
        val newEpoch = epoch.get(pid) + 1 {
        all {
          members' = members.set(pid, updatedMembers),
          pendingAdds' = pendingAdds.set(pid, Set()),
          pendingRemoves' = pendingRemoves.set(pid, Set()),
          phase' = phase.set(pid, Active),
          epoch' = epoch.set(pid, newEpoch),
          groupIds' = groupIds,
          operationLog' = logOperation(operationLog, pid, gid, Remove, removals, newEpoch),
          lifecycleStatus' = lifecycleStatus,
          lifecycleEffects' = lifecycleEffects,
          lastOperation' = lastOperation.set(pid, Remove),
          outcome' = outcome.set(pid, Some(makeOutcome(Remove, newEpoch, updatedMembers)))
        }
      }}}
    }}}}

  action advanceEpoch(pid: ProtocolId): bool =
    val currentPhase = phase.get(pid) {
    val gid = groupIds.get(pid) {
    val roster = members.get(pid) {
      currentPhase = Active {
        val newEpoch = epoch.get(pid) + 1 {
        all {
          epoch' = epoch.set(pid, newEpoch),
          phase' = phase,
          groupIds' = groupIds,
          members' = members,
          pendingAdds' = pendingAdds,
          pendingRemoves' = pendingRemoves,
          operationLog' = logOperation(operationLog, pid, gid, RotateEpoch, Set(), newEpoch),
          lifecycleStatus' = lifecycleStatus,
          lifecycleEffects' = lifecycleEffects,
          lastOperation' = lastOperation.set(pid, RotateEpoch),
          outcome' = outcome.set(pid, Some(makeOutcome(RotateEpoch, newEpoch, roster)))
        }
      }}}}
    }}

  action archiveGroup(pid: ProtocolId): bool =
    val currentPhase = phase.get(pid) {
    val gid = groupIds.get(pid) {
    val roster = members.get(pid) {
      not currentPhase = Archived {
        all {
          phase' = phase.set(pid, Archived),
          pendingAdds' = pendingAdds.set(pid, Set()),
          pendingRemoves' = pendingRemoves.set(pid, Set()),
          epoch' = epoch,
          groupIds' = groupIds,
          members' = members,
          operationLog' = logOperation(operationLog, pid, gid, Archive, Set(), epoch.get(pid)),
          lifecycleStatus' = lifecycleStatus,
          lifecycleEffects' = lifecycleEffects,
          lastOperation' = lastOperation.set(pid, Archive),
          outcome' = outcome.set(
            pid,
            Some(makeOutcome(Archive, epoch.get(pid), roster))
          )
        }
      }}}
    }}

  action failGroup(pid: ProtocolId): bool =
    val currentPhase = phase.get(pid) {
      not currentPhase = Archived {
        all {
          phase' = phase.set(pid, Failed),
          groupIds' = groupIds,
          members' = members,
          pendingAdds' = pendingAdds,
          pendingRemoves' = pendingRemoves,
          epoch' = epoch,
          operationLog' = operationLog,
          lifecycleStatus' = lifecycleStatus.set(pid, Aborted),
          lifecycleEffects' = lifecycleEffects,
          lastOperation' = lastOperation,
          outcome' = outcome
        }
      }
    }

  action signalSuccess(pid: ProtocolId, message: str): bool =
    val status = lifecycleStatus.get(pid).default(Pending) {
    val outcomeOpt = outcome.get(pid) {
    val op = lastOperation.get(pid) {
      status = Pending and match outcomeOpt {
        | Some(_) => true
        | None => false
      } and message = messageFor(op) {
        lifecycleStatus' = lifecycleStatus.set(pid, Completed),
        lifecycleEffects' = lifecycleEffects.set(pid, Set(core.Trace({
          message: message
        }))),
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
    }}}

  action signalFailure(pid: ProtocolId, message: str): bool =
    lifecycleStatus.get(pid).default(Pending) = Pending {
      lifecycleStatus' = lifecycleStatus.set(pid, Aborted),
      lifecycleEffects' = lifecycleEffects.set(pid, Set(core.Trace({
        message: message
      }))),
      phase' = phase.set(pid, Failed),
      groupIds' = groupIds,
      members' = members,
      pendingAdds' = pendingAdds,
      pendingRemoves' = pendingRemoves,
      epoch' = epoch,
      operationLog' = operationLog,
      lastOperation' = lastOperation,
      outcome' = outcome
    }

  // ============ INVARIANTS ============

  val InvariantMembersOnlyWhenConfigured =
    members.keys().subseteq(groupIds.keys())

  val InvariantPhaseDefined =
    phase.keys() = groupIds.keys()

  val InvariantPendingDisjoint =
    pendingAdds.keys().forall(pid =>
      disjoint(getOrDefault(pendingAdds, pid, Set()), getOrDefault(pendingRemoves, pid, Set()))
    )

  val InvariantEpochNonNegative =
    epoch.keys().forall(pid => epoch.get(pid) >= 0)

  val InvariantArchivedNoPending =
    phase.keys().forall(pid =>
      phase.get(pid) = Archived implies
        getOrDefault(pendingAdds, pid, Set()).size() = 0 and
        getOrDefault(pendingRemoves, pid, Set()).size() = 0
    )

  val InvariantLogMatchesGroup =
    operationLog.forall(entry =>
      entry.protocol.in(groupIds.keys()) and entry.group = groupIds.get(entry.protocol)
    )

  val InvariantPendingRequiresPendingPhase =
    pendingAdds.keys().forall(pid =>
      getOrDefault(pendingAdds, pid, Set()).size() = 0 or phase.get(pid) = PendingChange
    ) and
    pendingRemoves.keys().forall(pid =>
      getOrDefault(pendingRemoves, pid, Set()).size() = 0 or phase.get(pid) = PendingChange
    )

  val InvariantActiveMembersNonEmpty =
    phase.keys().forall(pid =>
      phase.get(pid) = Active implies getOrDefault(members, pid, Set()).size() >= 1
    )

  val InvariantLifecycleStatusDefined =
    lifecycleStatus.keys() = groupIds.keys()

  val InvariantLifecycleEffectsTraceOnly =
    lifecycleEffects.keys().forall(pid =>
      lifecycleEffects.get(pid).forall(effect =>
        match effect {
          | core.Trace(_) => true
          | _ => false
        }
      )
    )

  val InvariantLifecycleCompletionHasTrace =
    lifecycleStatus.keys().forall(pid =>
      lifecycleStatus.get(pid) = Completed implies (
        lifecycleEffects.get(pid).size() = 1 and
        match outcome.get(pid) {
          | Some(_) => true
          | None => false
        } and
        pid.in(lastOperation.keys())
      )
    )

  val InvariantOutcomeDefinedForOperations =
    outcome.keys().forall(pid =>
      match outcome.get(pid) {
        | Some(result) => result.members.subseteq(getOrDefault(members, pid, Set()))
        | None => true
      }
    )
}
