module protocol_resharing {
  import protocol_core as core from "protocol_core"

  type Option[a] = Some(a) | None

  type ProtocolId = core.ProtocolId
  type DeviceId = core.DeviceId

  type ResharingPhase =
    | WaitingProposal
    | CollectingApprovals
    | DistributingShares
    | Completed
    | Failed

  type ShareState = ProtocolId -> DeviceId -> Option[int]
  type LifecycleStatus = Pending | Completed | Aborted
  type ResharingOutcome = {
    newThreshold: int,
    newParticipants: Set[DeviceId],
    oldParticipants: Set[DeviceId],
    shareCount: int
  }

  // ============ STATE ============

  var oldParticipants: ProtocolId -> Set[DeviceId]
  var newParticipants: ProtocolId -> Set[DeviceId]
  var currentPhase: ProtocolId -> ResharingPhase
  var threshold: ProtocolId -> int
  var approvals: ProtocolId -> Set[DeviceId]
  var shareCommitments: ShareState
  var encryptedShares: ShareState
  var lifecycleStatus: ProtocolId -> LifecycleStatus
  var lifecycleEffects: ProtocolId -> Set[core.ProtocolEffect]
  var outcome: ProtocolId -> Option[ResharingOutcome]

  // ============ HELPERS ============

  pure def getOrDefault[K, V](m: K -> V, key: K, default: V): V =
    if key.in(m.keys()) then m.get(key) else default

  pure def optionIsSome[a](opt: Option[a]): bool =
    match opt {
      | Some(_) => true
      | None => false
    }

  pure def ensureShareMap(
    mapping: ShareState,
    pid: ProtocolId,
    members: Set[DeviceId]
  ): ShareState =
    if pid.in(mapping.keys()) then mapping
    else
      mapping.set(
        pid,
        members.fold(Map(), (acc, member) => acc.set(member, None))
      )

  pure def setShare(
    mapping: ShareState,
    pid: ProtocolId,
    member: DeviceId,
    value: Option[int]
  ): ShareState =
    ensureShareMap(mapping, pid, getOrDefault(newParticipants, pid, Set())).set(
      pid,
      ensureShareMap(mapping, pid, getOrDefault(newParticipants, pid, Set()))
        .get(pid)
        .set(member, value)
    )

  pure def countApprovals(pid: ProtocolId): int =
    getOrDefault(approvals, pid, Set()).size()

  pure def thresholdReached(pid: ProtocolId): bool =
    countApprovals(pid) >= getOrDefault(threshold, pid, 0)

  pure def commitmentsReady(pid: ProtocolId): bool =
    getOrDefault(newParticipants, pid, Set()).forall(member =>
      optionIsSome(getOrDefault(shareCommitments.get(pid), member, None))
    )

  pure def sharesDelivered(pid: ProtocolId): bool =
    getOrDefault(newParticipants, pid, Set()).forall(member =>
      optionIsSome(getOrDefault(encryptedShares.get(pid), member, None))
    )

  pure def shareCountFor(map: ShareState, pid: ProtocolId): int =
    getOrDefault(newParticipants, pid, Set()).fold(0, (acc, participant) =>
      if optionIsSome(getOrDefault(map.get(pid), participant, None))
        then acc + 1
        else acc
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
    oldSet: Set[DeviceId],
    newSet: Set[DeviceId],
    newThreshold: int
  ): bool = all {
    oldSet.size() >= 1,
    newSet.size() >= 1,
    newThreshold >= 1,
    newThreshold <= newSet.size(),
    not pid.in(oldParticipants.keys()),
    oldParticipants' = oldParticipants.set(pid, oldSet),
    newParticipants' = newParticipants.set(pid, newSet),
    threshold' = threshold.set(pid, newThreshold),
    currentPhase' = currentPhase.set(pid, WaitingProposal),
    approvals' = approvals.set(pid, Set()),
    shareCommitments' = ensureShareMap(shareCommitments, pid, newSet),
    encryptedShares' = ensureShareMap(encryptedShares, pid, newSet),
    lifecycleStatus' = lifecycleStatus.set(pid, Pending),
    lifecycleEffects' = lifecycleEffects.set(pid, Set()),
    outcome' = outcome.set(pid, None)
  }

  action approveResharing(pid: ProtocolId, member: DeviceId): bool =
    val phase = currentPhase.get(pid) {
    val oldSet = oldParticipants.get(pid) {
      phase = WaitingProposal or phase = CollectingApprovals and
      oldSet.contains(member) {
        val updatedApprovals =
          getOrDefault(approvals, pid, Set()).union(Set(member)) {
        val nextPhase =
          if thresholdReached(pid)
            CollectingApprovals
          else
            phase {
          all {
            approvals' = approvals.set(pid, updatedApprovals),
            currentPhase' =
              (if thresholdReached(pid) then currentPhase.set(pid, CollectingApprovals) else currentPhase),
            oldParticipants' = oldParticipants,
            newParticipants' = newParticipants,
            threshold' = threshold,
            shareCommitments' = shareCommitments,
            encryptedShares' = encryptedShares
          }
        }
      }
    }}

  action moveToDistribution(pid: ProtocolId): bool =
    val phase = currentPhase.get(pid) {
      phase = CollectingApprovals and thresholdReached(pid) {
        all {
          currentPhase' = currentPhase.set(pid, DistributingShares),
          oldParticipants' = oldParticipants,
          newParticipants' = newParticipants,
          threshold' = threshold,
          approvals' = approvals,
          shareCommitments' = shareCommitments,
          encryptedShares' = encryptedShares
        }
      }
    }

  action recordCommitment(
    pid: ProtocolId,
    member: DeviceId,
    commitmentHash: int
  ): bool =
    val phase = currentPhase.get(pid) {
    val newSet = newParticipants.get(pid) {
      phase = DistributingShares and
      newSet.contains(member) and
      not optionIsSome(getOrDefault(shareCommitments.get(pid), member, None)) {
        val updated = setShare(shareCommitments, pid, member, Some(commitmentHash)) {
        all {
          shareCommitments' = updated,
          encryptedShares' = encryptedShares,
          currentPhase' = currentPhase,
          oldParticipants' = oldParticipants,
          newParticipants' = newParticipants,
          threshold' = threshold,
          approvals' = approvals
        }
      }
    }}

  action deliverShare(
    pid: ProtocolId,
    member: DeviceId,
    ciphertextHash: int
  ): bool =
    val phase = currentPhase.get(pid) {
    val newSet = newParticipants.get(pid) {
      phase = DistributingShares and
      newSet.contains(member) and
      optionIsSome(getOrDefault(shareCommitments.get(pid), member, None)) and
      not optionIsSome(getOrDefault(encryptedShares.get(pid), member, None)) {
        val updated = setShare(encryptedShares, pid, member, Some(ciphertextHash)) {
        val completedDelivery = newSet.forall(participant =>
          optionIsSome(getOrDefault(updated.get(pid), participant, None))
        ) {
        val nextPhase = if completedDelivery then Completed else phase {
          all {
            encryptedShares' = updated,
            shareCommitments' = shareCommitments,
            currentPhase' = (if completedDelivery then currentPhase.set(pid, Completed) else currentPhase),
            oldParticipants' = oldParticipants,
            newParticipants' = newParticipants,
            threshold' = threshold,
            approvals' = approvals,
            outcome' = (if completedDelivery then outcome.set(pid, Some({
              newThreshold: threshold.get(pid),
              newParticipants: getOrDefault(newParticipants, pid, Set()),
              oldParticipants: getOrDefault(oldParticipants, pid, Set()),
              shareCount: shareCountFor(updated, pid)
            })) else outcome)
          }
        }
      }
    }}}

  action failResharing(pid: ProtocolId): bool =
    val phase = currentPhase.get(pid) {
      not phase = Completed {
        all {
          currentPhase' = currentPhase.set(pid, Failed),
          oldParticipants' = oldParticipants,
          newParticipants' = newParticipants,
          threshold' = threshold,
          approvals' = approvals,
          shareCommitments' = shareCommitments,
          encryptedShares' = encryptedShares,
          lifecycleStatus' = lifecycleStatus.set(pid, Aborted),
          lifecycleEffects' = lifecycleEffects
        }
      }
    }

  action signalComplete(pid: ProtocolId): bool =
    val status = lifecycleStatus.get(pid) {
    val phase = currentPhase.get(pid) {
    val outcomeOpt = outcome.get(pid) {
    val hasOutcome = match outcomeOpt {
      | Some(_) => true
      | None => false
    } {
      status = Pending and phase = Completed and hasOutcome {
        lifecycleStatus' = lifecycleStatus.set(pid, Completed),
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
    }}}}

  action signalAbort(pid: ProtocolId): bool =
    val status = lifecycleStatus.get(pid) {
      status = Pending {
        lifecycleStatus' = lifecycleStatus.set(pid, Aborted),
        lifecycleEffects' = lifecycleEffects,
        currentPhase' = currentPhase.set(pid, Failed),
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
      threshold.get(pid) <= getOrDefault(newParticipants, pid, Set()).size()
    )

  val InvariantApprovalsSubsetOld =
    approvals.keys().forall(pid =>
      getOrDefault(approvals, pid, Set()).subseteq(getOrDefault(oldParticipants, pid, Set()))
    )

  val InvariantPhaseProgression =
    currentPhase.keys().forall(pid =>
      match currentPhase.get(pid) {
        | WaitingProposal => countApprovals(pid) = 0
        | CollectingApprovals =>
            countApprovals(pid) >= 0
        | DistributingShares =>
            thresholdReached(pid)
        | Completed =>
            sharesDelivered(pid) and commitmentsReady(pid)
        | Failed => true
      }
    )

  val InvariantSharesOnlyForNewParticipants =
    encryptedShares.keys().forall(pid =>
      getOrDefault(encryptedShares, pid, Map()).keys().subseteq(getOrDefault(newParticipants, pid, Set()))
    )

  val InvariantLifecycleStatusDefined =
    lifecycleStatus.keys() = currentPhase.keys()

  val InvariantLifecycleCompletionRequiresPhase =
    lifecycleStatus.keys().forall(pid =>
      lifecycleStatus.get(pid) = Completed implies currentPhase.get(pid) = Completed
    )

  val InvariantLifecycleEffectsEmpty =
    lifecycleEffects.keys().forall(pid => lifecycleEffects.get(pid).size() = 0)

  val InvariantOutcomeWhenCompleted =
    lifecycleStatus.keys().forall(pid =>
      lifecycleStatus.get(pid) = Completed implies outcome.get(pid).isSome()
    )
}
