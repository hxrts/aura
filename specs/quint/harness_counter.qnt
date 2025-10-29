module harness_counter {
  import protocol_core as core from "protocol_core"
  import protocol_counter as counter from "protocol_counter"
  import protocol_signals as signals from "protocol_signals"

  /// Register a counter lifecycle and configure underlying metadata.
  action register(
    pid: core.ProtocolId,
    descriptor: core.ProtocolDescriptor,
    members: Set[core.DeviceId],
    counterIds: Set[counter.CounterId],
    owner: core.DeviceId,
    relationship: int,
    requester: core.DeviceId,
    base: int,
    ttlValue: int,
    epochValue: int
  ): bool =
    all {
      core.registerProtocol(pid, descriptor, members),
      counter.configureCounters(pid, members, counterIds, owner, relationship, requester, base, ttlValue, epochValue)
    }

  /// Record the reservation outcome prior to completion.
  action recordReservation(
    pid: core.ProtocolId,
    values: Set[int]
  ): bool =
    counter.recordReservation(pid, values)

  /// Complete the counter lifecycle (LocalSignal "complete").
  action complete(pid: core.ProtocolId): bool =
    all {
      core.recordInput(pid, core.ProtocolInput::LocalSignal({signal: signals.COMPLETE})),
      counter.finalize(pid),
      core.processStep(
        pid,
        core.ProtocolState::Completed,
        counter.lifecycleEffects.get(pid),
        None
      )
    }

  /// Abort the counter lifecycle (LocalSignal "abort").
  action abort(pid: core.ProtocolId, reason: str): bool =
    all {
      core.recordInput(pid, core.ProtocolInput::LocalSignal({signal: signals.ABORT})),
      counter.signalAbort(pid),
      core.processStep(
        pid,
        core.ProtocolState::Failed,
        counter.lifecycleEffects.get(pid),
        Some(core.OutputState::Failure({reason: reason}))
      )
    }
}
