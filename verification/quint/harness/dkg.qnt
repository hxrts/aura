// DKG Test Harness
//
// Test driver for Distributed Key Generation protocol.
// Wraps protocol_dkg actions for integration testing and trace generation.
// Tracks registered protocols to ensure proper lifecycle sequencing.

module harness_dkg {
  // ============ TYPE DEFINITIONS ============
  type ProtocolId = str
  type AuthorityId = str

  // ============ IMPORTS ============
  import protocol_core from "../core"
  import protocol_dkg from "../keys/dkg"

  // ============ CONSTANTS ============
  pure val SIGNAL_COMPLETE = "complete"
  pure val SIGNAL_ABORT = "abort"

  // ============ HARNESS STATE ============
  // Tracks protocols registered via this harness.
  var harnessRegistered: Set[ProtocolId]

  action init: bool = all {
    harnessRegistered' = Set()
  }

  // ============ HARNESS ACTIONS ============

  // Configure and register a DKG protocol session.
  action register(
    pid: ProtocolId,
    members: Set[AuthorityId],
    threshold: int
  ): bool = all {
    not(pid.in(harnessRegistered)),
    protocol_dkg::configureDkg(pid, members, threshold),
    harnessRegistered' = harnessRegistered.union(Set(pid))
  }

  // Finalize DKG with derived key, public key, and transcript.
  action complete(
    pid: ProtocolId,
    derivedKey: int,
    publicKey: int,
    transcript: int
  ): bool = all {
    pid.in(harnessRegistered),
    protocol_dkg::finalizeDkg(pid, derivedKey, publicKey, transcript),
    harnessRegistered' = harnessRegistered
  }

  // Abort the DKG protocol (timeout or error).
  action abort(pid: ProtocolId): bool = all {
    pid.in(harnessRegistered),
    protocol_dkg::signalAbort(pid),
    harnessRegistered' = harnessRegistered
  }

  // ============ INVARIANTS ============
  // Harness-level safety properties.

  // All registered protocol IDs are non-empty.
  val InvariantHarnessTracksRegistrations =
    harnessRegistered.forall(pid => pid != "")
}
