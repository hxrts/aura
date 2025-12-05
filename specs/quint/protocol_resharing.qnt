module protocol_resharing {
  // Base types (defined locally - type imports don't work in Quint)
  type Option[a] = Some(a) | None
  type ProtocolId = str
  type AuthorityId = str  // renamed from DeviceId for authority model

  type ResharingPhase =
    | WaitingProposal
    | CollectingApprovals
    | DistributingShares
    | ResharingCompleted
    | ResharingFailed

  type ShareState = ProtocolId -> AuthorityId -> Option[int]

  // Unique lifecycle variants for resharing
  type ResharingLifecycleStatus = ResharingPending | ResharingLifecycleCompleted | ResharingAborted

  type ResharingOutcome = {
    newThreshold: int,
    newParticipants: Set[AuthorityId],
    oldParticipants: Set[AuthorityId],
    shareCount: int
  }

  // Simplified effect type (no imports)
  type ResharingEffect = { effectType: str, payload: str }

  // ============ STATE ============

  var oldParticipants: ProtocolId -> Set[AuthorityId]
  var newParticipants: ProtocolId -> Set[AuthorityId]
  var currentPhase: ProtocolId -> ResharingPhase
  var threshold: ProtocolId -> int
  var approvals: ProtocolId -> Set[AuthorityId]
  var shareCommitments: ShareState
  var encryptedShares: ShareState
  var lifecycleStatus: ProtocolId -> ResharingLifecycleStatus
  var lifecycleEffects: ProtocolId -> Set[ResharingEffect]
  var outcome: ProtocolId -> Option[ResharingOutcome]

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

  pure def ensureShareMap(
    mapping: ShareState,
    pid: ProtocolId,
    members: Set[AuthorityId]
  ): ShareState =
    if (pid.in(mapping.keys())) mapping
    else
      mapping.set(
        pid,
        members.fold(Map(), (acc, member) => acc.set(member, None))
      )

  pure def getShareOpt(mapping: ShareState, pid: ProtocolId, member: AuthorityId): Option[int] =
    if (pid.in(mapping.keys()))
      if (member.in(mapping.get(pid).keys())) mapping.get(pid).get(member) else None
    else None

  pure def setShare(
    mapping: ShareState,
    pid: ProtocolId,
    member: AuthorityId,
    value: Option[int],
    members: Set[AuthorityId]
  ): ShareState = {
    val ensured = ensureShareMap(mapping, pid, members)
    ensured.set(pid, ensured.get(pid).set(member, value))
  }

  def countApprovals(pid: ProtocolId): int =
    getOrDefaultSet(approvals, pid).size()

  def thresholdReached(pid: ProtocolId): bool =
    countApprovals(pid) >= getOrDefaultInt(threshold, pid, 0)

  def commitmentsReady(pid: ProtocolId): bool =
    getOrDefaultSet(newParticipants, pid).forall(member =>
      optionIsSome(getShareOpt(shareCommitments, pid, member))
    )

  def sharesDelivered(pid: ProtocolId): bool =
    getOrDefaultSet(newParticipants, pid).forall(member =>
      optionIsSome(getShareOpt(encryptedShares, pid, member))
    )

  def shareCountFor(mapping: ShareState, pid: ProtocolId): int =
    getOrDefaultSet(newParticipants, pid).fold(0, (acc, participant) =>
      if (optionIsSome(getShareOpt(mapping, pid, participant))) acc + 1 else acc
    )

  // ============ INITIALISATION ============

  action init: bool = all {
    oldParticipants' = Map(),
    newParticipants' = Map(),
    currentPhase' = Map(),
    threshold' = Map(),
    approvals' = Map(),
    shareCommitments' = Map(),
    encryptedShares' = Map(),
    lifecycleStatus' = Map(),
    lifecycleEffects' = Map(),
    outcome' = Map()
  }

  // ============ ACTIONS ============

  action configureResharing(
    pid: ProtocolId,
    oldSet: Set[AuthorityId],
    newSet: Set[AuthorityId],
    newThreshold: int
  ): bool = all {
    oldSet.size() >= 1,
    newSet.size() >= 1,
    newThreshold >= 1,
    newThreshold <= newSet.size(),
    not(pid.in(oldParticipants.keys())),
    oldParticipants' = oldParticipants.set(pid, oldSet),
    newParticipants' = newParticipants.set(pid, newSet),
    threshold' = threshold.set(pid, newThreshold),
    currentPhase' = currentPhase.set(pid, WaitingProposal),
    approvals' = approvals.set(pid, Set()),
    shareCommitments' = ensureShareMap(shareCommitments, pid, newSet),
    encryptedShares' = ensureShareMap(encryptedShares, pid, newSet),
    lifecycleStatus' = lifecycleStatus.set(pid, ResharingPending),
    lifecycleEffects' = lifecycleEffects.set(pid, Set()),
    outcome' = outcome.set(pid, None)
  }

  action approveResharing(pid: ProtocolId, member: AuthorityId): bool = {
    val phase = if (pid.in(currentPhase.keys())) currentPhase.get(pid) else WaitingProposal
    val isWaiting = match phase { | WaitingProposal => true | _ => false }
    val isCollecting = match phase { | CollectingApprovals => true | _ => false }
    val oldSet = getOrDefaultSet(oldParticipants, pid)
    val updatedApprovals = getOrDefaultSet(approvals, pid).union(Set(member))
    all {
      isWaiting or isCollecting,
      oldSet.contains(member),
      approvals' = approvals.set(pid, updatedApprovals),
      currentPhase' = if (thresholdReached(pid)) currentPhase.set(pid, CollectingApprovals) else currentPhase,
      oldParticipants' = oldParticipants,
      newParticipants' = newParticipants,
      threshold' = threshold,
      shareCommitments' = shareCommitments,
      encryptedShares' = encryptedShares,
      lifecycleStatus' = lifecycleStatus,
      lifecycleEffects' = lifecycleEffects,
      outcome' = outcome
    }
  }

  action moveToDistribution(pid: ProtocolId): bool = {
    val phase = if (pid.in(currentPhase.keys())) currentPhase.get(pid) else WaitingProposal
    val isCollecting = match phase { | CollectingApprovals => true | _ => false }
    all {
      isCollecting,
      thresholdReached(pid),
      currentPhase' = currentPhase.set(pid, DistributingShares),
      oldParticipants' = oldParticipants,
      newParticipants' = newParticipants,
      threshold' = threshold,
      approvals' = approvals,
      shareCommitments' = shareCommitments,
      encryptedShares' = encryptedShares,
      lifecycleStatus' = lifecycleStatus,
      lifecycleEffects' = lifecycleEffects,
      outcome' = outcome
    }
  }

  action recordCommitment(
    pid: ProtocolId,
    member: AuthorityId,
    commitmentHash: int
  ): bool = {
    val phase = if (pid.in(currentPhase.keys())) currentPhase.get(pid) else WaitingProposal
    val isDistributing = match phase { | DistributingShares => true | _ => false }
    val newSet = getOrDefaultSet(newParticipants, pid)
    val hasCommitment = optionIsSome(getShareOpt(shareCommitments, pid, member))
    val updated = setShare(shareCommitments, pid, member, Some(commitmentHash), newSet)
    all {
      isDistributing,
      newSet.contains(member),
      not(hasCommitment),
      shareCommitments' = updated,
      encryptedShares' = encryptedShares,
      currentPhase' = currentPhase,
      oldParticipants' = oldParticipants,
      newParticipants' = newParticipants,
      threshold' = threshold,
      approvals' = approvals,
      lifecycleStatus' = lifecycleStatus,
      lifecycleEffects' = lifecycleEffects,
      outcome' = outcome
    }
  }

  action deliverShare(
    pid: ProtocolId,
    member: AuthorityId,
    ciphertextHash: int
  ): bool = {
    val phase = if (pid.in(currentPhase.keys())) currentPhase.get(pid) else WaitingProposal
    val isDistributing = match phase { | DistributingShares => true | _ => false }
    val newSet = getOrDefaultSet(newParticipants, pid)
    val hasCommitment = optionIsSome(getShareOpt(shareCommitments, pid, member))
    val hasShare = optionIsSome(getShareOpt(encryptedShares, pid, member))
    val updated = setShare(encryptedShares, pid, member, Some(ciphertextHash), newSet)
    val completedDelivery = newSet.forall(participant =>
      optionIsSome(getShareOpt(updated, pid, participant))
    )
    val oldSet = getOrDefaultSet(oldParticipants, pid)
    val t = getOrDefaultInt(threshold, pid, 0)
    all {
      isDistributing,
      newSet.contains(member),
      hasCommitment,
      not(hasShare),
      encryptedShares' = updated,
      shareCommitments' = shareCommitments,
      currentPhase' = if (completedDelivery) currentPhase.set(pid, ResharingCompleted) else currentPhase,
      outcome' = if (completedDelivery) outcome.set(pid, Some({
        newThreshold: t,
        newParticipants: newSet,
        oldParticipants: oldSet,
        shareCount: shareCountFor(updated, pid)
      })) else outcome,
      oldParticipants' = oldParticipants,
      newParticipants' = newParticipants,
      threshold' = threshold,
      approvals' = approvals,
      lifecycleStatus' = lifecycleStatus,
      lifecycleEffects' = lifecycleEffects
    }
  }

  action failResharing(pid: ProtocolId): bool = {
    val phase = if (pid.in(currentPhase.keys())) currentPhase.get(pid) else WaitingProposal
    val isNotCompleted = match phase { | ResharingCompleted => false | _ => true }
    all {
      isNotCompleted,
      currentPhase' = currentPhase.set(pid, ResharingFailed),
      lifecycleStatus' = lifecycleStatus.set(pid, ResharingAborted),
      oldParticipants' = oldParticipants,
      newParticipants' = newParticipants,
      threshold' = threshold,
      approvals' = approvals,
      shareCommitments' = shareCommitments,
      encryptedShares' = encryptedShares,
      lifecycleEffects' = lifecycleEffects,
      outcome' = outcome
    }
  }

  action signalComplete(pid: ProtocolId): bool = {
    val status = if (pid.in(lifecycleStatus.keys())) lifecycleStatus.get(pid) else ResharingPending
    val isPending = match status { | ResharingPending => true | _ => false }
    val phase = if (pid.in(currentPhase.keys())) currentPhase.get(pid) else WaitingProposal
    val isPhaseCompleted = match phase { | ResharingCompleted => true | _ => false }
    val outcomeOpt = if (pid.in(outcome.keys())) outcome.get(pid) else None
    val hasOutcome = match outcomeOpt { | Some(_) => true | None => false }
    all {
      isPending,
      isPhaseCompleted,
      hasOutcome,
      lifecycleStatus' = lifecycleStatus.set(pid, ResharingLifecycleCompleted),
      lifecycleEffects' = lifecycleEffects.set(pid, Set()),
      oldParticipants' = oldParticipants,
      newParticipants' = newParticipants,
      currentPhase' = currentPhase,
      threshold' = threshold,
      approvals' = approvals,
      shareCommitments' = shareCommitments,
      encryptedShares' = encryptedShares,
      outcome' = outcome
    }
  }

  action signalAbort(pid: ProtocolId): bool = {
    val status = if (pid.in(lifecycleStatus.keys())) lifecycleStatus.get(pid) else ResharingPending
    val isPending = match status { | ResharingPending => true | _ => false }
    all {
      isPending,
      lifecycleStatus' = lifecycleStatus.set(pid, ResharingAborted),
      lifecycleEffects' = lifecycleEffects,
      currentPhase' = currentPhase.set(pid, ResharingFailed),
      oldParticipants' = oldParticipants,
      newParticipants' = newParticipants,
      threshold' = threshold,
      approvals' = approvals,
      shareCommitments' = shareCommitments,
      encryptedShares' = encryptedShares,
      outcome' = outcome
    }
  }

  // ============ INVARIANTS ============

  val InvariantThresholdWithinBounds =
    threshold.keys().forall(pid =>
      threshold.get(pid) >= 1 and
      threshold.get(pid) <= getOrDefaultSet(newParticipants, pid).size()
    )

  val InvariantApprovalsSubsetOld =
    approvals.keys().forall(pid =>
      getOrDefaultSet(approvals, pid).subseteq(getOrDefaultSet(oldParticipants, pid))
    )

  val InvariantPhaseProgression =
    currentPhase.keys().forall(pid =>
      match currentPhase.get(pid) {
        | WaitingProposal => countApprovals(pid) == 0
        | CollectingApprovals => countApprovals(pid) >= 0
        | DistributingShares => thresholdReached(pid)
        | ResharingCompleted => sharesDelivered(pid) and commitmentsReady(pid)
        | ResharingFailed => true
      }
    )

  val InvariantLifecycleStatusDefined =
    lifecycleStatus.keys() == currentPhase.keys()

  val InvariantLifecycleCompletionRequiresPhase =
    lifecycleStatus.keys().forall(pid => {
      val status = lifecycleStatus.get(pid)
      val phase = if (pid.in(currentPhase.keys())) currentPhase.get(pid) else WaitingProposal
      val isCompleted = match status { | ResharingLifecycleCompleted => true | _ => false }
      val isPhaseCompleted = match phase { | ResharingCompleted => true | _ => false }
      isCompleted implies isPhaseCompleted
    })

  val InvariantLifecycleEffectsEmpty =
    lifecycleEffects.keys().forall(pid => lifecycleEffects.get(pid).size() == 0)

  val InvariantOutcomeWhenCompleted =
    lifecycleStatus.keys().forall(pid => {
      val status = lifecycleStatus.get(pid)
      val isCompleted = match status { | ResharingLifecycleCompleted => true | _ => false }
      val outcomeOpt = if (pid.in(outcome.keys())) outcome.get(pid) else None
      val hasOutcome = match outcomeOpt { | Some(_) => true | None => false }
      isCompleted implies hasOutcome
    })
}
