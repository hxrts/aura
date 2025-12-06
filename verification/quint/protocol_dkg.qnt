// Distributed Key Generation (DKG) Protocol Specification
//
// Models the Pedersen DKG ceremony for establishing threshold key shares.
// Participants exchange commitments, verify each other's contributions,
// then combine shares to derive a group public key.
//
// Phase progression: Unconfigured → AwaitingCommitments → AwaitingVerifications
//                    → AggregatingShares → DkgCompleted (or DkgFailed)
//
// Safety: No participant receives shares until all commitments are verified.
// Liveness: Progress requires threshold honest participants.

module protocol_dkg {
  // Base types (defined locally - type imports don't work in Quint)
  type Option[a] = Some(a) | None
  type ProtocolId = str
  type AuthorityId = str  // Opaque authority (was DeviceId)

  // DkgPhase tracks the ceremony's progress through distinct rounds.
  type DkgPhase =
    | Unconfigured           // Not yet started
    | AwaitingCommitments    // Round 1: collecting dealer commitments
    | AwaitingVerifications  // Round 2: verifying commitments
    | AggregatingShares      // Round 3: combining verified shares
    | DkgCompleted           // Successfully derived group key
    | DkgFailed              // Ceremony aborted (cheater detected or timeout)

  // Per-participant state maps: None = not yet received.
  type CommitmentState = ProtocolId -> AuthorityId -> Option[int]
  type VerificationState = ProtocolId -> AuthorityId -> Option[bool]
  type ShareState = ProtocolId -> AuthorityId -> Option[int]

  // Lifecycle status for external observers.
  type DkgLifecycleStatus = DkgPending | DkgLifecycleCompleted | DkgAborted

  // Final output of a successful DKG ceremony.
  type DkgOutcome = {
    derivedKey: int,             // Secret key material (never exposed)
    publicKey: int,              // Group public key for verification
    transcript: int,             // Audit log hash for replay
    participants: Set[AuthorityId],
    threshold: int               // k-of-n threshold
  }

  // Simplified effect type (no imports)
  type DkgEffect = { effectType: str, payload: str }

  // ============ STATE ============
  // Mutable state tracked by the model checker.

  var participants: ProtocolId -> Set[AuthorityId]      // Registered participants
  var threshold: ProtocolId -> int                       // k-of-n threshold
  var phase: ProtocolId -> DkgPhase                      // Current phase
  var dealerCommitments: CommitmentState                 // Round 1 commitments
  var verifierResponses: VerificationState               // Round 2 verifications
  var participantShares: ShareState                      // Round 3 shares
  var publicKeyPackage: ProtocolId -> Option[int]        // Derived group public key
  var transcriptDigest: ProtocolId -> Option[int]        // Audit trail
  var lifecycleStatus: ProtocolId -> DkgLifecycleStatus  // External status
  var lifecycleEffects: ProtocolId -> Set[DkgEffect]     // Pending effects
  var outcome: ProtocolId -> Option[DkgOutcome]          // Final result

  // ============ HELPERS ============
  // Pure functions for safe map/option access.

  pure def getOrDefaultSet(m: ProtocolId -> Set[AuthorityId], key: ProtocolId): Set[AuthorityId] =
    if (key.in(m.keys())) m.get(key) else Set()

  pure def getOrDefaultInt(m: ProtocolId -> int, key: ProtocolId, default: int): int =
    if (key.in(m.keys())) m.get(key) else default

  // Option predicates for cleaner guards.
  pure def optionIsSome(opt: Option[int]): bool =
    match opt {
      | Some(_) => true
      | None => false
    }

  pure def optionIsSomeBool(opt: Option[bool]): bool =
    match opt {
      | Some(_) => true
      | None => false
    }

  pure def optionValueOrBool(opt: Option[bool], fallback: bool): bool =
    match opt {
      | Some(value) => value
      | None => fallback
    }

  // Initialize participant map with None for all members.
  pure def ensureParticipantMap(
    mapping: CommitmentState,
    pid: ProtocolId,
    members: Set[AuthorityId]
  ): CommitmentState =
    if (pid.in(mapping.keys())) mapping
    else
      mapping.put(
        pid,
        members.fold(Map(), (acc, member) => acc.put(member, None))
      )

  // Update a single participant's entry in the commitment map.
  pure def updateMapping(
    mapping: CommitmentState,
    pid: ProtocolId,
    member: AuthorityId,
    value: Option[int],
    members: Set[AuthorityId]
  ): CommitmentState = {
    val ensured = ensureParticipantMap(mapping, pid, members)
    ensured.put(pid, ensured.get(pid).put(member, value))
  }

  pure def ensureVerificationMap(
    mapping: VerificationState,
    pid: ProtocolId,
    members: Set[AuthorityId]
  ): VerificationState =
    if (pid.in(mapping.keys())) mapping
    else
      mapping.put(
        pid,
        members.fold(Map(), (acc, member) => acc.put(member, None))
      )

  pure def updateVerification(
    mapping: VerificationState,
    pid: ProtocolId,
    member: AuthorityId,
    value: Option[bool],
    members: Set[AuthorityId]
  ): VerificationState = {
    val ensured = ensureVerificationMap(mapping, pid, members)
    ensured.put(pid, ensured.get(pid).put(member, value))
  }

  pure def getCommitmentOpt(mapping: CommitmentState, pid: ProtocolId, member: AuthorityId): Option[int] =
    if (pid.in(mapping.keys()))
      if (member.in(mapping.get(pid).keys())) mapping.get(pid).get(member) else None
    else None

  pure def getVerificationOpt(mapping: VerificationState, pid: ProtocolId, member: AuthorityId): Option[bool] =
    if (pid.in(mapping.keys()))
      if (member.in(mapping.get(pid).keys())) mapping.get(pid).get(member) else None
    else None

  // Count how many participants have submitted commitments.
  def countFilledCommitments(pid: ProtocolId): int =
    getOrDefaultSet(participants, pid).fold(0, (acc, member) =>
      acc + (if (optionIsSome(getCommitmentOpt(dealerCommitments, pid, member))) 1 else 0)
    )

  // True if all participants verified their commitments as valid.
  def allVerified(pid: ProtocolId): bool =
    getOrDefaultSet(participants, pid).forall(member =>
      getVerificationOpt(verifierResponses, pid, member) == Some(true)
    )

  // True if all participants have submitted their shares.
  def sharesComplete(pid: ProtocolId): bool =
    getOrDefaultSet(participants, pid).forall(member =>
      optionIsSome(getCommitmentOpt(participantShares, pid, member))
    )

  // True if we have at least threshold commitments (can proceed to verification).
  def thresholdReachedCommitments(pid: ProtocolId): bool =
    countFilledCommitments(pid) >= getOrDefaultInt(threshold, pid, 0)

  // ============ INITIALISATION ============
  // Start with empty state (no DKG ceremonies).

  action init: bool = all {
    participants' = Map(),
    threshold' = Map(),
    phase' = Map(),
    dealerCommitments' = Map(),
    verifierResponses' = Map(),
    participantShares' = Map(),
    publicKeyPackage' = Map(),
    transcriptDigest' = Map(),
    lifecycleStatus' = Map(),
    lifecycleEffects' = Map(),
    outcome' = Map()
  }

  // ============ ACTIONS ============
  // These model the DKG ceremony's message handlers.

  // Configure a new DKG ceremony with participant set and threshold.
  // Preconditions: 1 ≤ threshold ≤ participants.size(), no existing ceremony with this pid.
  action configureDkg(
    pid: ProtocolId,
    members: Set[AuthorityId],
    t: int
  ): bool = all {
    members.size() >= 1,
    t >= 1,
    t <= members.size(),
    not(pid.in(participants.keys())),
    participants' = participants.put(pid, members),
    threshold' = threshold.put(pid, t),
    phase' = phase.put(pid, AwaitingCommitments),
    dealerCommitments' = ensureParticipantMap(dealerCommitments, pid, members),
    verifierResponses' = ensureVerificationMap(verifierResponses, pid, members),
    participantShares' = ensureParticipantMap(participantShares, pid, members),
    publicKeyPackage' = publicKeyPackage.put(pid, None),
    transcriptDigest' = transcriptDigest.put(pid, None),
    lifecycleStatus' = lifecycleStatus.put(pid, DkgPending),
    lifecycleEffects' = lifecycleEffects.put(pid, Set()),
    outcome' = outcome.put(pid, None)
  }

  // Round 1: Submit a Pedersen commitment (polynomial evaluation point).
  // Auto-advances to AwaitingVerifications when threshold is reached.
  action submitCommitment(
    pid: ProtocolId,
    participant: AuthorityId,
    commitment: int
  ): bool = {
    val members = getOrDefaultSet(participants, pid)
    val currentPhase = if (pid.in(phase.keys())) phase.get(pid) else Unconfigured
    val hasCommitment = optionIsSome(getCommitmentOpt(dealerCommitments, pid, participant))
    val isValidPhase = match currentPhase {
      | AwaitingCommitments => true
      | AwaitingVerifications => true
      | _ => false
    }
    val updated = updateMapping(dealerCommitments, pid, participant, Some(commitment), members)
    val newCount = members.fold(0, (acc, m) =>
      acc + (if (optionIsSome(getCommitmentOpt(updated, pid, m))) 1 else 0)
    )
    val meetsThreshold = newCount >= getOrDefaultInt(threshold, pid, 0)
    val nextPhase = if (meetsThreshold) AwaitingVerifications else currentPhase
    all {
      members.contains(participant),
      isValidPhase,
      not(hasCommitment),
      dealerCommitments' = updated,
      phase' = phase.set(pid, nextPhase),
      participants' = participants,
      threshold' = threshold,
      verifierResponses' = verifierResponses,
      participantShares' = participantShares,
      publicKeyPackage' = publicKeyPackage,
      transcriptDigest' = transcriptDigest,
      lifecycleStatus' = lifecycleStatus,
      lifecycleEffects' = lifecycleEffects,
      outcome' = outcome
    }
  }

  // Round 2: Verify another participant's commitment.
  // If any participant rejects (valid=false), the ceremony fails.
  // Auto-advances to AggregatingShares when all verify successfully.
  action verifyCommitment(
    pid: ProtocolId,
    participant: AuthorityId,
    valid: bool
  ): bool = {
    val members = getOrDefaultSet(participants, pid)
    val currentPhase = if (pid.in(phase.keys())) phase.get(pid) else Unconfigured
    val isVerifyingPhase = match currentPhase { | AwaitingVerifications => true | _ => false }
    val hasCommitment = optionIsSome(getCommitmentOpt(dealerCommitments, pid, participant))
    val updated = updateVerification(verifierResponses, pid, participant, Some(valid), members)
    val allVerifiedNow = members.forall(m =>
      getVerificationOpt(updated, pid, m) == Some(true)
    )
    val nextPhase = if (not(valid)) DkgFailed else (if (allVerifiedNow) AggregatingShares else currentPhase)
    all {
      members.contains(participant),
      isVerifyingPhase,
      hasCommitment,
      verifierResponses' = updated,
      phase' = phase.set(pid, nextPhase),
      participants' = participants,
      threshold' = threshold,
      dealerCommitments' = dealerCommitments,
      participantShares' = participantShares,
      publicKeyPackage' = publicKeyPackage,
      transcriptDigest' = transcriptDigest,
      lifecycleStatus' = lifecycleStatus,
      lifecycleEffects' = lifecycleEffects,
      outcome' = outcome
    }
  }

  // Round 3: Submit secret share after verification succeeds.
  // Only allowed for participants who verified successfully.
  // Auto-advances to DkgCompleted when all shares are collected.
  action submitShare(
    pid: ProtocolId,
    participant: AuthorityId,
    shareDigest: int
  ): bool = {
    val members = getOrDefaultSet(participants, pid)
    val currentPhase = if (pid.in(phase.keys())) phase.get(pid) else Unconfigured
    val isAggregating = match currentPhase { | AggregatingShares => true | _ => false }
    val hasVerification = optionIsSomeBool(getVerificationOpt(verifierResponses, pid, participant))
    val verificationValid = optionValueOrBool(getVerificationOpt(verifierResponses, pid, participant), false)
    val hasShare = optionIsSome(getCommitmentOpt(participantShares, pid, participant))
    val updated = updateMapping(participantShares, pid, participant, Some(shareDigest), members)
    val allSharesNow = members.forall(m =>
      optionIsSome(getCommitmentOpt(updated, pid, m))
    )
    val nextPhase = if (allSharesNow) DkgCompleted else AggregatingShares
    all {
      isAggregating,
      members.contains(participant),
      hasVerification,
      verificationValid,
      not(hasShare),
      participantShares' = updated,
      phase' = phase.set(pid, nextPhase),
      participants' = participants,
      threshold' = threshold,
      dealerCommitments' = dealerCommitments,
      verifierResponses' = verifierResponses,
      publicKeyPackage' = publicKeyPackage,
      transcriptDigest' = transcriptDigest,
      lifecycleStatus' = lifecycleStatus,
      lifecycleEffects' = lifecycleEffects,
      outcome' = outcome
    }
  }

  // Finalize the ceremony: combine shares into group key and transcript.
  // Only callable after all shares are collected (DkgCompleted phase).
  action finalizeDkg(
    pid: ProtocolId,
    derivedKeyHash: int,
    publicKeyHash: int,
    transcriptHash: int
  ): bool = {
    val currentPhase = if (pid.in(phase.keys())) phase.get(pid) else Unconfigured
    val isCompleted = match currentPhase { | DkgCompleted => true | _ => false }
    val members = getOrDefaultSet(participants, pid)
    val t = getOrDefaultInt(threshold, pid, 0)
    all {
      isCompleted,
      sharesComplete(pid),
      publicKeyPackage' = publicKeyPackage.set(pid, Some(publicKeyHash)),
      transcriptDigest' = transcriptDigest.set(pid, Some(transcriptHash)),
      outcome' = outcome.set(pid, Some({
        derivedKey: derivedKeyHash,
        publicKey: publicKeyHash,
        transcript: transcriptHash,
        participants: members,
        threshold: t
      })),
      participants' = participants,
      threshold' = threshold,
      dealerCommitments' = dealerCommitments,
      verifierResponses' = verifierResponses,
      participantShares' = participantShares,
      phase' = phase,
      lifecycleStatus' = lifecycleStatus,
      lifecycleEffects' = lifecycleEffects
    }
  }

  // Abort the ceremony (timeout or detected cheater).
  action failDkg(pid: ProtocolId): bool = {
    val currentPhase = if (pid.in(phase.keys())) phase.get(pid) else Unconfigured
    val isNotCompleted = match currentPhase { | DkgCompleted => false | _ => true }
    val isConfigured = pid.in(phase.keys())
    all {
      isConfigured,
      isNotCompleted,
      phase' = phase.set(pid, DkgFailed),
      lifecycleStatus' = lifecycleStatus.set(pid, DkgAborted),
      participants' = participants,
      threshold' = threshold,
      dealerCommitments' = dealerCommitments,
      verifierResponses' = verifierResponses,
      participantShares' = participantShares,
      publicKeyPackage' = publicKeyPackage,
      transcriptDigest' = transcriptDigest,
      lifecycleEffects' = lifecycleEffects,
      outcome' = outcome
    }
  }

  // Signal successful completion to external observers.
  action signalComplete(pid: ProtocolId): bool = {
    val status = if (pid.in(lifecycleStatus.keys())) lifecycleStatus.get(pid) else DkgPending
    val isPending = match status { | DkgPending => true | _ => false }
    val currentPhase = if (pid.in(phase.keys())) phase.get(pid) else Unconfigured
    val isPhaseCompleted = match currentPhase { | DkgCompleted => true | _ => false }
    val isConfigured = pid.in(lifecycleStatus.keys())
    all {
      isConfigured,
      isPending,
      isPhaseCompleted,
      lifecycleStatus' = lifecycleStatus.set(pid, DkgLifecycleCompleted),
      lifecycleEffects' = lifecycleEffects.set(pid, Set()),
      participants' = participants,
      threshold' = threshold,
      phase' = phase,
      dealerCommitments' = dealerCommitments,
      verifierResponses' = verifierResponses,
      participantShares' = participantShares,
      publicKeyPackage' = publicKeyPackage,
      transcriptDigest' = transcriptDigest,
      outcome' = outcome
    }
  }

  // Signal abort to external observers (user-initiated cancellation).
  action signalAbort(pid: ProtocolId): bool = {
    val status = if (pid.in(lifecycleStatus.keys())) lifecycleStatus.get(pid) else DkgPending
    val isPending = match status { | DkgPending => true | _ => false }
    val isConfigured = pid.in(lifecycleStatus.keys())
    all {
      isConfigured,
      isPending,
      lifecycleStatus' = lifecycleStatus.set(pid, DkgAborted),
      lifecycleEffects' = lifecycleEffects,
      phase' = phase.set(pid, DkgFailed),
      participants' = participants,
      threshold' = threshold,
      dealerCommitments' = dealerCommitments,
      verifierResponses' = verifierResponses,
      participantShares' = participantShares,
      publicKeyPackage' = publicKeyPackage,
      transcriptDigest' = transcriptDigest,
      outcome' = outcome
    }
  }

  // ============ INVARIANTS ============
  // Safety properties that must hold in every reachable state.

  // Threshold is always valid: 1 ≤ t ≤ n.
  val InvariantThresholdBounds =
    threshold.keys().forall(pid =>
      threshold.get(pid) >= 1 and
      threshold.get(pid) <= getOrDefaultSet(participants, pid).size()
    )

  // Commitments can only exist for configured ceremonies.
  val InvariantCommitmentsRequireConfiguration =
    dealerCommitments.keys().subseteq(participants.keys())

  // Phase preconditions: each phase requires sufficient prior progress.
  val InvariantPhaseCommitmentCounts =
    phase.keys().forall(pid =>
      match phase.get(pid) {
        | Unconfigured => true
        | AwaitingCommitments => true
        | AwaitingVerifications =>
            thresholdReachedCommitments(pid)
        | AggregatingShares =>
            thresholdReachedCommitments(pid) and allVerified(pid)
        | DkgCompleted =>
            thresholdReachedCommitments(pid) and
            allVerified(pid) and
            sharesComplete(pid)
        | DkgFailed => true
      }
    )

  // Shares can only be submitted after successful verification.
  val InvariantSharesOnlyAfterVerification =
    participantShares.keys().forall(pid =>
      getOrDefaultSet(participants, pid).forall(member =>
        optionIsSome(getCommitmentOpt(participantShares, pid, member)) implies
          optionValueOrBool(getVerificationOpt(verifierResponses, pid, member), false)
      )
    )

  // Lifecycle status and phase maps have the same keys.
  val InvariantLifecycleStatusDefined =
    lifecycleStatus.keys() == phase.keys()

  // Lifecycle completion implies phase completion.
  val InvariantLifecycleStatusMatchesPhase =
    lifecycleStatus.keys().forall(pid => {
      val status = lifecycleStatus.get(pid)
      val currentPhase = phase.get(pid)
      val isLifecycleCompleted = match status { | DkgLifecycleCompleted => true | _ => false }
      val isPhaseCompleted = match currentPhase { | DkgCompleted => true | _ => false }
      isLifecycleCompleted implies isPhaseCompleted
    })

  // No pending effects (simplified model).
  val InvariantLifecycleEffectsEmpty =
    lifecycleEffects.keys().forall(pid => lifecycleEffects.get(pid).size() == 0)

  // Completed ceremonies have an outcome.
  val InvariantOutcomeWhenCompleted =
    lifecycleStatus.keys().forall(pid => {
      val status = lifecycleStatus.get(pid)
      val isCompleted = match status { | DkgLifecycleCompleted => true | _ => false }
      val outcomeOpt = if (pid.in(outcome.keys())) outcome.get(pid) else None
      val hasOutcome = match outcomeOpt { | Some(_) => true | None => false }
      isCompleted implies hasOutcome
    })

  // ============ STEP RELATION (for trace generation) ============
  // Nondeterministically choose any enabled action.

  action step = any {
    // Configure a new DKG ceremony
    nondet pid = oneOf(Set("dkg1", "dkg2", "dkg3"))
    nondet members = oneOf(Set(
      Set("alice", "bob"),
      Set("alice", "bob", "carol"),
      Set("bob", "carol", "dave")
    ))
    nondet t = oneOf(Set(2, 3))
    any {
      configureDkg(pid, members, t),
      // Submit commitment
      nondet participant = oneOf(Set("alice", "bob", "carol", "dave"))
      nondet commitment = oneOf(Set(100, 200, 300))
      submitCommitment(pid, participant, commitment),
      // Verify commitment
      nondet verifier = oneOf(Set("alice", "bob", "carol", "dave"))
      nondet valid = oneOf(Set(true, false))
      verifyCommitment(pid, verifier, valid),
      // Submit share
      nondet sharer = oneOf(Set("alice", "bob", "carol", "dave"))
      nondet shareDigest = oneOf(Set(1000, 2000, 3000))
      submitShare(pid, sharer, shareDigest),
      // Finalize DKG
      nondet derivedKey = oneOf(Set(9001, 9002))
      nondet publicKey = oneOf(Set(8001, 8002))
      nondet transcript = oneOf(Set(7001, 7002))
      finalizeDkg(pid, derivedKey, publicKey, transcript),
      // Fail or signal completion
      failDkg(pid),
      signalComplete(pid),
      signalAbort(pid),
    }
  }
}
