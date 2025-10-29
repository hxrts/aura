module protocol_recovery {
  import protocol_core as core from "protocol_core"

  type Option[a] = Some(a) | None

  type ProtocolId = core.ProtocolId
  type DeviceId = core.DeviceId
  type GuardianId = DeviceId
  type LifecycleStatus = Pending | Completed | Aborted
  type RecoveryOutcome = {
    newDevice: DeviceId,
    guardiansApproved: Set[GuardianId],
    shareCount: int
  }

  type RecoveryPhase =
    | Idle
    | CollectingApprovals
    | CooldownActive
    | ReconstructingShares
    | Completed
    | Cancelled

  // ============ STATE ============

  var guardians: ProtocolId -> Set[GuardianId]
  var approvals: ProtocolId -> Set[GuardianId]
  var threshold: ProtocolId -> int
  var currentPhase: ProtocolId -> RecoveryPhase
  var cooldownRemaining: ProtocolId -> int
  var reconstructedDevice: ProtocolId -> Option[DeviceId]
  var pendingShares: ProtocolId -> GuardianId -> Option[int]
  var lifecycleStatus: ProtocolId -> LifecycleStatus
  var lifecycleEffects: ProtocolId -> Set[core.ProtocolEffect]
  var outcome: ProtocolId -> Option[RecoveryOutcome]

  // ============ HELPERS ============

  pure def getOrDefault[K, V](m: K -> V, key: K, default: V): V =
    if key.in(m.keys()) then m.get(key) else default

  pure def optionIsSome[a](opt: Option[a]): bool =
    match opt {
      | Some(_) => true
      | None => false
    }

  pure def ensureShareMap(
    mapping: ProtocolId -> GuardianId -> Option[int],
    pid: ProtocolId,
    members: Set[GuardianId]
  ): ProtocolId -> GuardianId -> Option[int] =
    if pid.in(mapping.keys()) then mapping
    else
      mapping.set(
        pid,
        members.fold(Map(), (acc, member) => acc.set(member, None))
      )

  pure def approvalsCount(pid: ProtocolId): int =
    getOrDefault(approvals, pid, Set()).size()

  pure def thresholdReached(pid: ProtocolId): bool =
    approvalsCount(pid) >= getOrDefault(threshold, pid, 0)

  pure def allSharesProvided(pid: ProtocolId): bool =
    getOrDefault(guardians, pid, Set()).forall(guardian =>
      optionIsSome(getOrDefault(pendingShares.get(pid), guardian, None))
    )

  pure def shareCount(pid: ProtocolId): int =
    getOrDefault(guardians, pid, Set()).fold(0, (acc, guardian) =>
      if optionIsSome(getOrDefault(pendingShares.get(pid), guardian, None))
        then acc + 1
        else acc
    )

  // ============ INITIALISATION ============

  action init: bool = all {
    guardians' = Map(),
    approvals' = Map(),
    threshold' = Map(),
    currentPhase' = Map(),
    cooldownRemaining' = Map(),
    reconstructedDevice' = Map(),
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
    not pid.in(guardians.keys()),
    guardians' = guardians.set(pid, guardianSet),
    approvals' = approvals.set(pid, Set()),
    threshold' = threshold.set(pid, thresholdValue),
    currentPhase' = currentPhase.set(pid, Idle),
    cooldownRemaining' = cooldownRemaining.set(pid, 0),
    reconstructedDevice' = reconstructedDevice.set(pid, None),
    pendingShares' = ensureShareMap(pendingShares, pid, guardianSet),
    lifecycleStatus' = lifecycleStatus.set(pid, Pending),
    lifecycleEffects' = lifecycleEffects.set(pid, Set())
  }

  action initiateRecovery(pid: ProtocolId, device: DeviceId, cooldown: int): bool =
    val phase = currentPhase.get(pid) {
      phase = Idle and cooldown >= 0 {
        all {
          currentPhase' = currentPhase.set(pid, CollectingApprovals),
          cooldownRemaining' = cooldownRemaining.set(pid, cooldown),
          reconstructedDevice' = reconstructedDevice.set(pid, Some(device)),
          guardians' = guardians,
          approvals' = approvals.set(pid, Set()),
          threshold' = threshold,
          pendingShares' = ensureShareMap(pendingShares, pid, getOrDefault(guardians, pid, Set()))
        }
      }
    }

  action approveRecovery(pid: ProtocolId, guardian: GuardianId): bool =
    val phase = currentPhase.get(pid) {
    val guardianSet = guardians.get(pid) {
      phase = CollectingApprovals and guardianSet.contains(guardian) {
        val updated = getOrDefault(approvals, pid, Set()).union(Set(guardian)) {
        all {
          approvals' = approvals.set(pid, updated),
          guardians' = guardians,
          currentPhase' =
            (if thresholdReached(pid) and getOrDefault(cooldownRemaining, pid, 0) > 0
              then currentPhase.set(pid, CooldownActive)
              else currentPhase),
          threshold' = threshold,
          cooldownRemaining' = cooldownRemaining,
          reconstructedDevice' = reconstructedDevice,
          pendingShares' = pendingShares
        }
      }
    }}

  action tickCooldown(pid: ProtocolId): bool =
    val phase = currentPhase.get(pid) {
      phase = CooldownActive and getOrDefault(cooldownRemaining, pid, 0) > 0 {
        val remaining = cooldownRemaining.get(pid) - 1 {
        val nextPhase =
          if remaining = 0 and thresholdReached(pid) then ReconstructingShares else CooldownActive {
          all {
            cooldownRemaining' = cooldownRemaining.set(pid, remaining),
            currentPhase' = currentPhase.set(pid, nextPhase),
            guardians' = guardians,
            approvals' = approvals,
            threshold' = threshold,
            reconstructedDevice' = reconstructedDevice,
            pendingShares' = pendingShares
          }
        }
      }
    }}

  action submitShare(
    pid: ProtocolId,
    guardian: GuardianId,
    shareHash: int
  ): bool =
    val phase = currentPhase.get(pid) {
    val guardianSet = guardians.get(pid) {
      phase = ReconstructingShares and guardianSet.contains(guardian) and
      optionIsSome(getOrDefault(pendingShares.get(pid), guardian, None)) = false {
        val updated = pendingShares.set(
          pid,
          pendingShares.get(pid).set(guardian, Some(shareHash))
        ) {
        val deviceOpt = reconstructedDevice.get(pid) {
        val completed = match deviceOpt {
          | Some(_) =>
              getOrDefault(guardians, pid, Set()).forall(member =>
                optionIsSome(getOrDefault(updated.get(pid), member, None))
              )
          | None => false
        } {
        val nextPhase =
          if completed then Completed else ReconstructingShares {
          all {
            pendingShares' = updated,
            currentPhase' = currentPhase.set(pid, nextPhase),
            guardians' = guardians,
            approvals' = approvals,
            threshold' = threshold,
            cooldownRemaining' = cooldownRemaining,
            reconstructedDevice' = reconstructedDevice,
            outcome' = (if completed then outcome.set(pid, Some({
              newDevice: match deviceOpt {
                | Some(device) => device
                | None => "unknown"
              },
              guardiansApproved: getOrDefault(approvals, pid, Set()),
              shareCount: shareCount(pid)
            })) else outcome)
          }
        }
      }
    }}}

  action cancelRecovery(pid: ProtocolId): bool =
    val phase = currentPhase.get(pid) {
      not phase = Completed {
        all {
          currentPhase' = currentPhase.set(pid, Cancelled),
          guardians' = guardians,
          approvals' = approvals,
          threshold' = threshold,
          cooldownRemaining' = cooldownRemaining,
          reconstructedDevice' = reconstructedDevice,
          pendingShares' = pendingShares,
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
      status = Pending and phase = Completed and allSharesProvided(pid) and hasOutcome {
        lifecycleStatus' = lifecycleStatus.set(pid, Completed),
        lifecycleEffects' = lifecycleEffects.set(pid, Set()),
        guardians' = guardians,
        approvals' = approvals,
        threshold' = threshold,
        currentPhase' = currentPhase,
        cooldownRemaining' = cooldownRemaining,
        reconstructedDevice' = reconstructedDevice,
        pendingShares' = pendingShares,
        outcome' = outcome
      }
    }}}}

  action signalAbort(pid: ProtocolId): bool =
    val status = lifecycleStatus.get(pid) {
      status = Pending {
        lifecycleStatus' = lifecycleStatus.set(pid, Aborted),
        lifecycleEffects' = lifecycleEffects,
        currentPhase' = currentPhase.set(pid, Cancelled),
        guardians' = guardians,
        approvals' = approvals,
        threshold' = threshold,
        cooldownRemaining' = cooldownRemaining,
        reconstructedDevice' = reconstructedDevice,
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
        | Idle => approvalsCount(pid) = 0
        | CollectingApprovals => cooldownRemaining.get(pid) >= 0
        | CooldownActive =>
            thresholdReached(pid) and cooldownRemaining.get(pid) > 0
        | ReconstructingShares =>
            thresholdReached(pid) and cooldownRemaining.get(pid) = 0
        | Completed =>
            thresholdReached(pid) and allSharesProvided(pid)
        | Cancelled => true
      }
    )

  val InvariantLifecycleStatusDefined =
    lifecycleStatus.keys() = currentPhase.keys()

  val InvariantLifecycleCompleteRequiresPhase =
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
