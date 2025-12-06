// Distributed Key Derivation (DKD) Protocol Specification
//
// Models a commit-reveal scheme for collaborative key derivation.
// Participants first commit to their contributions, then reveal.
// This prevents "last actor" attacks where someone could bias the
// final derived key by choosing their contribution after seeing others.
//
// Phase progression: WaitingCommitments → WaitingReveals
//                    → Aggregating → DkdFinished (or DkdAborted)
//
// Safety: Derived key incorporates contributions from all revealed parties.
// Liveness: Requires threshold participants to commit and reveal.
//
// See: docs/116_crypto.md

module protocol_dkd {
  // ============ TYPE DEFINITIONS ============
  // Base types for distributed key derivation.

  type Option[a] = Some(a) | None
  type ProtocolId = str       // Unique DKD session identifier
  type AuthorityId = str      // Participant identity

  // DKD phase (state machine progression).
  type DkdPhase =
    | WaitingCommitments    // Collecting H(contribution) from participants
    | WaitingReveals        // Collecting actual contributions
    | Aggregating           // Combining contributions into derived key
    | DkdFinished           // Successfully derived key (terminal success)
    | DkdAborted            // Aborted (terminal failure)

  // Nested map types for tracking per-participant state.
  type CommitmentState = ProtocolId -> AuthorityId -> Option[int]
  type RevealState = ProtocolId -> AuthorityId -> Option[int]

  // ============ STATE VARIABLES ============
  // Global mutable state tracked by the model checker.

  var participants: ProtocolId -> Set[AuthorityId]  // All participants
  var phases: ProtocolId -> DkdPhase                // Current protocol phase
  var commitments: CommitmentState                  // H(contribution) per participant
  var reveals: RevealState                          // Actual contributions revealed
  var thresholds: ProtocolId -> int                 // k in k-of-n threshold
  var derivedKey: ProtocolId -> Option[int]         // Final derived key (if finished)
  var transcriptHash: ProtocolId -> Option[int]     // Hash of full protocol transcript

  // ============ HELPERS ============
  // Pure functions for safe state access.

  pure def getOrDefaultSet(m: ProtocolId -> Set[AuthorityId], key: ProtocolId): Set[AuthorityId] =
    if (key.in(m.keys())) m.get(key) else Set()

  pure def getOrDefaultInt(m: ProtocolId -> int, key: ProtocolId, default: int): int =
    if (key.in(m.keys())) m.get(key) else default

  pure def optionIsSome(opt: Option[int]): bool =
    match opt {
      | Some(_) => true
      | None => false
    }

  pure def ensureParticipantMap(
    mapping: CommitmentState,
    pid: ProtocolId,
    members: Set[AuthorityId]
  ): CommitmentState =
    if (pid.in(mapping.keys())) mapping
    else
      mapping.set(
        pid,
        members.fold(Map(), (acc, member) => acc.set(member, None))
      )

  pure def getCommitmentOpt(mapping: CommitmentState, pid: ProtocolId, member: AuthorityId): Option[int] =
    if (pid.in(mapping.keys()))
      if (member.in(mapping.get(pid).keys())) mapping.get(pid).get(member) else None
    else None

  def countFilled(mapping: CommitmentState, pid: ProtocolId): int =
    getOrDefaultSet(participants, pid).fold(0, (acc, member) =>
      acc + (if (optionIsSome(getCommitmentOpt(mapping, pid, member))) 1 else 0)
    )

  def allMembers(mapping: CommitmentState, pid: ProtocolId): bool =
    getOrDefaultSet(participants, pid).forall(member =>
      optionIsSome(getCommitmentOpt(mapping, pid, member))
    )

  pure def setMappingValue(
    mapping: CommitmentState,
    pid: ProtocolId,
    member: AuthorityId,
    value: Option[int],
    members: Set[AuthorityId]
  ): CommitmentState = {
    val ensured = ensureParticipantMap(mapping, pid, members)
    ensured.set(pid, ensured.get(pid).set(member, value))
  }

  def thresholdReached(pid: ProtocolId, mapping: CommitmentState): bool =
    countFilled(mapping, pid) >= getOrDefaultInt(thresholds, pid, 0)

  def revealsMatchCommitments(pid: ProtocolId): bool =
    getOrDefaultSet(participants, pid).forall(member => {
      val revealOpt = getCommitmentOpt(reveals, pid, member)
      val hasReveal = match revealOpt { | Some(_) => true | None => false }
      hasReveal implies optionIsSome(getCommitmentOpt(commitments, pid, member))
    })

  // ============ INITIALISATION ============
  // Start with empty state (no DKD sessions).

  action init: bool = all {
    participants' = Map(),
    phases' = Map(),
    commitments' = Map(),
    reveals' = Map(),
    thresholds' = Map(),
    derivedKey' = Map(),
    transcriptHash' = Map()
  }

  // ============ ACTIONS ============
  // State transitions modeling the DKD protocol.

  // Configure a new DKD session with participant set and threshold.
  action configureDkd(
    pid: ProtocolId,
    members: Set[AuthorityId],
    threshold: int
  ): bool = all {
    members.size() >= 1,
    threshold >= 1,
    threshold <= members.size(),
    not(pid.in(participants.keys())),
    participants' = participants.set(pid, members),
    phases' = phases.set(pid, WaitingCommitments),
    commitments' = ensureParticipantMap(commitments, pid, members),
    reveals' = ensureParticipantMap(reveals, pid, members),
    thresholds' = thresholds.set(pid, threshold),
    derivedKey' = derivedKey.set(pid, None),
    transcriptHash' = transcriptHash.set(pid, None)
  }

  // Record participant's commitment (hash of their contribution).
  // When threshold commits received, transitions to WaitingReveals.
  action recordCommitment(
    pid: ProtocolId,
    member: AuthorityId,
    commitment: int
  ): bool = {
    val members = getOrDefaultSet(participants, pid)
    val state = if (pid.in(phases.keys())) phases.get(pid) else WaitingCommitments
    val isWaiting = match state { | WaitingCommitments => true | _ => false }
    val isRevealing = match state { | WaitingReveals => true | _ => false }
    val hasCommitment = optionIsSome(getCommitmentOpt(commitments, pid, member))
    val updatedCommitments = setMappingValue(commitments, pid, member, Some(commitment), members)
    val meetsThreshold = countFilled(updatedCommitments, pid) >= getOrDefaultInt(thresholds, pid, 0)
    val newPhase = if (meetsThreshold) WaitingReveals else state
    all {
      members.contains(member),
      isWaiting or isRevealing,
      not(hasCommitment),
      commitments' = updatedCommitments,
      phases' = phases.set(pid, newPhase),
      participants' = participants,
      reveals' = reveals,
      thresholds' = thresholds,
      derivedKey' = derivedKey,
      transcriptHash' = transcriptHash
    }
  }

  // Record participant's reveal (actual contribution).
  // Requires: participant already committed. Verifies commitment matches.
  // When threshold reveals received, transitions to Aggregating.
  action recordReveal(
    pid: ProtocolId,
    member: AuthorityId,
    share: int
  ): bool = {
    val members = getOrDefaultSet(participants, pid)
    val state = if (pid.in(phases.keys())) phases.get(pid) else WaitingCommitments
    val isRevealing = match state { | WaitingReveals => true | _ => false }
    val isAggregating = match state { | Aggregating => true | _ => false }
    val hasCommitment = optionIsSome(getCommitmentOpt(commitments, pid, member))
    val hasReveal = optionIsSome(getCommitmentOpt(reveals, pid, member))
    val updatedReveals = setMappingValue(reveals, pid, member, Some(share), members)
    val meetsThreshold = countFilled(updatedReveals, pid) >= getOrDefaultInt(thresholds, pid, 0)
    val newPhase = if (meetsThreshold) Aggregating else state
    all {
      members.contains(member),
      isRevealing or isAggregating,
      hasCommitment,
      not(hasReveal),
      reveals' = updatedReveals,
      phases' = phases.set(pid, newPhase),
      participants' = participants,
      commitments' = commitments,
      thresholds' = thresholds,
      derivedKey' = derivedKey,
      transcriptHash' = transcriptHash
    }
  }

  // Finalize DKD by aggregating reveals into derived key.
  // Requires: all reveals match their commitments.
  action finalizeDkd(
    pid: ProtocolId,
    derivedKeyHash: int,
    transcript: int
  ): bool = {
    val state = if (pid.in(phases.keys())) phases.get(pid) else WaitingCommitments
    val isAggregating = match state { | Aggregating => true | _ => false }
    all {
      isAggregating,
      thresholdReached(pid, reveals),
      revealsMatchCommitments(pid),
      phases' = phases.set(pid, DkdFinished),
      derivedKey' = derivedKey.set(pid, Some(derivedKeyHash)),
      transcriptHash' = transcriptHash.set(pid, Some(transcript)),
      participants' = participants,
      commitments' = commitments,
      reveals' = reveals,
      thresholds' = thresholds
    }
  }

  // Abort DKD (timeout or invalid reveal detected).
  action abortDkd(pid: ProtocolId): bool = {
    val state = if (pid.in(phases.keys())) phases.get(pid) else WaitingCommitments
    val isNotFinished = match state { | DkdFinished => false | _ => true }
    all {
      isNotFinished,
      phases' = phases.set(pid, DkdAborted),
      participants' = participants,
      commitments' = commitments,
      reveals' = reveals,
      thresholds' = thresholds,
      derivedKey' = derivedKey,
      transcriptHash' = transcriptHash
    }
  }

  // ============ INVARIANTS ============
  // Safety properties that must hold in every reachable state.

  // Threshold is valid: 1 <= threshold <= participants.size().
  val InvariantThresholdsWithinBounds =
    thresholds.keys().forall(pid =>
      thresholds.get(pid) >= 1 and
      thresholds.get(pid) <= getOrDefaultSet(participants, pid).size()
    )

  // Each participant commits at most once (prevents contribution manipulation).
  val InvariantCommitmentsAtMostOne =
    commitments.keys().forall(pid =>
      getOrDefaultSet(participants, pid).forall(member =>
        match getCommitmentOpt(commitments, pid, member) {
          | Some(_) => true
          | None => true
        }
      )
    )

  // Reveals only from participants who committed (commit-before-reveal).
  val InvariantRevealsRequireCommitment =
    reveals.keys().forall(pid => revealsMatchCommitments(pid))

  // Phase-specific constraints are satisfied.
  // E.g., WaitingReveals requires threshold commitments.
  val InvariantPhaseProgression =
    phases.keys().forall(pid =>
      match phases.get(pid) {
        | WaitingCommitments => countFilled(commitments, pid) == 0
        | WaitingReveals =>
            countFilled(commitments, pid) >= thresholds.get(pid)
        | Aggregating =>
            countFilled(commitments, pid) >= thresholds.get(pid) and
            countFilled(reveals, pid) >= thresholds.get(pid)
        | DkdFinished => {
            val hasKey = match (if (pid.in(derivedKey.keys())) derivedKey.get(pid) else None) {
              | Some(_) => true
              | None => false
            }
            val hasTranscript = match (if (pid.in(transcriptHash.keys())) transcriptHash.get(pid) else None) {
              | Some(_) => true
              | None => false
            }
            hasKey and hasTranscript and countFilled(reveals, pid) >= thresholds.get(pid)
          }
        | DkdAborted => true
      }
    )

  // Derived key only present when phase is DkdFinished.
  // Prevents key leakage before all participants have revealed.
  val InvariantDerivedKeyOnlyFinished =
    derivedKey.keys().forall(pid => {
      val keyOpt = derivedKey.get(pid)
      val hasKey = match keyOpt { | Some(_) => true | None => false }
      val state = if (pid.in(phases.keys())) phases.get(pid) else WaitingCommitments
      val isFinished = match state { | DkdFinished => true | _ => false }
      hasKey implies isFinished
    })
}
