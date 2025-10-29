module protocol_dkd {
  import protocol_core as core from "protocol_core"

  type Option[a] = Some(a) | None

  type ProtocolId = core.ProtocolId
  type DeviceId = core.DeviceId

  type DkdPhase =
    | WaitingCommitments
    | WaitingReveals
    | Aggregating
    | Finished
    | Aborted

  type CommitmentState = ProtocolId -> DeviceId -> Option[int]
  type RevealState = ProtocolId -> DeviceId -> Option[int]

  // ============ STATE ============

  var participants: ProtocolId -> Set[DeviceId]
  var phases: ProtocolId -> DkdPhase
  var commitments: CommitmentState
  var reveals: RevealState
  var thresholds: ProtocolId -> int
  var derivedKey: ProtocolId -> Option[int]
  var transcriptHash: ProtocolId -> Option[int]

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
    mapping: CommitmentState,
    pid: ProtocolId,
    members: Set[DeviceId]
  ): CommitmentState =
    if pid.in(mapping.keys()) then mapping
    else
      mapping.set(
        pid,
        members.fold(Map(), (acc, member) => acc.set(member, None))
      )

  pure def countFilled(
    mapping: CommitmentState,
    pid: ProtocolId
  ): int =
    getOrDefault(participants, pid, Set()).fold(
      0,
      (acc, member) =>
        acc +
        (if optionIsSome(getOrDefault(mapping.get(pid), member, None)) then 1 else 0)
    )

  pure def allMembers(mapping: CommitmentState, pid: ProtocolId): bool =
    getOrDefault(participants, pid, Set()).forall(member =>
      optionIsSome(getOrDefault(mapping.get(pid), member, None))
    )

  pure def setMappingValue(
    mapping: CommitmentState,
    pid: ProtocolId,
    member: DeviceId,
    value: Option[int]
  ): CommitmentState =
    ensureParticipantMap(mapping, pid, getOrDefault(participants, pid, Set())).set(
      pid,
      ensureParticipantMap(mapping, pid, getOrDefault(participants, pid, Set()))
        .get(pid)
        .set(member, value)
    )

  pure def thresholdReached(pid: ProtocolId, mapping: CommitmentState): bool =
    countFilled(mapping, pid) >= getOrDefault(thresholds, pid, 0)

  pure def revealsMatchCommitments(pid: ProtocolId): bool =
    getOrDefault(participants, pid, Set()).forall(member =>
      match getOrDefault(reveals.get(pid), member, None) {
        | Some(_) => optionIsSome(getOrDefault(commitments.get(pid), member, None))
        | None => true
      }
    )

  // ============ INITIALISATION ============

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

  action configureDkd(
    pid: ProtocolId,
    members: Set[DeviceId],
    threshold: int
  ): bool = all {
    members.size() >= 1,
    threshold >= 1,
    threshold <= members.size(),
    not pid.in(participants.keys()),
    participants' = participants.set(pid, members),
    phases' = phases.set(pid, WaitingCommitments),
    commitments' = ensureParticipantMap(commitments, pid, members),
    reveals' = ensureParticipantMap(reveals, pid, members),
    thresholds' = thresholds.set(pid, threshold),
    derivedKey' = derivedKey.set(pid, None),
    transcriptHash' = transcriptHash.set(pid, None)
  }

  action recordCommitment(
    pid: ProtocolId,
    member: DeviceId,
    commitment: int
  ): bool =
    val members = participants.get(pid) {
    val state = phases.get(pid) {
      members.contains(member) and
      (state = WaitingCommitments or state = WaitingReveals) and
      optionIsSome(getOrDefault(commitments.get(pid), member, None)) = false {
        val updatedCommitments = setMappingValue(
          commitments,
          pid,
          member,
          Some(commitment)
        ) {
        val newPhase =
          if thresholdReached(pid, updatedCommitments) then WaitingReveals else state {
          all {
            participants' = participants,
            commitments' = updatedCommitments,
            reveals' = reveals,
            phases' = phases.set(pid, newPhase),
            thresholds' = thresholds,
            derivedKey' = derivedKey,
            transcriptHash' = transcriptHash
          }
        }
      }
    }}

  action recordReveal(
    pid: ProtocolId,
    member: DeviceId,
    share: int
  ): bool =
    val members = participants.get(pid) {
    val state = phases.get(pid) {
      members.contains(member) and
      (state = WaitingReveals or state = Aggregating) and
      optionIsSome(getOrDefault(commitments.get(pid), member, None)) and
      optionIsSome(getOrDefault(reveals.get(pid), member, None)) = false {
        val updatedReveals = setMappingValue(
          reveals,
          pid,
          member,
          Some(share)
        ) {
        val newPhase =
          if thresholdReached(pid, updatedReveals) then Aggregating else state {
          all {
            participants' = participants,
            commitments' = commitments,
            reveals' = updatedReveals,
            phases' = phases.set(pid, newPhase),
            thresholds' = thresholds,
            derivedKey' = derivedKey,
            transcriptHash' = transcriptHash
          }
        }
      }
    }}

  action finalizeDkd(
    pid: ProtocolId,
    derivedKeyHash: int,
    transcript: int
  ): bool =
    val state = phases.get(pid) {
      state = Aggregating and
      thresholdReached(pid, reveals) and
      revealsMatchCommitments(pid) {
        all {
          phases' = phases.set(pid, Finished),
          derivedKey' = derivedKey.set(pid, Some(derivedKeyHash)),
          transcriptHash' = transcriptHash.set(pid, Some(transcript)),
          participants' = participants,
          commitments' = commitments,
          reveals' = reveals,
          thresholds' = thresholds
        }
      }
    }

  action abortDkd(pid: ProtocolId): bool =
    val state = phases.get(pid) {
      not state = Finished {
        all {
          phases' = phases.set(pid, Aborted),
          participants' = participants,
          commitments' = commitments,
          reveals' = reveals,
          thresholds' = thresholds,
          derivedKey' = derivedKey,
          transcriptHash' = transcriptHash
        }
      }
    }

  // ============ INVARIANTS ============

  val InvariantThresholdsWithinBounds =
    thresholds.keys().forall(pid =>
      thresholds.get(pid) >= 1 and
      thresholds.get(pid) <= getOrDefault(participants, pid, Set()).size()
    )

  val InvariantCommitmentsAtMostOne =
    commitments.keys().forall(pid =>
      getOrDefault(participants, pid, Set()).forall(member =>
        match getOrDefault(commitments.get(pid), member, None) {
          | Some(_) => true
          | None => true
        }
      )
    )

  val InvariantRevealsRequireCommitment =
    reveals.keys().forall(pid => revealsMatchCommitments(pid))

  val InvariantPhaseProgression =
    phases.keys().forall(pid =>
      match phases.get(pid) {
        | WaitingCommitments => countFilled(commitments, pid) = 0
        | WaitingReveals =>
            countFilled(commitments, pid) >= thresholds.get(pid)
        | Aggregating =>
            countFilled(commitments, pid) >= thresholds.get(pid) and
            countFilled(reveals, pid) >= thresholds.get(pid)
        | Finished =>
            match derivedKey.get(pid) {
              | Some(_) =>
                  match transcriptHash.get(pid) {
                    | Some(_) =>
                        countFilled(reveals, pid) >= thresholds.get(pid)
                    | None => false
                  }
              | None => false
            }
        | Aborted => true
      }
    )

  val InvariantDerivedKeyOnlyFinished =
    derivedKey.keys().forall(pid =>
      derivedKey.get(pid) = None or phases.get(pid) = Finished
    )
}
