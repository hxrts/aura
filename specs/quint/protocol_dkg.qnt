module protocol_dkg {
  import protocol_core as core from "protocol_core"

  type Option[a] = Some(a) | None

  type ProtocolId = core.ProtocolId
  type DeviceId = core.DeviceId

  type DkgPhase =
    | Unconfigured
    | AwaitingCommitments
    | AwaitingVerifications
    | AggregatingShares
    | Completed
    | Failed

  type CommitmentState = ProtocolId -> DeviceId -> Option[int]
  type VerificationState = ProtocolId -> DeviceId -> Option[bool]
  type ShareState = ProtocolId -> DeviceId -> Option[int]
  type LifecycleStatus = Pending | Completed | Aborted
  type DkdOutcome = {
    derivedKey: int,
    publicKey: int,
    transcript: int,
    participants: Set[DeviceId],
    threshold: int
  }

  // ============ STATE ============

  var participants: ProtocolId -> Set[DeviceId]
  var threshold: ProtocolId -> int
  var phase: ProtocolId -> DkgPhase
  var dealerCommitments: CommitmentState
  var verifierResponses: VerificationState
  var participantShares: ShareState
  var publicKeyPackage: ProtocolId -> Option[int]
  var transcriptDigest: ProtocolId -> Option[int]
  var lifecycleStatus: ProtocolId -> LifecycleStatus
  var lifecycleEffects: ProtocolId -> Set[core.ProtocolEffect]
  var outcome: ProtocolId -> Option[DkdOutcome]

  // ============ HELPERS ============

  pure def getOrDefault[K, V](m: K -> V, key: K, default: V): V =
    if key.in(m.keys()) then m.get(key) else default

  pure def optionIsSome[a](opt: Option[a]): bool =
    match opt {
      | Some(_) => true
      | None => false
    }

  pure def optionValueOr[a](opt: Option[a], fallback: a): a =
    match opt {
      | Some(value) => value
      | None => fallback
    }

  pure def ensureParticipantMap(
    mapping: ProtocolId -> DeviceId -> Option[int],
    pid: ProtocolId,
    members: Set[DeviceId]
  ): ProtocolId -> DeviceId -> Option[int] =
    if pid.in(mapping.keys()) then mapping
    else
      mapping.set(
        pid,
        members.fold(Map(), (acc, member) => acc.set(member, None))
      )

  pure def updateMapping(
    mapping: ProtocolId -> DeviceId -> Option[int],
    pid: ProtocolId,
    member: DeviceId,
    value: Option[int],
    members: Set[DeviceId]
  ): ProtocolId -> DeviceId -> Option[int] =
    ensureParticipantMap(mapping, pid, members).set(
      pid,
      ensureParticipantMap(mapping, pid, members).get(pid).set(member, value)
    )

  pure def ensureVerificationMap(
    mapping: ProtocolId -> DeviceId -> Option[bool],
    pid: ProtocolId,
    members: Set[DeviceId]
  ): ProtocolId -> DeviceId -> Option[bool] =
    if pid.in(mapping.keys()) then mapping
    else
      mapping.set(
        pid,
        members.fold(Map(), (acc, member) => acc.set(member, None))
      )

  pure def updateVerification(
    mapping: ProtocolId -> DeviceId -> Option[bool],
    pid: ProtocolId,
    member: DeviceId,
    value: Option[bool],
    members: Set[DeviceId]
  ): ProtocolId -> DeviceId -> Option[bool] =
    ensureVerificationMap(mapping, pid, members).set(
      pid,
      ensureVerificationMap(mapping, pid, members).get(pid).set(member, value)
    )

  pure def countFilled(
    mapping: ProtocolId -> DeviceId -> Option[int],
    pid: ProtocolId
  ): int =
    getOrDefault(participants, pid, Set()).fold(
      0,
      (acc, member) =>
        acc +
        (if optionIsSome(getOrDefault(mapping.get(pid), member, None)) then 1 else 0)
    )

  pure def verificationMap(
    pid: ProtocolId
  ): DeviceId -> Option[bool] =
    ensureVerificationMap(
      verifierResponses,
      pid,
      getOrDefault(participants, pid, Set())
    ).get(pid)

  pure def shareMap(
    pid: ProtocolId
  ): DeviceId -> Option[int] =
    ensureParticipantMap(
      participantShares,
      pid,
      getOrDefault(participants, pid, Set())
    ).get(pid)

  pure def allVerified(pid: ProtocolId): bool =
    getOrDefault(participants, pid, Set()).forall(member =>
      verificationMap(pid).get(member) = Some(true)
    )

  pure def sharesComplete(pid: ProtocolId): bool =
    getOrDefault(participants, pid, Set()).forall(member =>
      optionIsSome(shareMap(pid).get(member))
    )

  pure def thresholdReached(pid: ProtocolId, mapping: ProtocolId -> DeviceId -> Option[int]): bool =
    countFilled(mapping, pid) >= getOrDefault(threshold, pid, 0)

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
    members: Set[DeviceId],
    t: int
  ): bool = all {
    members.size() >= 1,
    t >= 1,
    t <= members.size(),
    not pid.in(participants.keys()),
    participants' = participants.set(pid, members),
    threshold' = threshold.set(pid, t),
    phase' = phase.set(pid, AwaitingCommitments),
    dealerCommitments' = ensureParticipantMap(dealerCommitments, pid, members),
    verifierResponses' = ensureVerificationMap(verifierResponses, pid, members),
    participantShares' = ensureParticipantMap(participantShares, pid, members),
    publicKeyPackage' = publicKeyPackage.set(pid, None),
    transcriptDigest' = transcriptDigest.set(pid, None),
    lifecycleStatus' = lifecycleStatus.set(pid, Pending),
    lifecycleEffects' = lifecycleEffects.set(pid, Set()),
    outcome' = outcome.set(pid, None)
  }

  action submitCommitment(
    pid: ProtocolId,
    participant: DeviceId,
    commitment: int
  ): bool =
    val members = participants.get(pid) {
    val currentPhase = phase.get(pid) {
      members.contains(participant) and
      (currentPhase = AwaitingCommitments or currentPhase = AwaitingVerifications) and
      optionIsSome(getOrDefault(dealerCommitments.get(pid), participant, None)) = false {
        val updated = updateMapping(dealerCommitments, pid, participant, Some(commitment), members) {
        val nextPhase =
          if thresholdReached(pid, updated) then AwaitingVerifications else currentPhase {
          all {
            dealerCommitments' = updated,
            phase' = phase.set(pid, nextPhase),
            participants' = participants,
            threshold' = threshold,
            verifierResponses' = verifierResponses,
            participantShares' = participantShares,
            publicKeyPackage' = publicKeyPackage,
            transcriptDigest' = transcriptDigest
          }
        }
      }
    }}

  action verifyCommitment(
    pid: ProtocolId,
    participant: DeviceId,
    valid: bool
  ): bool =
    val members = participants.get(pid) {
    val currentPhase = phase.get(pid) {
      members.contains(participant) and
      currentPhase = AwaitingVerifications and
      optionIsSome(getOrDefault(dealerCommitments.get(pid), participant, None)) {
        val updated = updateVerification(
          verifierResponses,
          pid,
          participant,
          Some(valid),
          members
        ) {
        val nextPhase =
          if allVerified(pid) then AggregatingShares else currentPhase {
          val nextState =
            if not valid then Failed else nextPhase {
            all {
              participants' = participants,
              threshold' = threshold,
              dealerCommitments' = dealerCommitments,
              verifierResponses' = updated,
              participantShares' = participantShares,
              publicKeyPackage' = publicKeyPackage,
              transcriptDigest' = transcriptDigest,
              phase' = phase.set(pid, nextState)
            }
        }}}}

  action submitShare(
    pid: ProtocolId,
    participant: DeviceId,
    shareDigest: int
  ): bool =
    val members = participants.get(pid) {
    val currentPhase = phase.get(pid) {
      currentPhase = AggregatingShares and
      members.contains(participant) and
      optionIsSome(getOrDefault(verifierResponses.get(pid), participant, None)) and
      optionValueOr(getOrDefault(verifierResponses.get(pid), participant, None), false) = true and
      optionIsSome(getOrDefault(participantShares.get(pid), participant, None)) = false {
        val updated = updateMapping(participantShares, pid, participant, Some(shareDigest), members) {
        val nextPhase =
          if sharesComplete(pid) then Completed else AggregatingShares {
          all {
            participantShares' = updated,
            phase' = phase.set(pid, nextPhase),
            participants' = participants,
            threshold' = threshold,
            dealerCommitments' = dealerCommitments,
            verifierResponses' = verifierResponses,
            publicKeyPackage' = publicKeyPackage,
            transcriptDigest' = transcriptDigest
          }
        }
      }
    }}

  action finalizeDkg(
    pid: ProtocolId,
    derivedKeyHash: int,
    publicKeyHash: int,
    transcriptHash: int
  ): bool =
    val currentPhase = phase.get(pid) {
      currentPhase = Completed and sharesComplete(pid) {
        all {
          publicKeyPackage' = publicKeyPackage.set(pid, Some(publicKeyHash)),
          transcriptDigest' = transcriptDigest.set(pid, Some(transcriptHash)),
          participants' = participants,
          threshold' = threshold,
          dealerCommitments' = dealerCommitments,
          verifierResponses' = verifierResponses,
          participantShares' = participantShares,
          phase' = phase,
          outcome' = outcome.set(pid, Some({
            derivedKey: derivedKeyHash,
            publicKey: publicKeyHash,
            transcript: transcriptHash,
            participants: getOrDefault(participants, pid, Set()),
            threshold: threshold.get(pid)
          }))
        }
      }
    }

  action failDkg(pid: ProtocolId): bool =
    val currentPhase = phase.get(pid) {
      not currentPhase = Completed {
        all {
          phase' = phase.set(pid, Failed),
          participants' = participants,
          threshold' = threshold,
          dealerCommitments' = dealerCommitments,
          verifierResponses' = verifierResponses,
          participantShares' = participantShares,
          publicKeyPackage' = publicKeyPackage,
          transcriptDigest' = transcriptDigest,
          lifecycleStatus' = lifecycleStatus.set(pid, Aborted),
          lifecycleEffects' = lifecycleEffects
        }
      }
    }

  action signalComplete(pid: ProtocolId): bool =
    val status = lifecycleStatus.get(pid) {
    val currentPhase = phase.get(pid) {
      status = Pending and currentPhase = Completed {
        lifecycleStatus' = lifecycleStatus.set(pid, Completed),
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
    }}

  action signalAbort(pid: ProtocolId): bool =
    val status = lifecycleStatus.get(pid) {
      status = Pending {
        lifecycleStatus' = lifecycleStatus.set(pid, Aborted),
        lifecycleEffects' = lifecycleEffects,
        phase' = phase.set(pid, Failed),
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
      threshold.get(pid) <= getOrDefault(participants, pid, Set()).size()
    )

  val InvariantCommitmentsRequireConfiguration =
    dealerCommitments.keys().subseteq(participants.keys())

  val InvariantPhaseCommitmentCounts =
    phase.keys().forall(pid =>
      match phase.get(pid) {
        | Unconfigured => true
        | AwaitingCommitments => true
        | AwaitingVerifications =>
            thresholdReached(pid, dealerCommitments)
        | AggregatingShares =>
            thresholdReached(pid, dealerCommitments) and allVerified(pid)
        | Completed =>
            thresholdReached(pid, dealerCommitments) and
            allVerified(pid) and
            sharesComplete(pid) and
            match publicKeyPackage.get(pid) {
              | Some(_) =>
                  match transcriptDigest.get(pid) {
                    | Some(_) => true
                    | None => false
                  }
              | None => false
            }
        | Failed => true
      }
    )

  val InvariantSharesOnlyAfterVerification =
    participantShares.keys().forall(pid =>
      getOrDefault(participants, pid, Set()).forall(member =>
        optionIsSome(getOrDefault(participantShares.get(pid), member, None)) implies
          optionValueOr(getOrDefault(verifierResponses.get(pid), member, None), false)
      )
    )

  val InvariantCompletedImpliesPublicKey =
    phase.keys().forall(pid =>
      phase.get(pid) = Completed implies
        match publicKeyPackage.get(pid) {
          | Some(_) =>
              match transcriptDigest.get(pid) {
                | Some(_) => true
                | None => false
              }
          | None => false
        }
    )

  val InvariantLifecycleStatusDefined =
    lifecycleStatus.keys() = phase.keys()

  val InvariantLifecycleStatusMatchesPhase =
    lifecycleStatus.keys().forall(pid =>
      lifecycleStatus.get(pid) = Completed implies phase.get(pid) = Completed
    )

  val InvariantLifecycleEffectsEmpty =
    lifecycleEffects.keys().forall(pid => lifecycleEffects.get(pid).size() = 0)

  val InvariantOutcomeWhenCompleted =
    lifecycleStatus.keys().forall(pid =>
      lifecycleStatus.get(pid) = Completed implies outcome.get(pid).isSome()
    )
}
