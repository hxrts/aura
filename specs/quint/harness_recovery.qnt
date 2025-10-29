module harness_recovery {
  import protocol_core as core from "protocol_core"
  import protocol_recovery as recovery from "protocol_recovery"
  import protocol_signals as signals from "protocol_signals"

  /// Register a recovery lifecycle across core + spec.
  action register(
    pid: core.ProtocolId,
    descriptor: core.ProtocolDescriptor,
    members: Set[core.DeviceId],
    guardians: Set[core.DeviceId],
    threshold: int
  ): bool =
    all {
      core.registerProtocol(pid, descriptor, members),
      recovery.configureRecovery(pid, guardians, threshold)
    }

  /// Apply LocalSignal("complete") when recovery evidence is satisfied.
  action complete(pid: core.ProtocolId): bool =
    all {
      core.recordInput(pid, core.ProtocolInput::LocalSignal({signal: signals.COMPLETE})),
      recovery.signalComplete(pid),
      core.processStep(
        pid,
        core.ProtocolState::Completed,
        recovery.lifecycleEffects.get(pid),
        None
      )
    }

  /// Abort the recovery lifecycle via LocalSignal("abort").
  action abort(pid: core.ProtocolId, reason: str): bool =
    all {
      core.recordInput(pid, core.ProtocolInput::LocalSignal({signal: signals.ABORT})),
      recovery.signalAbort(pid),
      core.processStep(
        pid,
        core.ProtocolState::Failed,
        recovery.lifecycleEffects.get(pid),
        Some(core.OutputState::Failure({reason: reason}))
      )
    }
}
