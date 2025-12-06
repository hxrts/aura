module protocol_dkg {
  // Base types (defined locally - type imports don't work in Quint)
  type Option[a] = Some(a) | None
  type ProtocolId = str
  type AuthorityId = str  // renamed from DeviceId for authority model

  type DkgPhase =
    | Unconfigured
    | AwaitingCommitments
    | AwaitingVerifications
    | AggregatingShares
    | DkgCompleted  // unique name to avoid collision
    | DkgFailed

  type CommitmentState = ProtocolId -> AuthorityId -> Option[int]
  type VerificationState = ProtocolId -> AuthorityId -> Option[bool]
  type ShareState = ProtocolId -> AuthorityId -> Option[int]

  // Unique lifecycle variants for DKG
  type DkgLifecycleStatus = DkgPending | DkgLifecycleCompleted | DkgAborted

  type DkgOutcome = {
    derivedKey: int,
    publicKey: int,
    transcript: int,
    participants: Set[AuthorityId],
    threshold: int
  }

  // Simplified effect type (no imports)
  type DkgEffect = { effectType: str, payload: str }

  // ============ STATE ============

  var participants: ProtocolId -> Set[AuthorityId]
  var threshold: ProtocolId -> int
  var phase: ProtocolId -> DkgPhase
  var dealerCommitments: CommitmentState
  var verifierResponses: VerificationState
  var participantShares: ShareState
  var publicKeyPackage: ProtocolId -> Option[int]
  var transcriptDigest: ProtocolId -> Option[int]
  var lifecycleStatus: ProtocolId -> DkgLifecycleStatus
  var lifecycleEffects: ProtocolId -> Set[DkgEffect]
  var outcome: ProtocolId -> Option[DkgOutcome]

  // ============ HELPERS ============

  pure def getOrDefaultSet(m: ProtocolId -> Set[AuthorityId], key: ProtocolId): Set[AuthorityId] =
    if (key.in(m.keys())) m.get(key) else Set()

  pure def getOrDefaultInt(m: ProtocolId -> int, key: ProtocolId, default: int): int =
    if (key.in(m.keys())) m.get(key) else default

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

  def countFilledCommitments(pid: ProtocolId): int =
    getOrDefaultSet(participants, pid).fold(0, (acc, member) =>
      acc + (if (optionIsSome(getCommitmentOpt(dealerCommitments, pid, member))) 1 else 0)
    )

  def allVerified(pid: ProtocolId): bool =
    getOrDefaultSet(participants, pid).forall(member =>
      getVerificationOpt(verifierResponses, pid, member) == Some(true)
    )

  def sharesComplete(pid: ProtocolId): bool =
    getOrDefaultSet(participants, pid).forall(member =>
      optionIsSome(getCommitmentOpt(participantShares, pid, member))
    )

  def thresholdReachedCommitments(pid: ProtocolId): bool =
    countFilledCommitments(pid) >= getOrDefaultInt(threshold, pid, 0)

  // ============ INITIALISATION ============

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

  val InvariantThresholdBounds =
    threshold.keys().forall(pid =>
      threshold.get(pid) >= 1 and
      threshold.get(pid) <= getOrDefaultSet(participants, pid).size()
    )

  val InvariantCommitmentsRequireConfiguration =
    dealerCommitments.keys().subseteq(participants.keys())

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

  val InvariantSharesOnlyAfterVerification =
    participantShares.keys().forall(pid =>
      getOrDefaultSet(participants, pid).forall(member =>
        optionIsSome(getCommitmentOpt(participantShares, pid, member)) implies
          optionValueOrBool(getVerificationOpt(verifierResponses, pid, member), false)
      )
    )

  val InvariantLifecycleStatusDefined =
    lifecycleStatus.keys() == phase.keys()

  val InvariantLifecycleStatusMatchesPhase =
    lifecycleStatus.keys().forall(pid => {
      val status = lifecycleStatus.get(pid)
      val currentPhase = phase.get(pid)
      val isLifecycleCompleted = match status { | DkgLifecycleCompleted => true | _ => false }
      val isPhaseCompleted = match currentPhase { | DkgCompleted => true | _ => false }
      isLifecycleCompleted implies isPhaseCompleted
    })

  val InvariantLifecycleEffectsEmpty =
    lifecycleEffects.keys().forall(pid => lifecycleEffects.get(pid).size() == 0)

  val InvariantOutcomeWhenCompleted =
    lifecycleStatus.keys().forall(pid => {
      val status = lifecycleStatus.get(pid)
      val isCompleted = match status { | DkgLifecycleCompleted => true | _ => false }
      val outcomeOpt = if (pid.in(outcome.keys())) outcome.get(pid) else None
      val hasOutcome = match outcomeOpt { | Some(_) => true | None => false }
      isCompleted implies hasOutcome
    })

  // ============ STEP RELATION (for trace generation) ============

  action step = any {
    // Configure a new DKG session
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
