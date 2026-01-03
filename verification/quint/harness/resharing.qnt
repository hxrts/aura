// Resharing Test Harness
//
// Test driver for proactive secret sharing (key resharing) protocol.
// Wraps protocol_resharing actions for integration testing.
// Sequences: register → approve → moveToDistribution → recordCommitment → deliverShare.

module harness_resharing {
  // ============ TYPE DEFINITIONS ============
  type ProtocolId = str
  type AuthorityId = str

  // ============ IMPORTS ============
  import protocol_resharing from "../keys/resharing"

  // ============ CONSTANTS ============
  pure val SIGNAL_COMPLETE = "complete"
  pure val SIGNAL_ABORT = "abort"

  // ============ HARNESS STATE ============
  var harnessRegistered: Set[ProtocolId]

  action init: bool = all {
    harnessRegistered' = Set()
  }

  // ============ HARNESS ACTIONS ============

  // Configure resharing with old and new participant sets.
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

  // Old participant approves resharing proposal.
  action approve(
    pid: ProtocolId,
    member: AuthorityId
  ): bool = all {
    pid.in(harnessRegistered),
    protocol_resharing::approveResharing(pid, member),
    harnessRegistered' = harnessRegistered
  }

  // Transition to share distribution phase.
  action moveToDistribution(pid: ProtocolId): bool = all {
    pid.in(harnessRegistered),
    protocol_resharing::moveToDistribution(pid),
    harnessRegistered' = harnessRegistered
  }

  // New participant commits to received share.
  action recordCommitment(
    pid: ProtocolId,
    member: AuthorityId,
    commitmentHash: int
  ): bool = all {
    pid.in(harnessRegistered),
    protocol_resharing::recordCommitment(pid, member, commitmentHash),
    harnessRegistered' = harnessRegistered
  }

  // Deliver encrypted share to new participant.
  action deliverShare(
    pid: ProtocolId,
    member: AuthorityId,
    ciphertextHash: int
  ): bool = all {
    pid.in(harnessRegistered),
    protocol_resharing::deliverShare(pid, member, ciphertextHash),
    harnessRegistered' = harnessRegistered
  }

  // Signal successful completion.
  action complete(pid: ProtocolId): bool = all {
    pid.in(harnessRegistered),
    protocol_resharing::signalComplete(pid),
    harnessRegistered' = harnessRegistered
  }

  // Signal abort/failure.
  action abort(pid: ProtocolId): bool = all {
    pid.in(harnessRegistered),
    protocol_resharing::signalAbort(pid),
    harnessRegistered' = harnessRegistered
  }

  // ============ INVARIANTS ============

  // All registered protocol IDs are non-empty.
  val InvariantHarnessTracksRegistrations =
    harnessRegistered.forall(pid => pid != "")
}
