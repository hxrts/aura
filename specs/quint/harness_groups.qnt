module harness_groups {
  import protocol_core as core from "protocol_core"
  import protocol_groups as groups from "protocol_groups"
  import protocol_signals as signals from "protocol_signals"

  pure def messageFor(kind: groups.OperationKind): str =
    match kind {
      | groups.Create => signals.GROUP_CREATE
      | groups.Add => signals.GROUP_ADD
      | groups.Remove => signals.GROUP_REMOVE
      | groups.RotateEpoch => signals.GROUP_ROTATE_EPOCH
      | groups.Archive => signals.GROUP_ARCHIVE
    }

  /// Register a group lifecycle with an initial roster.
  action register(
    pid: core.ProtocolId,
    descriptor: core.ProtocolDescriptor,
    members: Set[core.DeviceId],
    groupId: groups.GroupId,
    initialMembers: Set[core.DeviceId]
  ): bool =
    all {
      core.registerProtocol(pid, descriptor, members),
      groups.configureGroup(pid, groupId, initialMembers)
    }

  /// Signal successful completion with a trace message.
  action complete(
    pid: core.ProtocolId
  ): bool =
    val op = groups.lastOperation.get(pid) {
      val message = messageFor(op) {
        all {
          core.recordInput(pid, core.ProtocolInput::LocalSignal({signal: signals.COMPLETE})),
          groups.signalSuccess(pid, message),
      core.processStep(
        pid,
        core.ProtocolState::Completed,
        groups.lifecycleEffects.get(pid),
        None
      )
      }
    }}

  /// Abort the lifecycle with a trace message.
  action abort(
    pid: core.ProtocolId,
    reason: str
  ): bool =
    all {
      core.recordInput(pid, core.ProtocolInput::LocalSignal({signal: signals.ABORT})),
      groups.signalFailure(pid, signals.ABORT),
      core.processStep(
        pid,
        core.ProtocolState::Failed,
        groups.lifecycleEffects.get(pid),
        Some(core.OutputState::Failure({reason: reason}))
      )
    }
}
