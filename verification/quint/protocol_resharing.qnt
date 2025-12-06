// Key Resharing Protocol Specification
//
// Models proactive secret sharing: redistributing key shares to a new
// participant set without reconstructing the secret. Essential for:
// - Adding/removing participants from threshold group
// - Refreshing shares to mitigate compromise
// - Changing threshold value (e.g., 2-of-3 to 3-of-5)
//
// Phase progression: WaitingProposal → CollectingApprovals
//                    → DistributingShares → ResharingCompleted (or ResharingFailed)
//
// Safety: New shares reconstruct to same secret; old shares invalidated.
// Liveness: Requires threshold old participants to approve and contribute.
//
// See: docs/116_crypto.md

module protocol_resharing {
  // ============ TYPE DEFINITIONS ============
  // Base types for resharing protocol.

  type Option[a] = Some(a) | None
  type ProtocolId = str       // Unique resharing session identifier
  type AuthorityId = str      // Participant identity (old or new)

  // Resharing phase (state machine progression).
  type ResharingPhase =
    | WaitingProposal      // Awaiting resharing request
    | CollectingApprovals  // Gathering old participant approvals
    | DistributingShares   // Sending new shares to new participants
    | ResharingCompleted   // Successfully reshared (terminal success)
    | ResharingFailed      // Aborted (terminal failure)

  // Nested map type for share tracking.
  type ShareState = ProtocolId -> AuthorityId -> Option[int]

  // Lifecycle status for external coordination.
  type ResharingLifecycleStatus = ResharingPending | ResharingLifecycleCompleted | ResharingAborted

  // Outcome of successful resharing.
  type ResharingOutcome = {
    newThreshold: int,               // New k value
    newParticipants: Set[AuthorityId],  // Who holds new shares
    oldParticipants: Set[AuthorityId],  // Who held old shares
    shareCount: int                  // Shares successfully delivered
  }

  // Protocol effect (simplified for this module).
  type ResharingEffect = { effectType: str, payload: str }

  // ============ STATE VARIABLES ============
  // Global mutable state tracked by the model checker.

  var oldParticipants: ProtocolId -> Set[AuthorityId]   // Current share holders
  var newParticipants: ProtocolId -> Set[AuthorityId]   // Future share holders
  var currentPhase: ProtocolId -> ResharingPhase        // Current protocol phase
  var threshold: ProtocolId -> int                      // New threshold value
  var approvals: ProtocolId -> Set[AuthorityId]         // Old participants who approved
  var shareCommitments: ShareState                      // Pedersen commitments
  var encryptedShares: ShareState                       // Encrypted new shares
  var lifecycleStatus: ProtocolId -> ResharingLifecycleStatus  // External lifecycle
  var lifecycleEffects: ProtocolId -> Set[ResharingEffect]     // Pending effects
  var outcome: ProtocolId -> Option[ResharingOutcome]   // Final result

  // ============ HELPERS ============
  // Pure functions for safe state access.

  // Safe map access for Set values.
  pure def getOrDefaultSet(m: ProtocolId -> Set[AuthorityId], key: ProtocolId): Set[AuthorityId] =
    if (key.in(m.keys())) m.get(key) else Set()

  // Safe map access for int values.
  pure def getOrDefaultInt(m: ProtocolId -> int, key: ProtocolId, default: int): int =
    if (key.in(m.keys())) m.get(key) else default

  // Check if Option[int] contains a value.
  pure def optionIsSome(opt: Option[int]): bool =
    match opt {
      | Some(_) => true
      | None => false
    }

  // Initialize share map for all new participants with None.
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

  // Get share value for a participant (safe nested access).
  pure def getShareOpt(mapping: ShareState, pid: ProtocolId, member: AuthorityId): Option[int] =
    if (pid.in(mapping.keys()))
      if (member.in(mapping.get(pid).keys())) mapping.get(pid).get(member) else None
    else None

  // Set share value for a participant.
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

  // Count how many old participants have approved.
  def countApprovals(pid: ProtocolId): int =
    getOrDefaultSet(approvals, pid).size()

  // Check if threshold approvals reached.
  def thresholdReached(pid: ProtocolId): bool =
    countApprovals(pid) >= getOrDefaultInt(threshold, pid, 0)

  // Check if all new participants have commitments.
  def commitmentsReady(pid: ProtocolId): bool =
    getOrDefaultSet(newParticipants, pid).forall(member =>
      optionIsSome(getShareOpt(shareCommitments, pid, member))
    )

  // Check if all new participants have received shares.
  def sharesDelivered(pid: ProtocolId): bool =
    getOrDefaultSet(newParticipants, pid).forall(member =>
      optionIsSome(getShareOpt(encryptedShares, pid, member))
    )

  // Count shares in a share map for this protocol.
  def shareCountFor(mapping: ShareState, pid: ProtocolId): int =
    getOrDefaultSet(newParticipants, pid).fold(0, (acc, participant) =>
      if (optionIsSome(getShareOpt(mapping, pid, participant))) acc + 1 else acc
    )

  // ============ INITIALISATION ============
  // Start with empty state (no resharing sessions).

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
  // State transitions modeling the resharing protocol.

  // Configure a new resharing session with old/new participant sets.
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

  // Old participant approves the resharing proposal.
  // When threshold reached, transitions to CollectingApprovals.
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

  // Move to share distribution phase.
  // Requires: threshold approvals collected.
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

  // Record Pedersen commitment for a new participant.
  // Commitments must be recorded before shares can be delivered.
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

  // Deliver encrypted share to a new participant.
  // When all shares delivered, transitions to ResharingCompleted.
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

  // Fail resharing (timeout or protocol error).
  // Can be called at any phase except ResharingCompleted.
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

  // Signal external completion (lifecycle status update).
  // Requires: phase is ResharingCompleted, outcome is present.
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

  // Signal external abort (lifecycle status update).
  // Moves lifecycle to ResharingAborted, phase to ResharingFailed.
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
  // Safety properties that must hold in every reachable state.

  // Threshold is valid: 1 <= threshold <= newParticipants.size().
  val InvariantThresholdWithinBounds =
    threshold.keys().forall(pid =>
      threshold.get(pid) >= 1 and
      threshold.get(pid) <= getOrDefaultSet(newParticipants, pid).size()
    )

  // Approvals only from old participants.
  // New participants cannot approve (they don't hold old shares).
  val InvariantApprovalsSubsetOld =
    approvals.keys().forall(pid =>
      getOrDefaultSet(approvals, pid).subseteq(getOrDefaultSet(oldParticipants, pid))
    )

  // Phase-specific constraints are satisfied.
  // E.g., DistributingShares requires threshold approvals.
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

  // Lifecycle status defined for all configured protocols.
  val InvariantLifecycleStatusDefined =
    lifecycleStatus.keys() == currentPhase.keys()

  // LifecycleCompleted implies phase is ResharingCompleted.
  val InvariantLifecycleCompletionRequiresPhase =
    lifecycleStatus.keys().forall(pid => {
      val status = lifecycleStatus.get(pid)
      val phase = if (pid.in(currentPhase.keys())) currentPhase.get(pid) else WaitingProposal
      val isCompleted = match status { | ResharingLifecycleCompleted => true | _ => false }
      val isPhaseCompleted = match phase { | ResharingCompleted => true | _ => false }
      isCompleted implies isPhaseCompleted
    })

  // No pending effects (simplified model).
  val InvariantLifecycleEffectsEmpty =
    lifecycleEffects.keys().forall(pid => lifecycleEffects.get(pid).size() == 0)

  // LifecycleCompleted implies outcome is present.
  // Successful resharing always produces a ResharingOutcome.
  val InvariantOutcomeWhenCompleted =
    lifecycleStatus.keys().forall(pid => {
      val status = lifecycleStatus.get(pid)
      val isCompleted = match status { | ResharingLifecycleCompleted => true | _ => false }
      val outcomeOpt = if (pid.in(outcome.keys())) outcome.get(pid) else None
      val hasOutcome = match outcomeOpt { | Some(_) => true | None => false }
      isCompleted implies hasOutcome
    })
}
