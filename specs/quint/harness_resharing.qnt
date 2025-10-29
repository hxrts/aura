module harness_resharing {
  import protocol_core as core from "protocol_core"
  import protocol_resharing as resharing from "protocol_resharing"
  import protocol_signals as signals from "protocol_signals"

  /// Register a resharing lifecycle across core + spec.
  action register(
    pid: core.ProtocolId,
    descriptor: core.ProtocolDescriptor,
    members: Set[core.DeviceId],
    oldParticipants: Set[core.DeviceId],
    newParticipants: Set[core.DeviceId],
    newThreshold: int
  ): bool =
    all {
      core.registerProtocol(pid, descriptor, members),
      resharing.configureResharing(pid, oldParticipants, newParticipants, newThreshold)
    }

  /// Apply a LocalSignal("complete") once resharing shares are in place.
  action complete(pid: core.ProtocolId): bool =
    all {
      core.recordInput(pid, core.ProtocolInput::LocalSignal({signal: signals.COMPLETE})),
      resharing.signalComplete(pid),
      core.processStep(
        pid,
        core.ProtocolState::Completed,
        resharing.lifecycleEffects.get(pid),
        None
      )
    }

  /// Abort the resharing lifecycle via LocalSignal("abort").
  action abort(pid: core.ProtocolId, reason: str): bool =
    all {
      core.recordInput(pid, core.ProtocolInput::LocalSignal({signal: signals.ABORT})),
      resharing.signalAbort(pid),
      core.processStep(
        pid,
        core.ProtocolState::Failed,
        resharing.lifecycleEffects.get(pid),
        Some(core.OutputState::Failure({reason: reason}))
      )
    }
}
