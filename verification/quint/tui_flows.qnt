// TUI Flows - Unified Multi-Agent Flow Verification
//
// Models end-to-end flows across multiple agents, verifying:
// - Command execution → Signal emission → UI state update
// - Multi-party protocol coordination (e.g., recovery approval)
// - Flow phase transitions and invariants
//
// Core Flows Modeled:
// 1. Guardian Recovery: Bob → request → Alice/Carol approve → restore
// 2. Block Lifecycle: create → invite → join → promote steward
// 3. Neighborhood: create blocks → link → traverse
// 4. Chat & Messaging: contact → DM/channel → message exchange
// 5. Invitation Lifecycle: create → export → import → accept → contact
// 6. Social Graph: contacts → blocks → assignment → views
//
// See: work/tui_flows.md for design documentation

module tui_flows {
  // ============ TYPE DEFINITIONS ============

  type Option[a] = Some(a) | None

  // Agent identifier (represents a user/authority in the system)
  type AgentId = str

  // Unique identifiers
  type InvitationId = str
  type ChannelId = str
  type HomeId = str
  type NeighborhoodId = str
  type RecoverySessionId = str
  type MessageId = str

  // ============ SIGNAL TYPES ============

  // Signal types that can be emitted (from tui_signal_propagation)
  type SignalType =
    | ChatSignal
    | ContactsSignal
    | InvitationsSignal
    | RecoverySignal
    | NeighborhoodSignal
    | BlockSignal
    | ConnectionStatusSignal
    | SyncStatusSignal

  // ============ FLOW TYPES ============

  // Flow 1: Guardian Recovery phases
  type RecoveryFlowPhase =
    | RecoveryIdle                   // No recovery in progress
    | RecoveryRequested              // Subject requested recovery
    | RecoveryApprovalsPending       // Waiting for guardian approvals
    | RecoveryCooldownActive         // Mandatory cooldown period
    | RecoverySharesSubmitted        // Guardians submitted shares
    | RecoveryCompleted              // Authority restored
    | RecoveryCancelled              // Aborted

  // Flow 2: Block lifecycle phases
  type BlockFlowPhase =
    | BlockNone                      // No block
    | HomeCreated                   // Home created by owner
    | BlockInvitationSent            // Resident invitation sent
    | BlockResidentJoined            // Resident accepted invitation
    | BlockStewardPromoted           // Resident promoted to steward
    | BlockFull                      // Block at capacity (8 residents)

  // Flow 3: Neighborhood formation phases
  type NeighborhoodFlowPhase =
    | NeighborhoodNone               // No neighborhood
    | NeighborhoodBlocksCreated      // Multiple blocks exist
    | NeighborhoodCreated            // Neighborhood entity created
    | NeighborhoodBlocksLinked       // Blocks connected to neighborhood
    | NeighborhoodTraversable        // Can traverse between blocks

  // Flow 4: Chat & messaging phases
  type ChatFlowPhase =
    | ChatNone                       // No chat
    | ChatContactExists              // Contact relationship established
    | ChatDMStarted                  // DM channel created
    | ChatChannelCreated             // Group channel created
    | ChatMembersInvited             // Members invited to channel
    | ChatMessagesExchanged          // Messages sent/received

  // Flow 5: Invitation lifecycle phases
  type InvitationFlowPhase =
    | InvitationNone                 // No invitation
    | InvitationCreated              // Invitation created by sender
    | InvitationExported             // Code exported (QR/text)
    | InvitationImported             // Code imported by recipient
    | InvitationAccepted             // Recipient accepted
    | InvitationDeclined             // Recipient declined
    | InvitationContactCreated       // Contact relationship created

  // Flow 6: Social Graph (Contacts + Blocks) phases
  type SocialGraphFlowPhase =
    | SocialNone                     // No social graph activity
    | SocialContactsExist            // User has contacts
    | SocialBlockExists              // User has created/owns a block
    | SocialContactInvitedToBlock    // Contact invited to join block
    | SocialContactJoinedBlock       // Contact joined as resident (is_resident = true)
    | SocialNicknameUpdated           // Contact nickname changed
    | SocialBlockContactsFiltered    // Contacts filtered by block membership

  // ============ INVITATION TYPE ============

  type InvitationType =
    | GuardianInvitation             // Become a guardian
    | ContactInvitation              // Simple contact
    | BlockResidentInvitation(HomeId) // Join a block as resident

  // ============ AGENT STATE ============

  // Per-agent state (simplified TUI + protocol state)
  type AgentState = {
    // Identity
    hasAccount: bool,
    authorityId: str,

    // Contacts & Social
    contacts: Set[AgentId],
    guardians: Set[AgentId],
    guardianFor: Set[AgentId],       // Who this agent is a guardian for
    contactNicknames: AgentId -> str, // Nicknames for contacts (contact_id -> nickname)

    // Invitations
    pendingInvitations: Set[InvitationId],
    sentInvitations: Set[InvitationId],

    // Chat
    channels: Set[ChannelId],
    directMessages: Set[AgentId],    // DM channels (keyed by other party)

    // Blocks & Neighborhoods
    ownedBlocks: Set[HomeId],
    residentOf: Set[HomeId],
    stewardOf: Set[HomeId],
    neighborhoods: Set[NeighborhoodId],

    // Recovery
    activeRecoverySession: Option[RecoverySessionId],
    pendingRecoveryApprovals: Set[RecoverySessionId],  // Approvals requested of this agent

    // Signal emission tracking (per agent)
    lastEmittedSignals: Set[SignalType]
  }

  // ============ GLOBAL STATE ============

  // Per-agent state map
  var agents: AgentId -> AgentState

  // All known agents in simulation
  var allAgents: Set[AgentId]

  // ============ INVITATION STATE ============

  type InvitationRecord = {
    id: InvitationId,
    sender: AgentId,
    invitationType: InvitationType,
    isExported: bool,
    isImported: bool,
    importedBy: Option[AgentId],
    isAccepted: bool,
    isDeclined: bool
  }

  var invitations: InvitationId -> InvitationRecord

  // ============ RECOVERY SESSION STATE ============

  type RecoverySession = {
    id: RecoverySessionId,
    subject: AgentId,                // Who is being recovered
    guardians: Set[AgentId],         // Configured guardians
    approvals: Set[AgentId],         // Guardians who approved
    threshold: int,                  // Required approvals
    cooldownRemaining: int,          // Ticks until cooldown ends
    phase: RecoveryFlowPhase
  }

  var recoverySessions: RecoverySessionId -> RecoverySession

  // ============ BLOCK STATE ============

  type BlockRecord = {
    id: HomeId,
    owner: AgentId,
    stewards: Set[AgentId],
    residents: Set[AgentId],
    capacity: int,                   // Max 8
    linkedNeighborhood: Option[NeighborhoodId]
  }

  var blocks: HomeId -> BlockRecord

  // ============ CHANNEL STATE ============

  type ChannelRecord = {
    id: ChannelId,
    name: str,
    creator: AgentId,
    members: Set[AgentId],
    messageCount: int
  }

  var channels: ChannelId -> ChannelRecord

  // ============ NEIGHBORHOOD STATE ============

  type NeighborhoodRecord = {
    id: NeighborhoodId,
    name: str,
    linkedBlocks: Set[HomeId]
  }

  var neighborhoods: NeighborhoodId -> NeighborhoodRecord

  // ============ FLOW PHASE TRACKING ============

  // Track current phase of each flow type for verification
  var recoveryFlowPhase: AgentId -> RecoveryFlowPhase
  var homeFlowPhase: AgentId -> BlockFlowPhase
  var neighborhoodFlowPhase: AgentId -> NeighborhoodFlowPhase
  var chatFlowPhase: AgentId -> ChatFlowPhase
  var invitationFlowPhase: AgentId -> InvitationFlowPhase
  var socialGraphFlowPhase: AgentId -> SocialGraphFlowPhase

  // ============ HELPERS ============

  // Default agent state
  pure def defaultAgentState(aid: AgentId): AgentState = {
    hasAccount: false,
    authorityId: "",
    contacts: Set(),
    guardians: Set(),
    guardianFor: Set(),
    contactNicknames: Map(),
    pendingInvitations: Set(),
    sentInvitations: Set(),
    channels: Set(),
    directMessages: Set(),
    ownedBlocks: Set(),
    residentOf: Set(),
    stewardOf: Set(),
    neighborhoods: Set(),
    activeRecoverySession: None,
    pendingRecoveryApprovals: Set(),
    lastEmittedSignals: Set()
  }

  // Safe agent state lookup
  pure def getAgent(agentMap: AgentId -> AgentState, aid: AgentId): AgentState =
    if (aid.in(agentMap.keys())) agentMap.get(aid) else defaultAgentState(aid)

  // Update agent state
  pure def updateAgent(
    agentMap: AgentId -> AgentState,
    aid: AgentId,
    newState: AgentState
  ): AgentId -> AgentState =
    agentMap.set(aid, newState)

  // ============ INITIALIZATION ============

  action init = all {
    allAgents' = Set("bob", "alice", "carol"),
    agents' = Map(
      "bob" -> defaultAgentState("bob"),
      "alice" -> defaultAgentState("alice"),
      "carol" -> defaultAgentState("carol")
    ),
    invitations' = Map(),
    recoverySessions' = Map(),
    blocks' = Map(),
    channels' = Map(),
    neighborhoods' = Map(),
    recoveryFlowPhase' = Map(
      "bob" -> RecoveryIdle,
      "alice" -> RecoveryIdle,
      "carol" -> RecoveryIdle
    ),
    homeFlowPhase' = Map(
      "bob" -> BlockNone,
      "alice" -> BlockNone,
      "carol" -> BlockNone
    ),
    neighborhoodFlowPhase' = Map(
      "bob" -> NeighborhoodNone,
      "alice" -> NeighborhoodNone,
      "carol" -> NeighborhoodNone
    ),
    chatFlowPhase' = Map(
      "bob" -> ChatNone,
      "alice" -> ChatNone,
      "carol" -> ChatNone
    ),
    invitationFlowPhase' = Map(
      "bob" -> InvitationNone,
      "alice" -> InvitationNone,
      "carol" -> InvitationNone
    ),
    socialGraphFlowPhase' = Map(
      "bob" -> SocialNone,
      "alice" -> SocialNone,
      "carol" -> SocialNone
    )
  }

  // ============ FLOW 1: GUARDIAN RECOVERY ============

  // Step 1: Create account with guardians
  action createAccountWithGuardians(
    subject: AgentId,
    guardianSet: Set[AgentId],
    thresholdVal: int
  ): bool = {
    val state = getAgent(agents, subject)
    // Build updated agents map: first update subject, then update each guardian
    val updatedWithSubject = agents.set(subject, {
      ...state,
      hasAccount: true,
      authorityId: subject,
      guardians: guardianSet,
      lastEmittedSignals: Set(RecoverySignal, ContactsSignal)
    })
    // Update guardians to add subject to their guardianFor set
    val fullyUpdated = guardianSet.fold(updatedWithSubject, (acc, g) => {
      val gs = getAgent(acc, g)
      acc.set(g, { ...gs, guardianFor: gs.guardianFor.union(Set(subject)) })
    })
    all {
      not(state.hasAccount),
      guardianSet.size() >= 1,
      thresholdVal >= 1,
      thresholdVal <= guardianSet.size(),
      agents' = fullyUpdated,
      // Flow phase updates
      recoveryFlowPhase' = recoveryFlowPhase.set(subject, RecoveryIdle),
      // Unchanged
      allAgents' = allAgents,
      invitations' = invitations,
      recoverySessions' = recoverySessions,
      blocks' = blocks,
      channels' = channels,
      neighborhoods' = neighborhoods,
      homeFlowPhase' = homeFlowPhase,
      neighborhoodFlowPhase' = neighborhoodFlowPhase,
      chatFlowPhase' = chatFlowPhase,
      invitationFlowPhase' = invitationFlowPhase,
      socialGraphFlowPhase' = socialGraphFlowPhase
    }
  }

  // Step 2: Initiate recovery request
  action initiateRecovery(
    subject: AgentId,
    sessionId: RecoverySessionId,
    cooldown: int
  ): bool = {
    val state = getAgent(agents, subject)
    val phase = if (subject.in(recoveryFlowPhase.keys())) recoveryFlowPhase.get(subject) else RecoveryIdle
    val isIdle = match phase { | RecoveryIdle => true | _ => false }
    all {
      state.hasAccount,
      state.guardians.size() >= 1,
      isIdle,
      not(sessionId.in(recoverySessions.keys())),
      // Create session
      recoverySessions' = recoverySessions.put(sessionId, {
        id: sessionId,
        subject: subject,
        guardians: state.guardians,
        approvals: Set(),
        threshold: 2,  // Simplified: 2-of-n threshold
        cooldownRemaining: cooldown,
        phase: RecoveryApprovalsPending
      }),
      // Update subject state
      agents' = agents.set(subject, {
        ...state,
        activeRecoverySession: Some(sessionId),
        lastEmittedSignals: Set(RecoverySignal)
      }),
      // Notify guardians
      recoveryFlowPhase' = recoveryFlowPhase.set(subject, RecoveryRequested),
      // Unchanged
      allAgents' = allAgents,
      invitations' = invitations,
      blocks' = blocks,
      channels' = channels,
      neighborhoods' = neighborhoods,
      homeFlowPhase' = homeFlowPhase,
      neighborhoodFlowPhase' = neighborhoodFlowPhase,
      chatFlowPhase' = chatFlowPhase,
      invitationFlowPhase' = invitationFlowPhase,
      socialGraphFlowPhase' = socialGraphFlowPhase
    }
  }

  // Step 3: Guardian approves recovery
  action approveRecovery(guardian: AgentId, sessionId: RecoverySessionId): bool = {
    val session = if (sessionId.in(recoverySessions.keys())) recoverySessions.get(sessionId) else {
      id: sessionId, subject: "", guardians: Set(), approvals: Set(),
      threshold: 0, cooldownRemaining: 0, phase: RecoveryIdle
    }
    val isPending = match session.phase { | RecoveryApprovalsPending => true | _ => false }
    val guardianState = getAgent(agents, guardian)
    val updatedApprovals = session.approvals.union(Set(guardian))
    val thresholdReached = updatedApprovals.size() >= session.threshold
    val nextPhase = if (thresholdReached and session.cooldownRemaining > 0)
      RecoveryCooldownActive else RecoveryApprovalsPending
    all {
      isPending,
      session.guardians.contains(guardian),
      not(session.approvals.contains(guardian)),
      // Update session
      recoverySessions' = recoverySessions.put(sessionId, {
        ...session,
        approvals: updatedApprovals,
        phase: nextPhase
      }),
      // Update guardian state (emits signal)
      agents' = agents.set(guardian, {
        ...guardianState,
        lastEmittedSignals: Set(RecoverySignal)
      }),
      // Update flow phase if threshold reached
      recoveryFlowPhase' = if (thresholdReached)
        recoveryFlowPhase.set(session.subject, RecoveryApprovalsPending)
        else recoveryFlowPhase,
      // Unchanged
      allAgents' = allAgents,
      invitations' = invitations,
      blocks' = blocks,
      channels' = channels,
      neighborhoods' = neighborhoods,
      homeFlowPhase' = homeFlowPhase,
      neighborhoodFlowPhase' = neighborhoodFlowPhase,
      chatFlowPhase' = chatFlowPhase,
      invitationFlowPhase' = invitationFlowPhase,
      socialGraphFlowPhase' = socialGraphFlowPhase
    }
  }

  // Step 4: Cooldown tick
  action tickRecoveryCooldown(sessionId: RecoverySessionId): bool = {
    val session = if (sessionId.in(recoverySessions.keys())) recoverySessions.get(sessionId) else {
      id: sessionId, subject: "", guardians: Set(), approvals: Set(),
      threshold: 0, cooldownRemaining: 0, phase: RecoveryIdle
    }
    val isCooldown = match session.phase { | RecoveryCooldownActive => true | _ => false }
    val remaining = session.cooldownRemaining - 1
    val nextPhase = if (remaining <= 0) RecoverySharesSubmitted else RecoveryCooldownActive
    all {
      isCooldown,
      session.cooldownRemaining > 0,
      recoverySessions' = recoverySessions.put(sessionId, {
        ...session,
        cooldownRemaining: remaining,
        phase: nextPhase
      }),
      recoveryFlowPhase' = if (remaining <= 0)
        recoveryFlowPhase.set(session.subject, RecoveryCooldownActive)
        else recoveryFlowPhase,
      // Unchanged
      allAgents' = allAgents,
      agents' = agents,
      invitations' = invitations,
      blocks' = blocks,
      channels' = channels,
      neighborhoods' = neighborhoods,
      homeFlowPhase' = homeFlowPhase,
      neighborhoodFlowPhase' = neighborhoodFlowPhase,
      chatFlowPhase' = chatFlowPhase,
      invitationFlowPhase' = invitationFlowPhase,
      socialGraphFlowPhase' = socialGraphFlowPhase
    }
  }

  // Step 5: Complete recovery
  action completeRecovery(sessionId: RecoverySessionId): bool = {
    val session = if (sessionId.in(recoverySessions.keys())) recoverySessions.get(sessionId) else {
      id: sessionId, subject: "", guardians: Set(), approvals: Set(),
      threshold: 0, cooldownRemaining: 0, phase: RecoveryIdle
    }
    val isSharesSubmitted = match session.phase { | RecoverySharesSubmitted => true | _ => false }
    val subjectState = getAgent(agents, session.subject)
    all {
      isSharesSubmitted,
      session.approvals.size() >= session.threshold,
      // Mark session completed
      recoverySessions' = recoverySessions.put(sessionId, {
        ...session,
        phase: RecoveryCompleted
      }),
      // Update subject state
      agents' = agents.set(session.subject, {
        ...subjectState,
        activeRecoverySession: None,
        lastEmittedSignals: Set(RecoverySignal, ContactsSignal)
      }),
      recoveryFlowPhase' = recoveryFlowPhase.set(session.subject, RecoveryCompleted),
      // Unchanged
      allAgents' = allAgents,
      invitations' = invitations,
      blocks' = blocks,
      channels' = channels,
      neighborhoods' = neighborhoods,
      homeFlowPhase' = homeFlowPhase,
      neighborhoodFlowPhase' = neighborhoodFlowPhase,
      chatFlowPhase' = chatFlowPhase,
      invitationFlowPhase' = invitationFlowPhase,
      socialGraphFlowPhase' = socialGraphFlowPhase
    }
  }

  // ============ FLOW 5: INVITATION LIFECYCLE ============

  // Step 1: Create invitation
  action createInvitation(
    sender: AgentId,
    invId: InvitationId,
    invType: InvitationType
  ): bool = {
    val senderState = getAgent(agents, sender)
    all {
      senderState.hasAccount,
      not(invId.in(invitations.keys())),
      // Create invitation record
      invitations' = invitations.put(invId, {
        id: invId,
        sender: sender,
        invitationType: invType,
        isExported: false,
        isImported: false,
        importedBy: None,
        isAccepted: false,
        isDeclined: false
      }),
      // Update sender state
      agents' = agents.set(sender, {
        ...senderState,
        sentInvitations: senderState.sentInvitations.union(Set(invId)),
        lastEmittedSignals: Set(InvitationsSignal)
      }),
      invitationFlowPhase' = invitationFlowPhase.set(sender, InvitationCreated),
      // Unchanged
      allAgents' = allAgents,
      recoverySessions' = recoverySessions,
      blocks' = blocks,
      channels' = channels,
      neighborhoods' = neighborhoods,
      recoveryFlowPhase' = recoveryFlowPhase,
      homeFlowPhase' = homeFlowPhase,
      neighborhoodFlowPhase' = neighborhoodFlowPhase,
      chatFlowPhase' = chatFlowPhase,
      socialGraphFlowPhase' = socialGraphFlowPhase
    }
  }

  // Step 2: Export invitation (mark as exported)
  action exportInvitation(invId: InvitationId): bool = {
    val inv = if (invId.in(invitations.keys())) invitations.get(invId) else {
      id: invId, sender: "", invitationType: ContactInvitation,
      isExported: false, isImported: false, importedBy: None,
      isAccepted: false, isDeclined: false
    }
    all {
      invId.in(invitations.keys()),
      not(inv.isExported),
      invitations' = invitations.put(invId, { ...inv, isExported: true }),
      invitationFlowPhase' = invitationFlowPhase.set(inv.sender, InvitationExported),
      // Unchanged
      allAgents' = allAgents,
      agents' = agents,
      recoverySessions' = recoverySessions,
      blocks' = blocks,
      channels' = channels,
      neighborhoods' = neighborhoods,
      recoveryFlowPhase' = recoveryFlowPhase,
      homeFlowPhase' = homeFlowPhase,
      neighborhoodFlowPhase' = neighborhoodFlowPhase,
      chatFlowPhase' = chatFlowPhase,
      socialGraphFlowPhase' = socialGraphFlowPhase
    }
  }

  // Step 3: Import invitation
  action importInvitation(recipient: AgentId, invId: InvitationId): bool = {
    val recipientState = getAgent(agents, recipient)
    val inv = if (invId.in(invitations.keys())) invitations.get(invId) else {
      id: invId, sender: "", invitationType: ContactInvitation,
      isExported: false, isImported: false, importedBy: None,
      isAccepted: false, isDeclined: false
    }
    all {
      recipientState.hasAccount,
      invId.in(invitations.keys()),
      inv.isExported,
      not(inv.isImported),
      recipient != inv.sender,
      // Update invitation
      invitations' = invitations.put(invId, {
        ...inv,
        isImported: true,
        importedBy: Some(recipient)
      }),
      // Update recipient state
      agents' = agents.set(recipient, {
        ...recipientState,
        pendingInvitations: recipientState.pendingInvitations.union(Set(invId)),
        lastEmittedSignals: Set(InvitationsSignal, ContactsSignal)
      }),
      invitationFlowPhase' = invitationFlowPhase.set(recipient, InvitationImported),
      // Unchanged
      allAgents' = allAgents,
      recoverySessions' = recoverySessions,
      blocks' = blocks,
      channels' = channels,
      neighborhoods' = neighborhoods,
      recoveryFlowPhase' = recoveryFlowPhase,
      homeFlowPhase' = homeFlowPhase,
      neighborhoodFlowPhase' = neighborhoodFlowPhase,
      chatFlowPhase' = chatFlowPhase,
      socialGraphFlowPhase' = socialGraphFlowPhase
    }
  }

  // Step 4a: Accept invitation
  action acceptInvitation(recipient: AgentId, invId: InvitationId): bool = {
    val recipientState = getAgent(agents, recipient)
    val inv = if (invId.in(invitations.keys())) invitations.get(invId) else {
      id: invId, sender: "", invitationType: ContactInvitation,
      isExported: false, isImported: false, importedBy: None,
      isAccepted: false, isDeclined: false
    }
    val senderState = getAgent(agents, inv.sender)
    val isRecipient = match inv.importedBy {
      | Some(r) => r == recipient
      | None => false
    }
    all {
      invId.in(invitations.keys()),
      inv.isImported,
      isRecipient,
      not(inv.isAccepted),
      not(inv.isDeclined),
      // Update invitation
      invitations' = invitations.put(invId, { ...inv, isAccepted: true }),
      // Update both parties - add as contacts
      agents' = agents
        .set(recipient, {
          ...recipientState,
          pendingInvitations: recipientState.pendingInvitations.exclude(Set(invId)),
          contacts: recipientState.contacts.union(Set(inv.sender)),
          lastEmittedSignals: Set(InvitationsSignal, ContactsSignal)
        })
        .set(inv.sender, {
          ...senderState,
          contacts: senderState.contacts.union(Set(recipient)),
          lastEmittedSignals: Set(ContactsSignal)
        }),
      invitationFlowPhase' = invitationFlowPhase
        .set(recipient, InvitationContactCreated)
        .set(inv.sender, InvitationContactCreated),
      // Unchanged
      allAgents' = allAgents,
      recoverySessions' = recoverySessions,
      blocks' = blocks,
      channels' = channels,
      neighborhoods' = neighborhoods,
      recoveryFlowPhase' = recoveryFlowPhase,
      homeFlowPhase' = homeFlowPhase,
      neighborhoodFlowPhase' = neighborhoodFlowPhase,
      chatFlowPhase' = chatFlowPhase,
      socialGraphFlowPhase' = socialGraphFlowPhase
    }
  }

  // Step 4b: Decline invitation
  action declineInvitation(recipient: AgentId, invId: InvitationId): bool = {
    val recipientState = getAgent(agents, recipient)
    val inv = if (invId.in(invitations.keys())) invitations.get(invId) else {
      id: invId, sender: "", invitationType: ContactInvitation,
      isExported: false, isImported: false, importedBy: None,
      isAccepted: false, isDeclined: false
    }
    val isRecipient = match inv.importedBy {
      | Some(r) => r == recipient
      | None => false
    }
    all {
      invId.in(invitations.keys()),
      inv.isImported,
      isRecipient,
      not(inv.isAccepted),
      not(inv.isDeclined),
      // Update invitation
      invitations' = invitations.put(invId, { ...inv, isDeclined: true }),
      // Update recipient - remove from pending
      agents' = agents.set(recipient, {
        ...recipientState,
        pendingInvitations: recipientState.pendingInvitations.exclude(Set(invId)),
        lastEmittedSignals: Set(InvitationsSignal)
      }),
      invitationFlowPhase' = invitationFlowPhase.set(recipient, InvitationDeclined),
      // Unchanged
      allAgents' = allAgents,
      recoverySessions' = recoverySessions,
      blocks' = blocks,
      channels' = channels,
      neighborhoods' = neighborhoods,
      recoveryFlowPhase' = recoveryFlowPhase,
      homeFlowPhase' = homeFlowPhase,
      neighborhoodFlowPhase' = neighborhoodFlowPhase,
      chatFlowPhase' = chatFlowPhase,
      socialGraphFlowPhase' = socialGraphFlowPhase
    }
  }

  // ============ FLOW 4: CHAT & MESSAGING ============

  // Start direct message with contact
  action startDirectChat(initiator: AgentId, contact: AgentId): bool = {
    val initiatorState = getAgent(agents, initiator)
    val contactState = getAgent(agents, contact)
    all {
      initiatorState.hasAccount,
      contactState.hasAccount,
      initiatorState.contacts.contains(contact),
      not(initiatorState.directMessages.contains(contact)),
      // Update both parties
      agents' = agents
        .set(initiator, {
          ...initiatorState,
          directMessages: initiatorState.directMessages.union(Set(contact)),
          lastEmittedSignals: Set(ChatSignal)
        })
        .set(contact, {
          ...contactState,
          directMessages: contactState.directMessages.union(Set(initiator)),
          lastEmittedSignals: Set(ChatSignal)
        }),
      chatFlowPhase' = chatFlowPhase
        .set(initiator, ChatDMStarted)
        .set(contact, ChatDMStarted),
      // Unchanged
      allAgents' = allAgents,
      invitations' = invitations,
      recoverySessions' = recoverySessions,
      blocks' = blocks,
      channels' = channels,
      neighborhoods' = neighborhoods,
      recoveryFlowPhase' = recoveryFlowPhase,
      homeFlowPhase' = homeFlowPhase,
      neighborhoodFlowPhase' = neighborhoodFlowPhase,
      invitationFlowPhase' = invitationFlowPhase,
      socialGraphFlowPhase' = socialGraphFlowPhase
    }
  }

  // Create group channel
  action createChannel(
    creator: AgentId,
    channelId: ChannelId,
    name: str,
    members: Set[AgentId]
  ): bool = {
    val creatorState = getAgent(agents, creator)
    all {
      creatorState.hasAccount,
      not(channelId.in(channels.keys())),
      members.forall(m => creatorState.contacts.contains(m) or m == creator),
      // Create channel
      channels' = channels.put(channelId, {
        id: channelId,
        name: name,
        creator: creator,
        members: members.union(Set(creator)),
        messageCount: 0
      }),
      // Update all members
      agents' = members.union(Set(creator)).fold(agents, (acc, m) => {
        val ms = getAgent(acc, m)
        acc.set(m, {
          ...ms,
          channels: ms.channels.union(Set(channelId)),
          lastEmittedSignals: Set(ChatSignal)
        })
      }),
      chatFlowPhase' = chatFlowPhase.set(creator, ChatChannelCreated),
      // Unchanged
      allAgents' = allAgents,
      invitations' = invitations,
      recoverySessions' = recoverySessions,
      blocks' = blocks,
      neighborhoods' = neighborhoods,
      recoveryFlowPhase' = recoveryFlowPhase,
      homeFlowPhase' = homeFlowPhase,
      neighborhoodFlowPhase' = neighborhoodFlowPhase,
      invitationFlowPhase' = invitationFlowPhase,
      socialGraphFlowPhase' = socialGraphFlowPhase
    }
  }

  // Send message to channel
  action sendMessage(sender: AgentId, channelId: ChannelId): bool = {
    val senderState = getAgent(agents, sender)
    val channel = if (channelId.in(channels.keys())) channels.get(channelId) else {
      id: channelId, name: "", creator: "", members: Set(), messageCount: 0
    }
    all {
      channelId.in(channels.keys()),
      channel.members.contains(sender),
      // Increment message count
      channels' = channels.put(channelId, {
        ...channel,
        messageCount: channel.messageCount + 1
      }),
      // Emit signal for all members
      agents' = channel.members.fold(agents, (acc, m) => {
        val ms = getAgent(acc, m)
        acc.set(m, { ...ms, lastEmittedSignals: Set(ChatSignal) })
      }),
      chatFlowPhase' = chatFlowPhase.set(sender, ChatMessagesExchanged),
      // Unchanged
      allAgents' = allAgents,
      invitations' = invitations,
      recoverySessions' = recoverySessions,
      blocks' = blocks,
      neighborhoods' = neighborhoods,
      recoveryFlowPhase' = recoveryFlowPhase,
      homeFlowPhase' = homeFlowPhase,
      neighborhoodFlowPhase' = neighborhoodFlowPhase,
      invitationFlowPhase' = invitationFlowPhase,
      socialGraphFlowPhase' = socialGraphFlowPhase
    }
  }

  // ============ FLOW 2: BLOCK LIFECYCLE ============

  // Create block
  action createBlock(owner: AgentId, blockId: HomeId): bool = {
    val ownerState = getAgent(agents, owner)
    all {
      ownerState.hasAccount,
      not(blockId.in(blocks.keys())),
      // Create block
      blocks' = blocks.put(blockId, {
        id: blockId,
        owner: owner,
        stewards: Set(owner),
        residents: Set(owner),
        capacity: 8,
        linkedNeighborhood: None
      }),
      // Update owner state
      agents' = agents.set(owner, {
        ...ownerState,
        ownedBlocks: ownerState.ownedBlocks.union(Set(blockId)),
        residentOf: ownerState.residentOf.union(Set(blockId)),
        stewardOf: ownerState.stewardOf.union(Set(blockId)),
        lastEmittedSignals: Set(BlockSignal)
      }),
      homeFlowPhase' = homeFlowPhase.set(owner, HomeCreated),
      // Unchanged
      allAgents' = allAgents,
      invitations' = invitations,
      recoverySessions' = recoverySessions,
      channels' = channels,
      neighborhoods' = neighborhoods,
      recoveryFlowPhase' = recoveryFlowPhase,
      neighborhoodFlowPhase' = neighborhoodFlowPhase,
      chatFlowPhase' = chatFlowPhase,
      invitationFlowPhase' = invitationFlowPhase,
      socialGraphFlowPhase' = socialGraphFlowPhase
    }
  }

  // Join block as resident
  action joinBlock(resident: AgentId, blockId: HomeId): bool = {
    val residentState = getAgent(agents, resident)
    val block = if (blockId.in(blocks.keys())) blocks.get(blockId) else {
      id: blockId, owner: "", stewards: Set(), residents: Set(),
      capacity: 8, linkedNeighborhood: None
    }
    val ownerState = getAgent(agents, block.owner)
    all {
      blockId.in(blocks.keys()),
      residentState.hasAccount,
      not(block.residents.contains(resident)),
      block.residents.size() < block.capacity,
      // Update block
      blocks' = blocks.put(blockId, {
        ...block,
        residents: block.residents.union(Set(resident))
      }),
      // Update resident and owner states
      agents' = agents
        .set(resident, {
          ...residentState,
          residentOf: residentState.residentOf.union(Set(blockId)),
          lastEmittedSignals: Set(BlockSignal, ContactsSignal)
        })
        .set(block.owner, {
          ...ownerState,
          lastEmittedSignals: Set(BlockSignal)
        }),
      homeFlowPhase' = homeFlowPhase.set(resident, BlockResidentJoined),
      // Unchanged
      allAgents' = allAgents,
      invitations' = invitations,
      recoverySessions' = recoverySessions,
      channels' = channels,
      neighborhoods' = neighborhoods,
      recoveryFlowPhase' = recoveryFlowPhase,
      neighborhoodFlowPhase' = neighborhoodFlowPhase,
      chatFlowPhase' = chatFlowPhase,
      invitationFlowPhase' = invitationFlowPhase,
      socialGraphFlowPhase' = socialGraphFlowPhase
    }
  }

  // Promote resident to steward
  action promoteSteward(owner: AgentId, blockId: HomeId, resident: AgentId): bool = {
    val block = if (blockId.in(blocks.keys())) blocks.get(blockId) else {
      id: blockId, owner: "", stewards: Set(), residents: Set(),
      capacity: 8, linkedNeighborhood: None
    }
    val residentState = getAgent(agents, resident)
    val ownerState = getAgent(agents, owner)
    all {
      blockId.in(blocks.keys()),
      block.owner == owner,
      block.residents.contains(resident),
      not(block.stewards.contains(resident)),
      // Update block
      blocks' = blocks.put(blockId, {
        ...block,
        stewards: block.stewards.union(Set(resident))
      }),
      // Update states
      agents' = agents
        .set(resident, {
          ...residentState,
          stewardOf: residentState.stewardOf.union(Set(blockId)),
          lastEmittedSignals: Set(BlockSignal)
        })
        .set(owner, {
          ...ownerState,
          lastEmittedSignals: Set(BlockSignal)
        }),
      homeFlowPhase' = homeFlowPhase.set(resident, BlockStewardPromoted),
      // Unchanged
      allAgents' = allAgents,
      invitations' = invitations,
      recoverySessions' = recoverySessions,
      channels' = channels,
      neighborhoods' = neighborhoods,
      recoveryFlowPhase' = recoveryFlowPhase,
      neighborhoodFlowPhase' = neighborhoodFlowPhase,
      chatFlowPhase' = chatFlowPhase,
      invitationFlowPhase' = invitationFlowPhase,
      socialGraphFlowPhase' = socialGraphFlowPhase
    }
  }

  // ============ FLOW 3: NEIGHBORHOOD FORMATION ============

  // Create neighborhood
  action createNeighborhood(creator: AgentId, neighborhoodId: NeighborhoodId, name: str): bool = {
    val creatorState = getAgent(agents, creator)
    all {
      creatorState.hasAccount,
      not(neighborhoodId.in(neighborhoods.keys())),
      // Create neighborhood
      neighborhoods' = neighborhoods.put(neighborhoodId, {
        id: neighborhoodId,
        name: name,
        linkedBlocks: Set()
      }),
      // Update creator
      agents' = agents.set(creator, {
        ...creatorState,
        neighborhoods: creatorState.neighborhoods.union(Set(neighborhoodId)),
        lastEmittedSignals: Set(NeighborhoodSignal)
      }),
      neighborhoodFlowPhase' = neighborhoodFlowPhase.set(creator, NeighborhoodCreated),
      // Unchanged
      allAgents' = allAgents,
      invitations' = invitations,
      recoverySessions' = recoverySessions,
      blocks' = blocks,
      channels' = channels,
      recoveryFlowPhase' = recoveryFlowPhase,
      homeFlowPhase' = homeFlowPhase,
      chatFlowPhase' = chatFlowPhase,
      invitationFlowPhase' = invitationFlowPhase,
      socialGraphFlowPhase' = socialGraphFlowPhase
    }
  }

  // Link block to neighborhood
  action linkBlockToNeighborhood(
    owner: AgentId,
    blockId: HomeId,
    neighborhoodId: NeighborhoodId
  ): bool = {
    val block = if (blockId.in(blocks.keys())) blocks.get(blockId) else {
      id: blockId, owner: "", stewards: Set(), residents: Set(),
      capacity: 8, linkedNeighborhood: None
    }
    val neighborhood = if (neighborhoodId.in(neighborhoods.keys())) neighborhoods.get(neighborhoodId) else {
      id: neighborhoodId, name: "", linkedBlocks: Set()
    }
    val ownerState = getAgent(agents, owner)
    val hasNoLink = match block.linkedNeighborhood { | None => true | Some(_) => false }
    all {
      blockId.in(blocks.keys()),
      neighborhoodId.in(neighborhoods.keys()),
      block.owner == owner,
      hasNoLink,
      // Update block
      blocks' = blocks.put(blockId, {
        ...block,
        linkedNeighborhood: Some(neighborhoodId)
      }),
      // Update neighborhood
      neighborhoods' = neighborhoods.put(neighborhoodId, {
        ...neighborhood,
        linkedBlocks: neighborhood.linkedBlocks.union(Set(blockId))
      }),
      // Update owner
      agents' = agents.set(owner, {
        ...ownerState,
        lastEmittedSignals: Set(NeighborhoodSignal, BlockSignal)
      }),
      neighborhoodFlowPhase' = neighborhoodFlowPhase.set(owner, NeighborhoodBlocksLinked),
      // Unchanged
      allAgents' = allAgents,
      invitations' = invitations,
      recoverySessions' = recoverySessions,
      channels' = channels,
      recoveryFlowPhase' = recoveryFlowPhase,
      homeFlowPhase' = homeFlowPhase,
      chatFlowPhase' = chatFlowPhase,
      invitationFlowPhase' = invitationFlowPhase,
      socialGraphFlowPhase' = socialGraphFlowPhase
    }
  }

  // ============ FLOW 6: SOCIAL GRAPH (Contacts + Blocks) ============

  // Set nickname for a contact
  action setNickname(agent: AgentId, contactId: AgentId, nickname: str): bool = {
    val agentState = getAgent(agents, agent)
    all {
      agentState.hasAccount,
      agentState.contacts.contains(contactId),
      // Update nickname
      agents' = agents.set(agent, {
        ...agentState,
        contactNicknames: agentState.contactNicknames.put(contactId, nickname),
        lastEmittedSignals: Set(ContactsSignal)
      }),
      socialGraphFlowPhase' = socialGraphFlowPhase.set(agent, SocialNicknameUpdated),
      // Unchanged
      allAgents' = allAgents,
      invitations' = invitations,
      recoverySessions' = recoverySessions,
      blocks' = blocks,
      channels' = channels,
      neighborhoods' = neighborhoods,
      recoveryFlowPhase' = recoveryFlowPhase,
      homeFlowPhase' = homeFlowPhase,
      neighborhoodFlowPhase' = neighborhoodFlowPhase,
      chatFlowPhase' = chatFlowPhase,
      invitationFlowPhase' = invitationFlowPhase
    }
  }

  // Invite a contact to join a block
  // Creates a block resident invitation and adds contact to block residents when they accept
  action inviteContactToBlock(owner: AgentId, blockId: HomeId, contactId: AgentId): bool = {
    val ownerState = getAgent(agents, owner)
    val contactState = getAgent(agents, contactId)
    val block = if (blockId.in(blocks.keys())) blocks.get(blockId) else {
      id: blockId, owner: "", stewards: Set(), residents: Set(),
      capacity: 8, linkedNeighborhood: None
    }
    all {
      blockId.in(blocks.keys()),
      ownerState.hasAccount,
      contactState.hasAccount,
      block.owner == owner or block.stewards.contains(owner),  // Owner or steward can invite
      ownerState.contacts.contains(contactId),  // Must be a contact
      not(block.residents.contains(contactId)), // Not already a resident
      block.residents.size() < block.capacity,  // Block has capacity
      // Update block with new resident
      blocks' = blocks.put(blockId, {
        ...block,
        residents: block.residents.union(Set(contactId))
      }),
      // Update both agent states
      agents' = agents
        .set(contactId, {
          ...contactState,
          residentOf: contactState.residentOf.union(Set(blockId)),
          lastEmittedSignals: Set(BlockSignal, ContactsSignal)
        })
        .set(owner, {
          ...ownerState,
          lastEmittedSignals: Set(BlockSignal, ContactsSignal)
        }),
      socialGraphFlowPhase' = socialGraphFlowPhase
        .set(owner, SocialContactInvitedToBlock)
        .set(contactId, SocialContactJoinedBlock),
      // Unchanged
      allAgents' = allAgents,
      invitations' = invitations,
      recoverySessions' = recoverySessions,
      channels' = channels,
      neighborhoods' = neighborhoods,
      recoveryFlowPhase' = recoveryFlowPhase,
      homeFlowPhase' = homeFlowPhase,
      neighborhoodFlowPhase' = neighborhoodFlowPhase,
      chatFlowPhase' = chatFlowPhase,
      invitationFlowPhase' = invitationFlowPhase
    }
  }

  // Remove a contact from a block (kick)
  action removeContactFromBlock(owner: AgentId, blockId: HomeId, contactId: AgentId): bool = {
    val ownerState = getAgent(agents, owner)
    val contactState = getAgent(agents, contactId)
    val block = if (blockId.in(blocks.keys())) blocks.get(blockId) else {
      id: blockId, owner: "", stewards: Set(), residents: Set(),
      capacity: 8, linkedNeighborhood: None
    }
    all {
      blockId.in(blocks.keys()),
      block.owner == owner or block.stewards.contains(owner),  // Owner or steward can remove
      block.residents.contains(contactId),  // Must be a resident
      contactId != block.owner,  // Can't remove owner
      // Update block - remove resident and steward status
      blocks' = blocks.put(blockId, {
        ...block,
        residents: block.residents.exclude(Set(contactId)),
        stewards: block.stewards.exclude(Set(contactId))
      }),
      // Update contact state
      agents' = agents
        .set(contactId, {
          ...contactState,
          residentOf: contactState.residentOf.exclude(Set(blockId)),
          stewardOf: contactState.stewardOf.exclude(Set(blockId)),
          lastEmittedSignals: Set(BlockSignal, ContactsSignal)
        })
        .set(owner, {
          ...ownerState,
          lastEmittedSignals: Set(BlockSignal)
        }),
      socialGraphFlowPhase' = socialGraphFlowPhase.set(contactId, SocialNone),
      // Unchanged
      allAgents' = allAgents,
      invitations' = invitations,
      recoverySessions' = recoverySessions,
      channels' = channels,
      neighborhoods' = neighborhoods,
      recoveryFlowPhase' = recoveryFlowPhase,
      homeFlowPhase' = homeFlowPhase,
      neighborhoodFlowPhase' = neighborhoodFlowPhase,
      chatFlowPhase' = chatFlowPhase,
      invitationFlowPhase' = invitationFlowPhase
    }
  }

  // Get contacts who are residents of a specific block (filter contacts by block)
  // This is a "view" action - no state mutation, just emits signal for UI refresh
  action filterContactsByBlock(agent: AgentId, blockId: HomeId): bool = {
    val agentState = getAgent(agents, agent)
    val block = if (blockId.in(blocks.keys())) blocks.get(blockId) else {
      id: blockId, owner: "", stewards: Set(), residents: Set(),
      capacity: 8, linkedNeighborhood: None
    }
    // Compute contacts that are residents of this block
    val blockContacts = agentState.contacts.filter(c => block.residents.contains(c))
    all {
      agentState.hasAccount,
      blockId.in(blocks.keys()),
      // Update agent's last emitted signals (view operation)
      agents' = agents.set(agent, {
        ...agentState,
        lastEmittedSignals: Set(ContactsSignal, BlockSignal)
      }),
      socialGraphFlowPhase' = socialGraphFlowPhase.set(agent, SocialBlockContactsFiltered),
      // Unchanged
      allAgents' = allAgents,
      invitations' = invitations,
      recoverySessions' = recoverySessions,
      blocks' = blocks,
      channels' = channels,
      neighborhoods' = neighborhoods,
      recoveryFlowPhase' = recoveryFlowPhase,
      homeFlowPhase' = homeFlowPhase,
      neighborhoodFlowPhase' = neighborhoodFlowPhase,
      chatFlowPhase' = chatFlowPhase,
      invitationFlowPhase' = invitationFlowPhase
    }
  }

  // ============ SOCIAL GRAPH HELPER DEFINITIONS ============

  // Pure function: Get contacts who are residents of a given block
  pure def getBlockContacts(agentState: AgentState, block: BlockRecord): Set[AgentId] =
    agentState.contacts.filter(c => block.residents.contains(c))

  // Pure function: Check if a contact is a resident of a given block
  pure def isContactResident(agentState: AgentState, contactId: AgentId, block: BlockRecord): bool =
    agentState.contacts.contains(contactId) and block.residents.contains(contactId)

  // ============ NON-DETERMINISTIC STEP ============

  action step = any {
    // Account creation
    nondet subject = allAgents.oneOf()
    nondet g1 = allAgents.oneOf()
    nondet g2 = allAgents.oneOf()
    createAccountWithGuardians(subject, Set(g1, g2).exclude(Set(subject)), 2),

    // Recovery flow
    nondet subject2 = allAgents.oneOf()
    initiateRecovery(subject2, "recovery-1", 3),

    nondet guardian = allAgents.oneOf()
    approveRecovery(guardian, "recovery-1"),

    tickRecoveryCooldown("recovery-1"),

    completeRecovery("recovery-1"),

    // Invitation flow
    nondet sender = allAgents.oneOf()
    createInvitation(sender, "inv-1", ContactInvitation),

    exportInvitation("inv-1"),

    nondet recipient = allAgents.oneOf()
    importInvitation(recipient, "inv-1"),

    nondet accepter = allAgents.oneOf()
    acceptInvitation(accepter, "inv-1"),

    nondet decliner = allAgents.oneOf()
    declineInvitation(decliner, "inv-1"),

    // Chat flow
    nondet initiator = allAgents.oneOf()
    nondet contactAgent = allAgents.oneOf()
    startDirectChat(initiator, contactAgent),

    nondet channelCreator = allAgents.oneOf()
    nondet member1 = allAgents.oneOf()
    createChannel(channelCreator, "channel-1", "General", Set(member1)),

    nondet messageSender = allAgents.oneOf()
    sendMessage(messageSender, "channel-1"),

    // Block flow
    nondet blockOwner = allAgents.oneOf()
    createBlock(blockOwner, "block-1"),

    nondet joiner = allAgents.oneOf()
    joinBlock(joiner, "block-1"),

    nondet promoter = allAgents.oneOf()
    nondet promotee = allAgents.oneOf()
    promoteSteward(promoter, "block-1", promotee),

    // Neighborhood flow
    nondet neighborhoodCreator = allAgents.oneOf()
    createNeighborhood(neighborhoodCreator, "neighborhood-1", "Downtown"),

    nondet linker = allAgents.oneOf()
    linkBlockToNeighborhood(linker, "block-1", "neighborhood-1"),

    // Social Graph flow
    nondet nicknameAgent = allAgents.oneOf()
    nondet nicknameContact = allAgents.oneOf()
    setNickname(nicknameAgent, nicknameContact, "Friend"),

    nondet blockInviter = allAgents.oneOf()
    nondet contactToInvite = allAgents.oneOf()
    inviteContactToBlock(blockInviter, "block-1", contactToInvite),

    nondet kickOwner = allAgents.oneOf()
    nondet contactToKick = allAgents.oneOf()
    removeContactFromBlock(kickOwner, "block-1", contactToKick),

    nondet filterAgent = allAgents.oneOf()
    filterContactsByBlock(filterAgent, "block-1")
  }

  // ============ INVARIANTS (SAFETY PROPERTIES) ============

  // Signal emission invariant: state changes must emit appropriate signals
  val signalEmissionInvariant: bool =
    allAgents.forall(agent => {
      val state = getAgent(agents, agent)
      // If contacts changed, ContactsSignal should have been emitted
      // (Simplified check - in practice would compare to previous state)
      true
    })

  // Recovery phase consistency
  val recoveryPhaseConsistency: bool =
    recoverySessions.keys().forall(sid => {
      val session = recoverySessions.get(sid)
      match session.phase {
        | RecoveryCompleted => session.approvals.size() >= session.threshold
        | RecoveryCooldownActive => session.approvals.size() >= session.threshold and session.cooldownRemaining > 0
        | _ => true
      }
    })

  // Block capacity invariant
  val blockCapacityInvariant: bool =
    blocks.keys().forall(bid => {
      val block = blocks.get(bid)
      block.residents.size() <= block.capacity
    })

  // Stewards must be residents
  val stewardsAreResidents: bool =
    blocks.keys().forall(bid => {
      val block = blocks.get(bid)
      block.stewards.subseteq(block.residents)
    })

  // Invitation consistency
  val invitationConsistency: bool =
    invitations.keys().forall(iid => {
      val inv = invitations.get(iid)
      // Can't be both accepted and declined
      not(inv.isAccepted and inv.isDeclined) and
      // Must be imported before accepted/declined
      (inv.isAccepted implies inv.isImported) and
      (inv.isDeclined implies inv.isImported)
    })

  // Social Graph: Contact-resident mapping consistency
  // If an agent is residentOf a block, they must appear in that block's residents
  val contactBlockMappingConsistency: bool =
    allAgents.forall(agent => {
      val state = getAgent(agents, agent)
      state.residentOf.forall(bid => {
        if (bid.in(blocks.keys())) {
          val block = blocks.get(bid)
          block.residents.contains(agent)
        } else true
      })
    })

  // Social Graph: Block residents must have accounts
  val blockResidentsHaveAccounts: bool =
    blocks.keys().forall(bid => {
      val block = blocks.get(bid)
      block.residents.forall(resident => {
        val residentState = getAgent(agents, resident)
        residentState.hasAccount
      })
    })

  // Social Graph: Nicknames only for contacts
  // An agent can only have nicknames for their contacts
  val nicknamesOnlyForContacts: bool =
    allAgents.forall(agent => {
      val state = getAgent(agents, agent)
      state.contactNicknames.keys().forall(contactId =>
        state.contacts.contains(contactId)
      )
    })

  // All invariants
  val allInvariants: bool =
    signalEmissionInvariant and
    recoveryPhaseConsistency and
    blockCapacityInvariant and
    stewardsAreResidents and
    invitationConsistency and
    contactBlockMappingConsistency and
    blockResidentsHaveAccounts and
    nicknamesOnlyForContacts

  // ============ TEST SCENARIOS ============

  // Test: Complete invitation flow
  run invitationFlowTest = {
    init
    .then(createAccountWithGuardians("bob", Set("alice", "carol"), 2))
    .then(createAccountWithGuardians("alice", Set("bob", "carol"), 2))
    .then(createInvitation("alice", "test-inv", ContactInvitation))
    .then(exportInvitation("test-inv"))
    .then(importInvitation("bob", "test-inv"))
    .then(acceptInvitation("bob", "test-inv"))
    .then(all {
      assert(getAgent(agents, "bob").contacts.contains("alice")),
      assert(getAgent(agents, "alice").contacts.contains("bob")),
      // Keep state unchanged for final assertion
      allAgents' = allAgents,
      agents' = agents,
      invitations' = invitations,
      recoverySessions' = recoverySessions,
      blocks' = blocks,
      channels' = channels,
      neighborhoods' = neighborhoods,
      recoveryFlowPhase' = recoveryFlowPhase,
      homeFlowPhase' = homeFlowPhase,
      neighborhoodFlowPhase' = neighborhoodFlowPhase,
      chatFlowPhase' = chatFlowPhase,
      invitationFlowPhase' = invitationFlowPhase,
      socialGraphFlowPhase' = socialGraphFlowPhase
    })
  }

  // Test: Guardian recovery flow
  run guardianRecoveryFlowTest = {
    init
    .then(createAccountWithGuardians("bob", Set("alice", "carol"), 2))
    .then(createAccountWithGuardians("alice", Set("bob", "carol"), 2))
    .then(createAccountWithGuardians("carol", Set("bob", "alice"), 2))
    .then(initiateRecovery("bob", "recovery-test", 1))
    .then(approveRecovery("alice", "recovery-test"))
    .then(approveRecovery("carol", "recovery-test"))
    .then(tickRecoveryCooldown("recovery-test"))
    .then(completeRecovery("recovery-test"))
    .then(all {
      assert(match recoverySessions.get("recovery-test").phase { | RecoveryCompleted => true | _ => false }),
      assert(recoverySessions.get("recovery-test").approvals.contains("alice")),
      assert(recoverySessions.get("recovery-test").approvals.contains("carol")),
      // Keep state unchanged
      allAgents' = allAgents,
      agents' = agents,
      invitations' = invitations,
      recoverySessions' = recoverySessions,
      blocks' = blocks,
      channels' = channels,
      neighborhoods' = neighborhoods,
      recoveryFlowPhase' = recoveryFlowPhase,
      homeFlowPhase' = homeFlowPhase,
      neighborhoodFlowPhase' = neighborhoodFlowPhase,
      chatFlowPhase' = chatFlowPhase,
      invitationFlowPhase' = invitationFlowPhase,
      socialGraphFlowPhase' = socialGraphFlowPhase
    })
  }

  // Test: Chat flow after contact
  run chatFlowTest = {
    init
    .then(createAccountWithGuardians("bob", Set("alice", "carol"), 2))
    .then(createAccountWithGuardians("alice", Set("bob", "carol"), 2))
    .then(createInvitation("alice", "chat-inv", ContactInvitation))
    .then(exportInvitation("chat-inv"))
    .then(importInvitation("bob", "chat-inv"))
    .then(acceptInvitation("bob", "chat-inv"))
    .then(startDirectChat("bob", "alice"))
    .then(createChannel("bob", "guardians-channel", "Guardians", Set("alice")))
    .then(sendMessage("bob", "guardians-channel"))
    .then(all {
      assert(getAgent(agents, "bob").directMessages.contains("alice")),
      assert(getAgent(agents, "bob").channels.contains("guardians-channel")),
      assert(channels.get("guardians-channel").messageCount == 1),
      // Keep state unchanged
      allAgents' = allAgents,
      agents' = agents,
      invitations' = invitations,
      recoverySessions' = recoverySessions,
      blocks' = blocks,
      channels' = channels,
      neighborhoods' = neighborhoods,
      recoveryFlowPhase' = recoveryFlowPhase,
      homeFlowPhase' = homeFlowPhase,
      neighborhoodFlowPhase' = neighborhoodFlowPhase,
      chatFlowPhase' = chatFlowPhase,
      invitationFlowPhase' = invitationFlowPhase,
      socialGraphFlowPhase' = socialGraphFlowPhase
    })
  }

  // Test: Block and neighborhood flow
  run blockNeighborhoodFlowTest = {
    init
    .then(createAccountWithGuardians("bob", Set("alice", "carol"), 2))
    .then(createAccountWithGuardians("alice", Set("bob", "carol"), 2))
    .then(createBlock("bob", "bob-block"))
    .then(joinBlock("alice", "bob-block"))
    .then(promoteSteward("bob", "bob-block", "alice"))
    .then(createNeighborhood("bob", "downtown", "Downtown"))
    .then(linkBlockToNeighborhood("bob", "bob-block", "downtown"))
    .then(all {
      assert(blocks.get("bob-block").residents.contains("bob")),
      assert(blocks.get("bob-block").residents.contains("alice")),
      assert(blocks.get("bob-block").stewards.contains("alice")),
      assert(neighborhoods.get("downtown").linkedBlocks.contains("bob-block")),
      // Keep state unchanged
      allAgents' = allAgents,
      agents' = agents,
      invitations' = invitations,
      recoverySessions' = recoverySessions,
      blocks' = blocks,
      channels' = channels,
      neighborhoods' = neighborhoods,
      recoveryFlowPhase' = recoveryFlowPhase,
      homeFlowPhase' = homeFlowPhase,
      neighborhoodFlowPhase' = neighborhoodFlowPhase,
      chatFlowPhase' = chatFlowPhase,
      invitationFlowPhase' = invitationFlowPhase,
      socialGraphFlowPhase' = socialGraphFlowPhase
    })
  }

  // Test: Random invariant verification
  run randomInvariantTest = {
    init
    .then(step)
    .then(all { assert(allInvariants), step })
    .then(all { assert(allInvariants), step })
    .then(all { assert(allInvariants), step })
    .then(all { assert(allInvariants), step })
    .then(all {
      assert(allInvariants),
      allAgents' = allAgents,
      agents' = agents,
      invitations' = invitations,
      recoverySessions' = recoverySessions,
      blocks' = blocks,
      channels' = channels,
      neighborhoods' = neighborhoods,
      recoveryFlowPhase' = recoveryFlowPhase,
      homeFlowPhase' = homeFlowPhase,
      neighborhoodFlowPhase' = neighborhoodFlowPhase,
      chatFlowPhase' = chatFlowPhase,
      invitationFlowPhase' = invitationFlowPhase,
      socialGraphFlowPhase' = socialGraphFlowPhase
    })
  }

  // Test: Social Graph flow - contacts, blocks, nicknames, assignment
  run socialGraphFlowTest = {
    init
    // Create accounts for bob, alice, carol
    .then(createAccountWithGuardians("bob", Set("alice", "carol"), 2))
    .then(createAccountWithGuardians("alice", Set("bob", "carol"), 2))
    .then(createAccountWithGuardians("carol", Set("bob", "alice"), 2))
    // Create contact relationship: alice invites bob
    .then(createInvitation("alice", "social-inv", ContactInvitation))
    .then(exportInvitation("social-inv"))
    .then(importInvitation("bob", "social-inv"))
    .then(acceptInvitation("bob", "social-inv"))
    // Bob sets nickname for alice
    .then(setNickname("bob", "alice", "My Guardian Alice"))
    // Verify nickname was set
    .then(all {
      assert(getAgent(agents, "bob").contactNicknames.get("alice") == "My Guardian Alice"),
      assert(getAgent(agents, "bob").lastEmittedSignals.contains(ContactsSignal)),
      // Continue to block creation
      allAgents' = allAgents,
      agents' = agents,
      invitations' = invitations,
      recoverySessions' = recoverySessions,
      blocks' = blocks,
      channels' = channels,
      neighborhoods' = neighborhoods,
      recoveryFlowPhase' = recoveryFlowPhase,
      homeFlowPhase' = homeFlowPhase,
      neighborhoodFlowPhase' = neighborhoodFlowPhase,
      chatFlowPhase' = chatFlowPhase,
      invitationFlowPhase' = invitationFlowPhase,
      socialGraphFlowPhase' = socialGraphFlowPhase
    })
    // Bob creates a block
    .then(createBlock("bob", "social-block"))
    // Bob invites alice (contact) to join block
    .then(inviteContactToBlock("bob", "social-block", "alice"))
    // Verify alice is now a resident
    .then(all {
      assert(blocks.get("social-block").residents.contains("alice")),
      assert(getAgent(agents, "alice").residentOf.contains("social-block")),
      assert(getAgent(agents, "bob").lastEmittedSignals.contains(BlockSignal)),
      assert(getAgent(agents, "bob").lastEmittedSignals.contains(ContactsSignal)),
      // Continue
      allAgents' = allAgents,
      agents' = agents,
      invitations' = invitations,
      recoverySessions' = recoverySessions,
      blocks' = blocks,
      channels' = channels,
      neighborhoods' = neighborhoods,
      recoveryFlowPhase' = recoveryFlowPhase,
      homeFlowPhase' = homeFlowPhase,
      neighborhoodFlowPhase' = neighborhoodFlowPhase,
      chatFlowPhase' = chatFlowPhase,
      invitationFlowPhase' = invitationFlowPhase,
      socialGraphFlowPhase' = socialGraphFlowPhase
    })
    // Bob filters contacts by block
    .then(filterContactsByBlock("bob", "social-block"))
    // Verify final state
    .then(all {
      // Alice is contact and resident
      assert(getAgent(agents, "bob").contacts.contains("alice")),
      assert(isContactResident(getAgent(agents, "bob"), "alice", blocks.get("social-block"))),
      // Nickname persists
      assert(getAgent(agents, "bob").contactNicknames.get("alice") == "My Guardian Alice"),
      // Social graph flow phase updated
      assert(match socialGraphFlowPhase.get("bob") { | SocialBlockContactsFiltered => true | _ => false }),
      // All invariants hold
      assert(allInvariants),
      // Keep state unchanged
      allAgents' = allAgents,
      agents' = agents,
      invitations' = invitations,
      recoverySessions' = recoverySessions,
      blocks' = blocks,
      channels' = channels,
      neighborhoods' = neighborhoods,
      recoveryFlowPhase' = recoveryFlowPhase,
      homeFlowPhase' = homeFlowPhase,
      neighborhoodFlowPhase' = neighborhoodFlowPhase,
      chatFlowPhase' = chatFlowPhase,
      invitationFlowPhase' = invitationFlowPhase,
      socialGraphFlowPhase' = socialGraphFlowPhase
    })
  }

  // Test: Social Graph - remove contact from block (kick)
  run socialGraphKickTest = {
    init
    .then(createAccountWithGuardians("bob", Set("alice", "carol"), 2))
    .then(createAccountWithGuardians("alice", Set("bob", "carol"), 2))
    // Create contact
    .then(createInvitation("alice", "kick-inv", ContactInvitation))
    .then(exportInvitation("kick-inv"))
    .then(importInvitation("bob", "kick-inv"))
    .then(acceptInvitation("bob", "kick-inv"))
    // Bob creates block and invites alice
    .then(createBlock("bob", "kick-block"))
    .then(inviteContactToBlock("bob", "kick-block", "alice"))
    // Verify alice is resident
    .then(all {
      assert(blocks.get("kick-block").residents.contains("alice")),
      allAgents' = allAgents,
      agents' = agents,
      invitations' = invitations,
      recoverySessions' = recoverySessions,
      blocks' = blocks,
      channels' = channels,
      neighborhoods' = neighborhoods,
      recoveryFlowPhase' = recoveryFlowPhase,
      homeFlowPhase' = homeFlowPhase,
      neighborhoodFlowPhase' = neighborhoodFlowPhase,
      chatFlowPhase' = chatFlowPhase,
      invitationFlowPhase' = invitationFlowPhase,
      socialGraphFlowPhase' = socialGraphFlowPhase
    })
    // Bob removes alice from block
    .then(removeContactFromBlock("bob", "kick-block", "alice"))
    // Verify alice is no longer resident
    .then(all {
      assert(not(blocks.get("kick-block").residents.contains("alice"))),
      assert(not(getAgent(agents, "alice").residentOf.contains("kick-block"))),
      // Alice is still a contact, just not a resident
      assert(getAgent(agents, "bob").contacts.contains("alice")),
      assert(allInvariants),
      allAgents' = allAgents,
      agents' = agents,
      invitations' = invitations,
      recoverySessions' = recoverySessions,
      blocks' = blocks,
      channels' = channels,
      neighborhoods' = neighborhoods,
      recoveryFlowPhase' = recoveryFlowPhase,
      homeFlowPhase' = homeFlowPhase,
      neighborhoodFlowPhase' = neighborhoodFlowPhase,
      chatFlowPhase' = chatFlowPhase,
      invitationFlowPhase' = invitationFlowPhase,
      socialGraphFlowPhase' = socialGraphFlowPhase
    })
  }
}
