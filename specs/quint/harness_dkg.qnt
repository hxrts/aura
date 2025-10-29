module harness_dkg {
  import protocol_core as core from "protocol_core"
  import protocol_dkg as dkg from "protocol_dkg"
  import protocol_signals as signals from "protocol_signals"

  /// Register a DKD lifecycle in both the core scheduler and the lifecycle spec.
  action register(
    pid: core.ProtocolId,
    descriptor: core.ProtocolDescriptor,
    members: Set[core.DeviceId],
    threshold: int
  ): bool =
    all {
      core.registerProtocol(pid, descriptor, members),
      dkg.configureDkd(pid, members, threshold)
    }

  /// Drive a successful DKD completion (LocalSignal "complete").
  action complete(
    pid: core.ProtocolId,
    derivedKey: int,
    publicKey: int,
    transcript: int
  ): bool =
    all {
      core.recordInput(pid, core.ProtocolInput::LocalSignal({signal: signals.COMPLETE})),
      dkg.finalizeDkg(pid, derivedKey, publicKey, transcript),
      core.processStep(
        pid,
        core.ProtocolState::Completed,
        dkg.lifecycleEffects.get(pid),
        None
      )
    }

  /// Abort the DKD lifecycle (LocalSignal "abort").
  action abort(
    pid: core.ProtocolId,
    reason: str
  ): bool =
    all {
      core.recordInput(pid, core.ProtocolInput::LocalSignal({signal: signals.ABORT})),
      dkg.signalAbort(pid),
      core.processStep(
        pid,
        core.ProtocolState::Failed,
        dkg.lifecycleEffects.get(pid),
        Some(core.OutputState::Failure({reason: reason}))
      )
    }
}
