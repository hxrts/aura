module harness_resharing {
  // Local type definitions (imports don't work for types in Quint)
  type ProtocolId = str
  type AuthorityId = str

  // Import modules for actions (not types)
  import protocol_resharing from "./protocol_resharing"

  // Constants for signals
  pure val SIGNAL_COMPLETE = "complete"
  pure val SIGNAL_ABORT = "abort"

  // Harness state - tracks which protocols have been registered via harness
  var harnessRegistered: Set[ProtocolId]

  action init: bool = all {
    harnessRegistered' = Set()
  }

  /// Register a resharing lifecycle across core + spec.
  action register(
    pid: ProtocolId,
    oldParticipants: Set[AuthorityId],
    newParticipants: Set[AuthorityId],
    newThreshold: int
  ): bool = all {
    not(pid.in(harnessRegistered)),
    protocol_resharing::configureResharing(pid, oldParticipants, newParticipants, newThreshold),
    harnessRegistered' = harnessRegistered.union(Set(pid))
  }

  /// Submit an approval from an old participant.
  action approve(
    pid: ProtocolId,
    member: AuthorityId
  ): bool = all {
    pid.in(harnessRegistered),
    protocol_resharing::approveResharing(pid, member),
    harnessRegistered' = harnessRegistered
  }

  /// Move to distribution phase.
  action moveToDistribution(pid: ProtocolId): bool = all {
    pid.in(harnessRegistered),
    protocol_resharing::moveToDistribution(pid),
    harnessRegistered' = harnessRegistered
  }

  /// Record a commitment from a new participant.
  action recordCommitment(
    pid: ProtocolId,
    member: AuthorityId,
    commitmentHash: int
  ): bool = all {
    pid.in(harnessRegistered),
    protocol_resharing::recordCommitment(pid, member, commitmentHash),
    harnessRegistered' = harnessRegistered
  }

  /// Deliver a share to a new participant.
  action deliverShare(
    pid: ProtocolId,
    member: AuthorityId,
    ciphertextHash: int
  ): bool = all {
    pid.in(harnessRegistered),
    protocol_resharing::deliverShare(pid, member, ciphertextHash),
    harnessRegistered' = harnessRegistered
  }

  /// Complete the resharing lifecycle.
  action complete(pid: ProtocolId): bool = all {
    pid.in(harnessRegistered),
    protocol_resharing::signalComplete(pid),
    harnessRegistered' = harnessRegistered
  }

  /// Abort the resharing lifecycle.
  action abort(pid: ProtocolId): bool = all {
    pid.in(harnessRegistered),
    protocol_resharing::signalAbort(pid),
    harnessRegistered' = harnessRegistered
  }

  // ============ INVARIANTS ============

  val InvariantHarnessTracksRegistrations =
    harnessRegistered.forall(pid => pid != "")
}
