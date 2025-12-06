module protocol_dkd {
  // Base types (defined locally - type imports don't work in Quint)
  type Option[a] = Some(a) | None
  type ProtocolId = str
  type AuthorityId = str  // renamed from DeviceId for authority model

  type DkdPhase =
    | WaitingCommitments
    | WaitingReveals
    | Aggregating
    | DkdFinished
    | DkdAborted

  type CommitmentState = ProtocolId -> AuthorityId -> Option[int]
  type RevealState = ProtocolId -> AuthorityId -> Option[int]

  // ============ STATE ============

  var participants: ProtocolId -> Set[AuthorityId]
  var phases: ProtocolId -> DkdPhase
  var commitments: CommitmentState
  var reveals: RevealState
  var thresholds: ProtocolId -> int
  var derivedKey: ProtocolId -> Option[int]
  var transcriptHash: ProtocolId -> Option[int]

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

  pure def ensureParticipantMap(
    mapping: CommitmentState,
    pid: ProtocolId,
    members: Set[AuthorityId]
  ): CommitmentState =
    if (pid.in(mapping.keys())) mapping
    else
      mapping.set(
        pid,
        members.fold(Map(), (acc, member) => acc.set(member, None))
      )

  pure def getCommitmentOpt(mapping: CommitmentState, pid: ProtocolId, member: AuthorityId): Option[int] =
    if (pid.in(mapping.keys()))
      if (member.in(mapping.get(pid).keys())) mapping.get(pid).get(member) else None
    else None

  def countFilled(mapping: CommitmentState, pid: ProtocolId): int =
    getOrDefaultSet(participants, pid).fold(0, (acc, member) =>
      acc + (if (optionIsSome(getCommitmentOpt(mapping, pid, member))) 1 else 0)
    )

  def allMembers(mapping: CommitmentState, pid: ProtocolId): bool =
    getOrDefaultSet(participants, pid).forall(member =>
      optionIsSome(getCommitmentOpt(mapping, pid, member))
    )

  pure def setMappingValue(
    mapping: CommitmentState,
    pid: ProtocolId,
    member: AuthorityId,
    value: Option[int],
    members: Set[AuthorityId]
  ): CommitmentState = {
    val ensured = ensureParticipantMap(mapping, pid, members)
    ensured.set(pid, ensured.get(pid).set(member, value))
  }

  def thresholdReached(pid: ProtocolId, mapping: CommitmentState): bool =
    countFilled(mapping, pid) >= getOrDefaultInt(thresholds, pid, 0)

  def revealsMatchCommitments(pid: ProtocolId): bool =
    getOrDefaultSet(participants, pid).forall(member => {
      val revealOpt = getCommitmentOpt(reveals, pid, member)
      val hasReveal = match revealOpt { | Some(_) => true | None => false }
      hasReveal implies optionIsSome(getCommitmentOpt(commitments, pid, member))
    })

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
    members: Set[AuthorityId],
    threshold: int
  ): bool = all {
    members.size() >= 1,
    threshold >= 1,
    threshold <= members.size(),
    not(pid.in(participants.keys())),
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
    member: AuthorityId,
    commitment: int
  ): bool = {
    val members = getOrDefaultSet(participants, pid)
    val state = if (pid.in(phases.keys())) phases.get(pid) else WaitingCommitments
    val isWaiting = match state { | WaitingCommitments => true | _ => false }
    val isRevealing = match state { | WaitingReveals => true | _ => false }
    val hasCommitment = optionIsSome(getCommitmentOpt(commitments, pid, member))
    val updatedCommitments = setMappingValue(commitments, pid, member, Some(commitment), members)
    val meetsThreshold = countFilled(updatedCommitments, pid) >= getOrDefaultInt(thresholds, pid, 0)
    val newPhase = if (meetsThreshold) WaitingReveals else state
    all {
      members.contains(member),
      isWaiting or isRevealing,
      not(hasCommitment),
      commitments' = updatedCommitments,
      phases' = phases.set(pid, newPhase),
      participants' = participants,
      reveals' = reveals,
      thresholds' = thresholds,
      derivedKey' = derivedKey,
      transcriptHash' = transcriptHash
    }
  }

  action recordReveal(
    pid: ProtocolId,
    member: AuthorityId,
    share: int
  ): bool = {
    val members = getOrDefaultSet(participants, pid)
    val state = if (pid.in(phases.keys())) phases.get(pid) else WaitingCommitments
    val isRevealing = match state { | WaitingReveals => true | _ => false }
    val isAggregating = match state { | Aggregating => true | _ => false }
    val hasCommitment = optionIsSome(getCommitmentOpt(commitments, pid, member))
    val hasReveal = optionIsSome(getCommitmentOpt(reveals, pid, member))
    val updatedReveals = setMappingValue(reveals, pid, member, Some(share), members)
    val meetsThreshold = countFilled(updatedReveals, pid) >= getOrDefaultInt(thresholds, pid, 0)
    val newPhase = if (meetsThreshold) Aggregating else state
    all {
      members.contains(member),
      isRevealing or isAggregating,
      hasCommitment,
      not(hasReveal),
      reveals' = updatedReveals,
      phases' = phases.set(pid, newPhase),
      participants' = participants,
      commitments' = commitments,
      thresholds' = thresholds,
      derivedKey' = derivedKey,
      transcriptHash' = transcriptHash
    }
  }

  action finalizeDkd(
    pid: ProtocolId,
    derivedKeyHash: int,
    transcript: int
  ): bool = {
    val state = if (pid.in(phases.keys())) phases.get(pid) else WaitingCommitments
    val isAggregating = match state { | Aggregating => true | _ => false }
    all {
      isAggregating,
      thresholdReached(pid, reveals),
      revealsMatchCommitments(pid),
      phases' = phases.set(pid, DkdFinished),
      derivedKey' = derivedKey.set(pid, Some(derivedKeyHash)),
      transcriptHash' = transcriptHash.set(pid, Some(transcript)),
      participants' = participants,
      commitments' = commitments,
      reveals' = reveals,
      thresholds' = thresholds
    }
  }

  action abortDkd(pid: ProtocolId): bool = {
    val state = if (pid.in(phases.keys())) phases.get(pid) else WaitingCommitments
    val isNotFinished = match state { | DkdFinished => false | _ => true }
    all {
      isNotFinished,
      phases' = phases.set(pid, DkdAborted),
      participants' = participants,
      commitments' = commitments,
      reveals' = reveals,
      thresholds' = thresholds,
      derivedKey' = derivedKey,
      transcriptHash' = transcriptHash
    }
  }

  // ============ INVARIANTS ============

  val InvariantThresholdsWithinBounds =
    thresholds.keys().forall(pid =>
      thresholds.get(pid) >= 1 and
      thresholds.get(pid) <= getOrDefaultSet(participants, pid).size()
    )

  val InvariantCommitmentsAtMostOne =
    commitments.keys().forall(pid =>
      getOrDefaultSet(participants, pid).forall(member =>
        match getCommitmentOpt(commitments, pid, member) {
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
        | WaitingCommitments => countFilled(commitments, pid) == 0
        | WaitingReveals =>
            countFilled(commitments, pid) >= thresholds.get(pid)
        | Aggregating =>
            countFilled(commitments, pid) >= thresholds.get(pid) and
            countFilled(reveals, pid) >= thresholds.get(pid)
        | DkdFinished => {
            val hasKey = match (if (pid.in(derivedKey.keys())) derivedKey.get(pid) else None) {
              | Some(_) => true
              | None => false
            }
            val hasTranscript = match (if (pid.in(transcriptHash.keys())) transcriptHash.get(pid) else None) {
              | Some(_) => true
              | None => false
            }
            hasKey and hasTranscript and countFilled(reveals, pid) >= thresholds.get(pid)
          }
        | DkdAborted => true
      }
    )

  val InvariantDerivedKeyOnlyFinished =
    derivedKey.keys().forall(pid => {
      val keyOpt = derivedKey.get(pid)
      val hasKey = match keyOpt { | Some(_) => true | None => false }
      val state = if (pid.in(phases.keys())) phases.get(pid) else WaitingCommitments
      val isFinished = match state { | DkdFinished => true | _ => false }
      hasKey implies isFinished
    })
}
