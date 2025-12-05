module protocol_recovery {
  // Base types (consistent with protocol_core)
  type Option[a] = Some(a) | None
  type ProtocolId = str
  type AuthorityId = str
  type GuardianId = str

  // Protocol effects (simplified for this module)
  type ProtocolEffect = { effectType: str, payload: str }

  // Note: Sum type variants must be globally unique in Quint
  type LifecycleStatus = LifecyclePending | LifecycleCompleted | LifecycleAborted
  type RecoveryOutcome = {
    newAuthority: AuthorityId,
    guardiansApproved: Set[GuardianId],
    shareCount: int
  }

  type RecoveryPhase =
    | Idle
    | CollectingApprovals
    | CooldownActive
    | ReconstructingShares
    | RecoveryCompleted
    | RecoveryCancelled

  // ============ STATE ============

  var guardians: ProtocolId -> Set[GuardianId]
  var approvals: ProtocolId -> Set[GuardianId]
  var threshold: ProtocolId -> int
  var currentPhase: ProtocolId -> RecoveryPhase
  var cooldownRemaining: ProtocolId -> int
  var reconstructedAuthority: ProtocolId -> Option[AuthorityId]
  var pendingShares: ProtocolId -> GuardianId -> Option[int]
  var lifecycleStatus: ProtocolId -> LifecycleStatus
  var lifecycleEffects: ProtocolId -> Set[ProtocolEffect]
  var outcome: ProtocolId -> Option[RecoveryOutcome]

  // ============ HELPERS ============

  pure def getOrDefault(m: ProtocolId -> Set[GuardianId], key: ProtocolId, default: Set[GuardianId]): Set[GuardianId] =
    if (key.in(m.keys())) m.get(key) else default

  pure def getOrDefaultInt(m: ProtocolId -> int, key: ProtocolId, default: int): int =
    if (key.in(m.keys())) m.get(key) else default

  pure def optionIsSome(opt: Option[AuthorityId]): bool =
    match opt {
      | Some(_) => true
      | None => false
    }

  pure def optionIsSomeInt(opt: Option[int]): bool =
    match opt {
      | Some(_) => true
      | None => false
    }

  pure def ensureShareMap(
    mapping: ProtocolId -> GuardianId -> Option[int],
    pid: ProtocolId,
    members: Set[GuardianId]
  ): ProtocolId -> GuardianId -> Option[int] =
    if (pid.in(mapping.keys())) mapping
    else
      mapping.set(
        pid,
        members.fold(Map(), (acc, member) => acc.set(member, None))
      )

  def approvalsCount(pid: ProtocolId): int =
    getOrDefault(approvals, pid, Set()).size()

  def thresholdReached(pid: ProtocolId): bool =
    approvalsCount(pid) >= getOrDefaultInt(threshold, pid, 0)

  def allSharesProvided(pid: ProtocolId): bool = {
    val guardianSet = getOrDefault(guardians, pid, Set())
    val shareMap = if (pid.in(pendingShares.keys())) pendingShares.get(pid) else Map()
    guardianSet.forall(guardian =>
      if (guardian.in(shareMap.keys())) optionIsSomeInt(shareMap.get(guardian)) else false
    )
  }

  def shareCount(pid: ProtocolId): int = {
    val guardianSet = getOrDefault(guardians, pid, Set())
    val shareMapLocal = if (pid.in(pendingShares.keys())) pendingShares.get(pid) else Map()
    guardianSet.fold(0, (acc, g) => {
      val hasShare = if (g.in(shareMapLocal.keys())) optionIsSomeInt(shareMapLocal.get(g)) else false
      if (hasShare) acc + 1 else acc
    })
  }

  // ============ INITIALISATION ============

  action init: bool = all {
    guardians' = Map(),
    approvals' = Map(),
    threshold' = Map(),
    currentPhase' = Map(),
    cooldownRemaining' = Map(),
    reconstructedAuthority' = Map(),
    pendingShares' = Map(),
    lifecycleStatus' = Map(),
    lifecycleEffects' = Map(),
    outcome' = Map()
  }

  // ============ ACTIONS ============

  action configureRecovery(
    pid: ProtocolId,
    guardianSet: Set[GuardianId],
    thresholdValue: int
  ): bool = all {
    guardianSet.size() >= 1,
    thresholdValue >= 1,
    thresholdValue <= guardianSet.size(),
    not(pid.in(guardians.keys())),
    guardians' = guardians.set(pid, guardianSet),
    approvals' = approvals.set(pid, Set()),
    threshold' = threshold.set(pid, thresholdValue),
    currentPhase' = currentPhase.set(pid, Idle),
    cooldownRemaining' = cooldownRemaining.set(pid, 0),
    reconstructedAuthority' = reconstructedAuthority.set(pid, None),
    pendingShares' = ensureShareMap(pendingShares, pid, guardianSet),
    lifecycleStatus' = lifecycleStatus.set(pid, LifecyclePending),
    lifecycleEffects' = lifecycleEffects.set(pid, Set()),
    outcome' = outcome
  }

  action initiateRecovery(pid: ProtocolId, authority: AuthorityId, cooldown: int): bool = {
    val phase = if (pid.in(currentPhase.keys())) currentPhase.get(pid) else Idle
    val isIdle = match phase { | Idle => true | _ => false }
    all {
      isIdle,
      cooldown >= 0,
      currentPhase' = currentPhase.set(pid, CollectingApprovals),
      cooldownRemaining' = cooldownRemaining.set(pid, cooldown),
      reconstructedAuthority' = reconstructedAuthority.set(pid, Some(authority)),
      guardians' = guardians,
      approvals' = approvals.set(pid, Set()),
      threshold' = threshold,
      pendingShares' = ensureShareMap(pendingShares, pid, getOrDefault(guardians, pid, Set())),
      lifecycleStatus' = lifecycleStatus,
      lifecycleEffects' = lifecycleEffects,
      outcome' = outcome
    }
  }

  action approveRecovery(pid: ProtocolId, guardian: GuardianId): bool = {
    val phase = if (pid.in(currentPhase.keys())) currentPhase.get(pid) else Idle
    val isCollecting = match phase { | CollectingApprovals => true | _ => false }
    val guardianSet = getOrDefault(guardians, pid, Set())
    val updated = getOrDefault(approvals, pid, Set()).union(Set(guardian))
    val cooldown = getOrDefaultInt(cooldownRemaining, pid, 0)
    all {
      isCollecting,
      guardianSet.contains(guardian),
      approvals' = approvals.set(pid, updated),
      guardians' = guardians,
      currentPhase' =
        (if (thresholdReached(pid) and cooldown > 0)
          currentPhase.set(pid, CooldownActive)
          else currentPhase),
      threshold' = threshold,
      cooldownRemaining' = cooldownRemaining,
      reconstructedAuthority' = reconstructedAuthority,
      pendingShares' = pendingShares,
      lifecycleStatus' = lifecycleStatus,
      lifecycleEffects' = lifecycleEffects,
      outcome' = outcome
    }
  }

  action tickCooldown(pid: ProtocolId): bool = {
    val phase = if (pid.in(currentPhase.keys())) currentPhase.get(pid) else Idle
    val isCooldownActive = match phase { | CooldownActive => true | _ => false }
    val cooldown = getOrDefaultInt(cooldownRemaining, pid, 0)
    val remaining = cooldown - 1
    val nextPhase =
      if (remaining == 0 and thresholdReached(pid)) ReconstructingShares else CooldownActive
    all {
      isCooldownActive,
      cooldown > 0,
      cooldownRemaining' = cooldownRemaining.set(pid, remaining),
      currentPhase' = currentPhase.set(pid, nextPhase),
      guardians' = guardians,
      approvals' = approvals,
      threshold' = threshold,
      reconstructedAuthority' = reconstructedAuthority,
      pendingShares' = pendingShares,
      lifecycleStatus' = lifecycleStatus,
      lifecycleEffects' = lifecycleEffects,
      outcome' = outcome
    }
  }

  action submitShare(
    pid: ProtocolId,
    guardian: GuardianId,
    shareHash: int
  ): bool = {
    val phase = if (pid.in(currentPhase.keys())) currentPhase.get(pid) else Idle
    val isReconstructing = match phase { | ReconstructingShares => true | _ => false }
    val guardianSet = getOrDefault(guardians, pid, Set())
    val shareMap = if (pid.in(pendingShares.keys())) pendingShares.get(pid) else Map()
    val hasShare = if (guardian.in(shareMap.keys())) optionIsSomeInt(shareMap.get(guardian)) else false
    val updatedShareMap = shareMap.set(guardian, Some(shareHash))
    val updated = pendingShares.set(pid, updatedShareMap)
    val authorityOpt = if (pid.in(reconstructedAuthority.keys())) reconstructedAuthority.get(pid) else None
    val hasAuthority = optionIsSome(authorityOpt)
    val extractedAuthority = match authorityOpt { | Some(a) => a | None => "" }
    val allProvided = guardianSet.forall(member =>
      if (member.in(updatedShareMap.keys())) optionIsSomeInt(updatedShareMap.get(member)) else false
    )
    val completed = hasAuthority and allProvided
    val nextPhase = if (completed) RecoveryCompleted else ReconstructingShares
    all {
      isReconstructing,
      guardianSet.contains(guardian),
      not(hasShare),
      pendingShares' = updated,
      currentPhase' = currentPhase.set(pid, nextPhase),
      guardians' = guardians,
      approvals' = approvals,
      threshold' = threshold,
      cooldownRemaining' = cooldownRemaining,
      reconstructedAuthority' = reconstructedAuthority,
      outcome' = if (completed) outcome.set(pid, Some({
        newAuthority: extractedAuthority,
        guardiansApproved: getOrDefault(approvals, pid, Set()),
        shareCount: shareCount(pid)
      })) else outcome,
      lifecycleStatus' = lifecycleStatus,
      lifecycleEffects' = lifecycleEffects
    }
  }

  action cancelRecovery(pid: ProtocolId): bool = {
    val phase = if (pid.in(currentPhase.keys())) currentPhase.get(pid) else Idle
    val isCompleted = match phase { | RecoveryCompleted => true | _ => false }
    all {
      not(isCompleted),
      currentPhase' = currentPhase.set(pid, RecoveryCancelled),
      guardians' = guardians,
      approvals' = approvals,
      threshold' = threshold,
      cooldownRemaining' = cooldownRemaining,
      reconstructedAuthority' = reconstructedAuthority,
      pendingShares' = pendingShares,
      lifecycleStatus' = lifecycleStatus.set(pid, LifecycleAborted),
      lifecycleEffects' = lifecycleEffects,
      outcome' = outcome
    }
  }

  action signalComplete(pid: ProtocolId): bool = {
    val status = if (pid.in(lifecycleStatus.keys())) lifecycleStatus.get(pid) else LifecyclePending
    val isPending = match status { | LifecyclePending => true | _ => false }
    val phase = if (pid.in(currentPhase.keys())) currentPhase.get(pid) else Idle
    val isPhaseCompleted = match phase { | RecoveryCompleted => true | _ => false }
    val outcomeOpt = if (pid.in(outcome.keys())) outcome.get(pid) else None
    val hasOutcome = match outcomeOpt { | Some(_) => true | None => false }
    all {
      isPending,
      isPhaseCompleted,
      allSharesProvided(pid),
      hasOutcome,
      lifecycleStatus' = lifecycleStatus.set(pid, LifecycleCompleted),
      lifecycleEffects' = lifecycleEffects.set(pid, Set()),
      guardians' = guardians,
      approvals' = approvals,
      threshold' = threshold,
      currentPhase' = currentPhase,
      cooldownRemaining' = cooldownRemaining,
      reconstructedAuthority' = reconstructedAuthority,
      pendingShares' = pendingShares,
      outcome' = outcome
    }
  }

  action signalAbort(pid: ProtocolId): bool = {
    val status = if (pid.in(lifecycleStatus.keys())) lifecycleStatus.get(pid) else LifecyclePending
    val isPending = match status { | LifecyclePending => true | _ => false }
    all {
      isPending,
      lifecycleStatus' = lifecycleStatus.set(pid, LifecycleAborted),
      lifecycleEffects' = lifecycleEffects,
      currentPhase' = currentPhase.set(pid, RecoveryCancelled),
      guardians' = guardians,
      approvals' = approvals,
      threshold' = threshold,
      cooldownRemaining' = cooldownRemaining,
      reconstructedAuthority' = reconstructedAuthority,
      pendingShares' = pendingShares,
      outcome' = outcome
    }
  }

  // ============ INVARIANTS ============

  val InvariantThresholdWithinBounds =
    threshold.keys().forall(pid =>
      threshold.get(pid) >= 1 and
      threshold.get(pid) <= getOrDefault(guardians, pid, Set()).size()
    )

  val InvariantApprovalsSubsetGuardians =
    approvals.keys().forall(pid =>
      getOrDefault(approvals, pid, Set()).subseteq(getOrDefault(guardians, pid, Set()))
    )

  val InvariantCooldownNonNegative =
    cooldownRemaining.keys().forall(pid => cooldownRemaining.get(pid) >= 0)

  val InvariantPhaseConsistency =
    currentPhase.keys().forall(pid =>
      match currentPhase.get(pid) {
        | Idle => approvalsCount(pid) == 0
        | CollectingApprovals => cooldownRemaining.get(pid) >= 0
        | CooldownActive =>
            thresholdReached(pid) and cooldownRemaining.get(pid) > 0
        | ReconstructingShares =>
            thresholdReached(pid) and cooldownRemaining.get(pid) == 0
        | RecoveryCompleted =>
            thresholdReached(pid) and allSharesProvided(pid)
        | RecoveryCancelled => true
      }
    )

  val InvariantLifecycleStatusDefined =
    lifecycleStatus.keys() == currentPhase.keys()

  val InvariantLifecycleCompleteRequiresPhase =
    lifecycleStatus.keys().forall(pid => {
      val status = lifecycleStatus.get(pid)
      val phase = currentPhase.get(pid)
      val isCompleted = match status { | LifecycleCompleted => true | _ => false }
      val isPhaseCompleted = match phase { | RecoveryCompleted => true | _ => false }
      isCompleted implies isPhaseCompleted
    })

  val InvariantLifecycleEffectsEmpty =
    lifecycleEffects.keys().forall(pid => lifecycleEffects.get(pid).size() == 0)

  val InvariantOutcomeWhenCompleted =
    lifecycleStatus.keys().forall(pid => {
      val status = lifecycleStatus.get(pid)
      val isCompleted = match status { | LifecycleCompleted => true | _ => false }
      val outcomeOpt = if (pid.in(outcome.keys())) outcome.get(pid) else None
      val hasOutcome = match outcomeOpt { | Some(_) => true | None => false }
      isCompleted implies hasOutcome
    })
}
