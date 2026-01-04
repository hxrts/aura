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

  type InviteStatus = Pending | Accepted | Declined

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

  // ============ STATE ============
  var channels: ChannelId -> ChannelRecord
  // invitations[channel][receiver] = status
  var invitations: ChannelId -> (AgentId -> InviteStatus)
  var messages: MessageId -> MessageRecord
  var deliveries: MessageId -> Set[AgentId]

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

  // ============ INIT ============
  action init = all {
    channels' = Map(),
    invitations' = Map(),
    messages' = Map(),
    deliveries' = Map(),
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
      actor' = leaver,
      channel' = cid,
      member' = leaver,
      message' = ""
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
}
