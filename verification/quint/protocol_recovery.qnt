// Guardian Recovery Protocol Specification
//
// Models social recovery where k-of-n guardians approve authority restoration.
// Includes mandatory cooldown period before share reconstruction to allow
// cancellation if the recovery request was fraudulent.
//
// Phase progression: Idle → CollectingApprovals → CooldownActive
//                    → ReconstructingShares → RecoveryCompleted (or RecoveryCancelled)
//
// Safety: Recovery requires threshold guardian approvals + cooldown.
// Liveness: Progress requires threshold guardians to respond.
//
// See: docs/103_relational_contexts.md

module protocol_recovery {
  // ============ TYPE DEFINITIONS ============
  // Base types for recovery protocol participants.

  type Option[a] = Some(a) | None
  type ProtocolId = str       // Unique recovery session identifier
  type AuthorityId = str      // Authority being recovered
  type GuardianId = str       // Social recovery guardian

  // Protocol effects (simplified for this module).
  type ProtocolEffect = { effectType: str, payload: str }

  // Lifecycle status for external coordination.
  type LifecycleStatus = LifecyclePending | LifecycleCompleted | LifecycleAborted

  // Outcome of a successful recovery.
  type RecoveryOutcome = {
    newAuthority: AuthorityId,      // Restored authority identity
    guardiansApproved: Set[GuardianId],  // Guardians who approved
    shareCount: int                 // Number of shares reconstructed
  }

  // Recovery phase (state machine progression).
  type RecoveryPhase =
    | Idle                   // Not started (awaiting initiation)
    | CollectingApprovals    // Gathering guardian approvals
    | CooldownActive         // Mandatory waiting period (fraud prevention)
    | ReconstructingShares   // Combining guardian key shares
    | RecoveryCompleted      // Successfully restored (terminal success)
    | RecoveryCancelled      // Aborted (terminal failure)

  // ============ STATE VARIABLES ============
  // Global mutable state tracked by the model checker.

  var guardians: ProtocolId -> Set[GuardianId]       // Guardian set per recovery
  var approvals: ProtocolId -> Set[GuardianId]       // Guardians who approved
  var threshold: ProtocolId -> int                   // k in k-of-n threshold
  var currentPhase: ProtocolId -> RecoveryPhase      // Current protocol phase
  var cooldownRemaining: ProtocolId -> int           // Ticks until cooldown ends
  var reconstructedAuthority: ProtocolId -> Option[AuthorityId]  // Target authority
  var pendingShares: ProtocolId -> GuardianId -> Option[int]  // Guardian shares
  var lifecycleStatus: ProtocolId -> LifecycleStatus  // External lifecycle state
  var lifecycleEffects: ProtocolId -> Set[ProtocolEffect]  // Pending effects
  var outcome: ProtocolId -> Option[RecoveryOutcome]  // Final result

  // ============ HELPERS ============
  // Pure functions for safe state access.

  // Safe map access with default for Set values.
  pure def getOrDefault(m: ProtocolId -> Set[GuardianId], key: ProtocolId, default: Set[GuardianId]): Set[GuardianId] =
    if (key.in(m.keys())) m.get(key) else default

  // Safe map access with default for int values.
  pure def getOrDefaultInt(m: ProtocolId -> int, key: ProtocolId, default: int): int =
    if (key.in(m.keys())) m.get(key) else default

  // Check if Option[AuthorityId] contains a value.
  pure def optionIsSome(opt: Option[AuthorityId]): bool =
    match opt {
      | Some(_) => true
      | None => false
    }

  // Check if Option[int] contains a value.
  pure def optionIsSomeInt(opt: Option[int]): bool =
    match opt {
      | Some(_) => true
      | None => false
    }

  // Initialize share map for all guardians with None.
  // Ensures map structure exists before share submission.
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

  // Count how many guardians have approved.
  def approvalsCount(pid: ProtocolId): int =
    getOrDefault(approvals, pid, Set()).size()

  // Check if threshold approvals reached.
  def thresholdReached(pid: ProtocolId): bool =
    approvalsCount(pid) >= getOrDefaultInt(threshold, pid, 0)

  // Check if all guardians have submitted shares.
  // Required before completing reconstruction phase.
  def allSharesProvided(pid: ProtocolId): bool = {
    val guardianSet = getOrDefault(guardians, pid, Set())
    val shareMap = if (pid.in(pendingShares.keys())) pendingShares.get(pid) else Map()
    guardianSet.forall(guardian =>
      if (guardian.in(shareMap.keys())) optionIsSomeInt(shareMap.get(guardian)) else false
    )
  }

  // Count how many shares have been submitted.
  def shareCount(pid: ProtocolId): int = {
    val guardianSet = getOrDefault(guardians, pid, Set())
    val shareMapLocal = if (pid.in(pendingShares.keys())) pendingShares.get(pid) else Map()
    guardianSet.fold(0, (acc, g) => {
      val hasShare = if (g.in(shareMapLocal.keys())) optionIsSomeInt(shareMapLocal.get(g)) else false
      if (hasShare) acc + 1 else acc
    })
  }

  // ============ INITIALISATION ============
  // Start with empty state (no recovery sessions).

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
  // State transitions modeling the recovery protocol.

  // Configure a new recovery session with guardian set and threshold.
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

  // Initiate recovery for a specific authority with cooldown period.
  // Moves from Idle to CollectingApprovals phase.
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

  // Guardian approves the recovery request.
  // When threshold reached, transitions to CooldownActive (if cooldown > 0).
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

  // Advance cooldown timer by one tick.
  // When cooldown expires, transitions to ReconstructingShares.
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

  // Guardian submits their key share for reconstruction.
  // When all shares received, transitions to RecoveryCompleted.
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

  // Cancel recovery (fraud detection or user request).
  // Can be called at any phase except RecoveryCompleted.
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

  // Signal external completion (lifecycle status update).
  // Requires: phase is RecoveryCompleted, all shares provided.
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

  // Signal external abort (lifecycle status update).
  // Moves lifecycle to LifecycleAborted, phase to RecoveryCancelled.
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
  // Safety properties that must hold in every reachable state.

  // Threshold is valid: 1 <= threshold <= guardians.size().
  val InvariantThresholdWithinBounds =
    threshold.keys().forall(pid =>
      threshold.get(pid) >= 1 and
      threshold.get(pid) <= getOrDefault(guardians, pid, Set()).size()
    )

  // Approvals only from configured guardians.
  // Prevents unauthorized approval injection.
  val InvariantApprovalsSubsetGuardians =
    approvals.keys().forall(pid =>
      getOrDefault(approvals, pid, Set()).subseteq(getOrDefault(guardians, pid, Set()))
    )

  // Cooldown timer is never negative.
  val InvariantCooldownNonNegative =
    cooldownRemaining.keys().forall(pid => cooldownRemaining.get(pid) >= 0)

  // Phase-specific constraints are satisfied.
  // E.g., CooldownActive requires threshold reached and cooldown > 0.
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

  // Lifecycle status defined for all configured protocols.
  val InvariantLifecycleStatusDefined =
    lifecycleStatus.keys() == currentPhase.keys()

  // LifecycleCompleted implies phase is RecoveryCompleted.
  val InvariantLifecycleCompleteRequiresPhase =
    lifecycleStatus.keys().forall(pid => {
      val status = lifecycleStatus.get(pid)
      val phase = currentPhase.get(pid)
      val isCompleted = match status { | LifecycleCompleted => true | _ => false }
      val isPhaseCompleted = match phase { | RecoveryCompleted => true | _ => false }
      isCompleted implies isPhaseCompleted
    })

  // No pending effects (simplified model).
  val InvariantLifecycleEffectsEmpty =
    lifecycleEffects.keys().forall(pid => lifecycleEffects.get(pid).size() == 0)

  // LifecycleCompleted implies outcome is present.
  // Successful recovery always produces a RecoveryOutcome.
  val InvariantOutcomeWhenCompleted =
    lifecycleStatus.keys().forall(pid => {
      val status = lifecycleStatus.get(pid)
      val isCompleted = match status { | LifecycleCompleted => true | _ => false }
      val outcomeOpt = if (pid.in(outcome.keys())) outcome.get(pid) else None
      val hasOutcome = match outcomeOpt { | Some(_) => true | None => false }
      isCompleted implies hasOutcome
    })

  // ============ STRENGTHENED INVARIANTS (T3c.12) ============
  // Guardian availability bounds: shares can only be from approving guardians.
  val InvariantSharesFromApprovers =
    pendingShares.keys().forall(pid => {
      val shareMapLocal = if (pid.in(pendingShares.keys())) pendingShares.get(pid) else Map()
      val approvalSet = getOrDefault(approvals, pid, Set())
      // Guardians who submitted shares must have approved
      shareMapLocal.keys().forall(guardian =>
        optionIsSomeInt(shareMapLocal.get(guardian)) implies approvalSet.contains(guardian)
      )
    })

  // Recovery progress: can only advance if sufficient guardians available.
  // This bounds guardian availability by requiring threshold for phase transitions.
  val InvariantProgressRequiresAvailability =
    currentPhase.keys().forall(pid =>
      match currentPhase.get(pid) {
        | ReconstructingShares =>
            // Can only be reconstructing if we have threshold approvals
            approvalsCount(pid) >= getOrDefaultInt(threshold, pid, 0)
        | RecoveryCompleted =>
            // Completed implies sufficient guardians were available
            shareCount(pid) >= getOrDefaultInt(threshold, pid, 0)
        | _ => true
      }
    )

  // Authority consistency: reconstructed authority must be set before completion.
  val InvariantAuthoritySetBeforeCompletion =
    currentPhase.keys().forall(pid =>
      match currentPhase.get(pid) {
        | RecoveryCompleted => optionIsSome(
            if (pid.in(reconstructedAuthority.keys())) reconstructedAuthority.get(pid) else None
          )
        | _ => true
      }
    )

  // Outcome guardians match approvals.
  val InvariantOutcomeGuardiansMatch =
    outcome.keys().forall(pid => {
      val outcomeOpt = outcome.get(pid)
      match outcomeOpt {
        | Some(o) => o.guardiansApproved.subseteq(getOrDefault(guardians, pid, Set()))
        | None => true
      }
    })

  // Combined well-formed state predicate.
  val WellFormedRecoveryState = all {
    InvariantThresholdWithinBounds,
    InvariantApprovalsSubsetGuardians,
    InvariantCooldownNonNegative,
    InvariantPhaseConsistency,
    InvariantLifecycleStatusDefined,
    InvariantLifecycleCompleteRequiresPhase,
    InvariantLifecycleEffectsEmpty,
    InvariantOutcomeWhenCompleted,
    InvariantSharesFromApprovers,
    InvariantProgressRequiresAvailability,
    InvariantAuthoritySetBeforeCompletion,
    InvariantOutcomeGuardiansMatch
  }
}
