module harness_locking {
  import protocol_core as core from "protocol_core"
  import protocol_locking as locking from "protocol_locking"
  import protocol_signals as signals from "protocol_signals"

  /// Register a locking lifecycle.
  action register(
    pid: core.ProtocolId,
    descriptor: core.ProtocolDescriptor,
    members: Set[core.DeviceId],
    quorum: int
  ): bool =
    all {
      core.registerProtocol(pid, descriptor, members),
      locking.configureLock(pid, members, quorum)
    }

  /// Open the lottery phase.
  action openLottery(pid: core.ProtocolId): bool =
    locking.openLottery(pid)

  /// Submit a ballot for a contender.
  action submitBallot(
    pid: core.ProtocolId,
    member: core.DeviceId,
    ballot: int
  ): bool =
    locking.submitBallot(pid, member, ballot)

  /// Cast a release vote.
  action voteRelease(
    pid: core.ProtocolId,
    member: core.DeviceId
  ): bool =
    locking.voteRelease(pid, member)

  /// Finalize release before completion.
  action finalizeRelease(pid: core.ProtocolId): bool =
    locking.finalizeRelease(pid)

  /// Complete the locking lifecycle (LocalSignal "complete").
  action complete(pid: core.ProtocolId): bool =
    all {
      core.recordInput(pid, core.ProtocolInput::LocalSignal({signal: signals.COMPLETE})),
      locking.signalComplete(pid),
      core.processStep(
        pid,
        core.ProtocolState::Completed,
        locking.lifecycleEffects.get(pid),
        None
      )
    }

  /// Abort the locking lifecycle (LocalSignal "abort").
  action abort(pid: core.ProtocolId, reason: str): bool =
    all {
      core.recordInput(pid, core.ProtocolInput::LocalSignal({signal: signals.ABORT})),
      locking.signalAbort(pid),
      core.processStep(
        pid,
        core.ProtocolState::Failed,
        locking.lifecycleEffects.get(pid),
        Some(core.OutputState::Failure({reason: reason}))
      )
    }
}
