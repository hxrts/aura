// AMP Channel Lifecycle Specification
//
// Models the AMP channel lifecycle with invitations, membership, messaging,
// and key rotation on leave. Intended for ITF trace generation + simulator replay.

module amp_channel {
  // ============ TYPES ============
  type Option[a] = Some(a) | None
  type AgentId = str
  type ChannelId = str
  type MessageId = str
  type TransitionId = str

  type InviteStatus = Pending | Accepted | Declined
  type TransitionPolicy =
    NormalTransition
    | AdditiveTransition
    | SubtractiveTransition
    | EmergencyQuarantineTransition
    | EmergencyCryptoshredTransition
  type TransitionStatus =
    Observed
    | A2Live
    | A2Conflict
    | A3Finalized
    | A3Conflict
    | Aborted
    | Superseded

  type ChannelRecord = {
    id: ChannelId,
    creator: AgentId,
    members: Set[AgentId],
    epoch: int
  }

  type MessageRecord = {
    id: MessageId,
    channel: ChannelId,
    sender: AgentId
  }

  type TransitionRecord = {
    id: TransitionId,
    channel: ChannelId,
    parentEpoch: int,
    successorEpoch: int,
    parentCommitment: str,
    successorCommitment: str,
    membershipCommitment: str,
    policy: TransitionPolicy,
    status: TransitionStatus,
    suspect: Option[AgentId],
    readableStateDestroyed: bool,
    supersededBy: Option[TransitionId]
  }

  // ============ STATE ============
  var channels: ChannelId -> ChannelRecord
  // invitations[channel][receiver] = status
  var invitations: ChannelId -> (AgentId -> InviteStatus)
  var messages: MessageId -> MessageRecord
  var deliveries: MessageId -> Set[AgentId]
  var transitions: TransitionId -> TransitionRecord

  // Action parameter fields (captured in ITF traces for replay)
  var actor: AgentId
  var channel: ChannelId
  var member: AgentId
  var message: MessageId

  // ============ HELPERS ============
  def channel_or_default(cid: ChannelId): ChannelRecord =
    if (cid.in(channels.keys())) channels.get(cid)
    else { id: cid, creator: "", members: Set(), epoch: 0 }

  def invite_map(cid: ChannelId): AgentId -> InviteStatus =
    if (cid.in(invitations.keys())) invitations.get(cid) else Map()

  def invite_status(cid: ChannelId, receiver: AgentId): Option[InviteStatus] =
    if (receiver.in(invite_map(cid).keys())) Some(invite_map(cid).get(receiver)) else None

  def deliveries_for(mid: MessageId): Set[AgentId] =
    if (mid.in(deliveries.keys())) deliveries.get(mid) else Set()

  def transition_or_default(tid: TransitionId): TransitionRecord =
    if (tid.in(transitions.keys())) transitions.get(tid)
    else {
      id: tid,
      channel: "",
      parentEpoch: 0,
      successorEpoch: 0,
      parentCommitment: "",
      successorCommitment: "",
      membershipCommitment: "",
      policy: NormalTransition,
      status: Observed,
      suspect: None,
      readableStateDestroyed: false,
      supersededBy: None
    }

  def same_parent(left: TransitionRecord, right: TransitionRecord): bool =
    left.channel == right.channel
      and left.parentEpoch == right.parentEpoch
      and left.parentCommitment == right.parentCommitment

  def live_successors_for(candidate: TransitionRecord): Set[TransitionId] =
    transitions.keys().filter(tid => {
      val transition = transitions.get(tid)
      same_parent(candidate, transition) and transition.status == A2Live
    })

  def finalized_successors_for(candidate: TransitionRecord): Set[TransitionId] =
    transitions.keys().filter(tid => {
      val transition = transitions.get(tid)
      same_parent(candidate, transition) and transition.status == A3Finalized
    })

  def active_members(cid: ChannelId): Set[AgentId] =
    channel_or_default(cid).members

  // ============ INIT ============
  action init = all {
    channels' = Map(),
    invitations' = Map(),
    messages' = Map(),
    deliveries' = Map(),
    transitions' = Map(),
    actor' = "",
    channel' = "",
    member' = "",
    message' = ""
  }

  // ============ ACTIONS ============
  action createChannel(creator: AgentId, cid: ChannelId): bool = all {
    not(cid.in(channels.keys())),
    channels' = channels.put(cid, {
      id: cid,
      creator: creator,
      members: Set(creator),
      epoch: 0
    }),
    invitations' = invitations,
    messages' = messages,
    deliveries' = deliveries,
    transitions' = transitions,
    actor' = creator,
    channel' = cid,
    member' = "",
    message' = ""
  }

  action inviteMember(sender: AgentId, cid: ChannelId, receiver: AgentId): bool = all {
    cid.in(channels.keys()),
    channel_or_default(cid).members.contains(sender),
    invitations' = invitations.put(
      cid,
      invite_map(cid).put(receiver, Pending)
    ),
    channels' = channels,
    messages' = messages,
    deliveries' = deliveries,
    transitions' = transitions,
    actor' = sender,
    channel' = cid,
    member' = receiver,
    message' = ""
  }

  action acceptInvite(receiver: AgentId, cid: ChannelId): bool = all {
    cid.in(channels.keys()),
    invite_status(cid, receiver) == Some(Pending),
    invitations' = invitations.put(
      cid,
      invite_map(cid).put(receiver, Accepted)
    ),
    channels' = channels,
    messages' = messages,
    deliveries' = deliveries,
    transitions' = transitions,
    actor' = receiver,
    channel' = cid,
    member' = receiver,
    message' = ""
  }

  action joinChannel(participant: AgentId, cid: ChannelId): bool = {
    val chan = channel_or_default(cid)
    all {
      cid.in(channels.keys()),
      invite_status(cid, participant) == Some(Accepted),
      not(chan.members.contains(participant)),
      channels' = channels.put(cid, {
        ...chan,
        members: chan.members.union(Set(participant))
      }),
      invitations' = invitations,
      messages' = messages,
      deliveries' = deliveries,
      transitions' = transitions,
      actor' = participant,
      channel' = cid,
      member' = participant,
      message' = ""
    }
  }

  action sendMessage(sender: AgentId, cid: ChannelId, mid: MessageId): bool = {
    val chan = channel_or_default(cid)
    all {
      cid.in(channels.keys()),
      chan.members.contains(sender),
      not(mid.in(messages.keys())),
      messages' = messages.put(mid, { id: mid, channel: cid, sender: sender }),
      deliveries' = deliveries,
      channels' = channels,
      invitations' = invitations,
      transitions' = transitions,
      actor' = sender,
      channel' = cid,
      member' = "",
      message' = mid
    }
  }

  action receiveMessage(receiver: AgentId, cid: ChannelId, mid: MessageId): bool = {
    val chan = channel_or_default(cid)
    val msg = if (mid.in(messages.keys())) messages.get(mid) else { id: mid, channel: "", sender: "" }
    all {
      mid.in(messages.keys()),
      msg.channel == cid,
      chan.members.contains(receiver),
      deliveries' = deliveries.put(mid, deliveries_for(mid).union(Set(receiver))),
      channels' = channels,
      invitations' = invitations,
      messages' = messages,
      transitions' = transitions,
      actor' = receiver,
      channel' = cid,
      member' = receiver,
      message' = mid
    }
  }

  action leaveChannel(leaver: AgentId, cid: ChannelId): bool = {
    val chan = channel_or_default(cid)
    all {
      cid.in(channels.keys()),
      chan.members.contains(leaver),
      channels' = channels.put(cid, {
        ...chan,
        members: chan.members.exclude(Set(leaver)),
        epoch: chan.epoch + 1
      }),
      invitations' = invitations,
      messages' = messages,
      deliveries' = deliveries,
      transitions' = transitions,
      actor' = leaver,
      channel' = cid,
      member' = leaver,
      message' = ""
    }
  }

  action proposeTransition(
    coordinator: AgentId,
    cid: ChannelId,
    tid: TransitionId,
    policy: TransitionPolicy
  ): bool = {
    val chan = channel_or_default(cid)
    all {
      cid.in(channels.keys()),
      chan.members.contains(coordinator),
      not(tid.in(transitions.keys())),
      transitions' = transitions.put(tid, {
        id: tid,
        channel: cid,
        parentEpoch: chan.epoch,
        successorEpoch: chan.epoch + 1,
        parentCommitment: "parent",
        successorCommitment: tid,
        membershipCommitment: "members",
        policy: policy,
        status: Observed,
        suspect: None,
        readableStateDestroyed: false,
        supersededBy: None
      }),
      channels' = channels,
      invitations' = invitations,
      messages' = messages,
      deliveries' = deliveries,
      actor' = coordinator,
      channel' = cid,
      member' = "",
      message' = tid
    }
  }

  action certifyTransition(tid: TransitionId): bool = {
    val transition = transition_or_default(tid)
    val noOtherLive = live_successors_for(transition).filter(other => other != tid).size() == 0
    all {
      tid.in(transitions.keys()),
      transition.status == Observed,
      noOtherLive,
      transitions' = transitions.put(tid, { ...transition, status: A2Live }),
      channels' = channels,
      invitations' = invitations,
      messages' = messages,
      deliveries' = deliveries,
      actor' = actor,
      channel' = transition.channel,
      member' = "",
      message' = tid
    }
  }

  action reportTransitionConflict(leftId: TransitionId, rightId: TransitionId): bool = {
    val left = transition_or_default(leftId)
    val right = transition_or_default(rightId)
    all {
      leftId.in(transitions.keys()),
      rightId.in(transitions.keys()),
      leftId != rightId,
      same_parent(left, right),
      transitions' = transitions
        .put(leftId, { ...left, status: if (left.status == A3Finalized) A3Conflict else A2Conflict })
        .put(rightId, { ...right, status: if (right.status == A3Finalized) A3Conflict else A2Conflict }),
      channels' = channels,
      invitations' = invitations,
      messages' = messages,
      deliveries' = deliveries,
      actor' = actor,
      channel' = left.channel,
      member' = "",
      message' = leftId
    }
  }

  action finalizeTransition(tid: TransitionId): bool = {
    val transition = transition_or_default(tid)
    val chan = channel_or_default(transition.channel)
    val noOtherFinalized = finalized_successors_for(transition).filter(other => other != tid).size() == 0
    all {
      tid.in(transitions.keys()),
      transition.status == A2Live,
      noOtherFinalized,
      transitions' = transitions.put(tid, { ...transition, status: A3Finalized }),
      channels' = channels.put(transition.channel, {
        ...chan,
        epoch: transition.successorEpoch,
        members: if (transition.policy == SubtractiveTransition) chan.members.exclude(Set(member)) else chan.members
      }),
      invitations' = invitations,
      messages' = messages,
      deliveries' = deliveries,
      actor' = actor,
      channel' = transition.channel,
      member' = member,
      message' = tid
    }
  }

  action abortTransition(tid: TransitionId): bool = {
    val transition = transition_or_default(tid)
    all {
      tid.in(transitions.keys()),
      transition.status != A3Finalized,
      transitions' = transitions.put(tid, { ...transition, status: Aborted }),
      channels' = channels,
      invitations' = invitations,
      messages' = messages,
      deliveries' = deliveries,
      actor' = actor,
      channel' = transition.channel,
      member' = "",
      message' = tid
    }
  }

  action supersedeTransition(oldId: TransitionId, newId: TransitionId): bool = {
    val oldTransition = transition_or_default(oldId)
    all {
      oldId.in(transitions.keys()),
      newId.in(transitions.keys()),
      oldId != newId,
      transitions' = transitions.put(oldId, {
        ...oldTransition,
        status: Superseded,
        supersededBy: Some(newId)
      }),
      channels' = channels,
      invitations' = invitations,
      messages' = messages,
      deliveries' = deliveries,
      actor' = actor,
      channel' = oldTransition.channel,
      member' = "",
      message' = oldId
    }
  }

  action emergencyQuarantine(
    coordinator: AgentId,
    cid: ChannelId,
    tid: TransitionId,
    suspect: AgentId
  ): bool = {
    val chan = channel_or_default(cid)
    all {
      cid.in(channels.keys()),
      chan.members.contains(coordinator),
      not(tid.in(transitions.keys())),
      transitions' = transitions.put(tid, {
        id: tid,
        channel: cid,
        parentEpoch: chan.epoch,
        successorEpoch: chan.epoch + 1,
        parentCommitment: "parent",
        successorCommitment: tid,
        membershipCommitment: "members",
        policy: EmergencyQuarantineTransition,
        status: Observed,
        suspect: Some(suspect),
        readableStateDestroyed: false,
        supersededBy: None
      }),
      channels' = channels,
      invitations' = invitations,
      messages' = messages,
      deliveries' = deliveries,
      actor' = coordinator,
      channel' = cid,
      member' = suspect,
      message' = tid
    }
  }

  action emergencyCryptoshred(
    coordinator: AgentId,
    cid: ChannelId,
    tid: TransitionId,
    suspect: AgentId
  ): bool = {
    val chan = channel_or_default(cid)
    all {
      cid.in(channels.keys()),
      chan.members.contains(coordinator),
      not(tid.in(transitions.keys())),
      transitions' = transitions.put(tid, {
        id: tid,
        channel: cid,
        parentEpoch: chan.epoch,
        successorEpoch: chan.epoch + 1,
        parentCommitment: "parent",
        successorCommitment: tid,
        membershipCommitment: "members",
        policy: EmergencyCryptoshredTransition,
        status: Observed,
        suspect: Some(suspect),
        readableStateDestroyed: true,
        supersededBy: None
      }),
      channels' = channels,
      invitations' = invitations,
      messages' = messages,
      deliveries' = deliveries,
      actor' = coordinator,
      channel' = cid,
      member' = suspect,
      message' = tid
    }
  }

  // ============ INVARIANTS ============

  // Members must be unique (set invariant is implicit), and creator is always a member.
  val InvariantCreatorMember =
    channels.keys().forall(cid => channels.get(cid).members.contains(channels.get(cid).creator))

  // Delivered messages are only to members of the channel at receive time.
  val InvariantDeliveriesHaveMessages =
    deliveries.keys().forall(mid => mid.in(messages.keys()))

  val InvariantDeliveriesToMembers =
    deliveries.keys().forall(mid => {
      val msg = messages.get(mid)
      deliveries.get(mid).forall(m => channels.get(msg.channel).members.contains(m))
    })

  val InvariantDeterministicTransitionConvergence =
    transitions.keys().forall(tid => {
      val transition = transitions.get(tid)
      transition.id == tid and transition.successorEpoch == transition.parentEpoch + 1
    })

  val InvariantSingleLiveSuccessor =
    transitions.keys().forall(tid => {
      val transition = transitions.get(tid)
      live_successors_for(transition).size() <= 1
    })

  val InvariantConflictSuppression =
    transitions.keys().forall(tid => {
      val transition = transitions.get(tid)
      (transition.status == A2Conflict or transition.status == A3Conflict)
        implies not(transition.id.in(live_successors_for(transition)))
    })

  val InvariantEpochLocalMembershipTruth =
    transitions.keys().forall(tid => {
      val transition = transitions.get(tid)
      transition.status == A2Live implies transition.parentEpoch == channel_or_default(transition.channel).epoch
    })

  val InvariantEmergencyNonRetroactivity =
    transitions.keys().forall(tid => {
      val transition = transitions.get(tid)
      transition.policy == EmergencyCryptoshredTransition implies transition.readableStateDestroyed
    })

  temporal TemporalA2LiveCanFinalizeWithoutSecondLive =
    always(transitions.keys().forall(tid => {
      val transition = transitions.get(tid)
      transition.status == A2Live implies eventually(
        transitions.get(tid).status == A3Finalized
          or transitions.get(tid).status == A2Conflict
          or transitions.get(tid).status == Aborted
          or transitions.get(tid).status == Superseded
      )
    }))
}
