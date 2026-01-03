// CLI Recovery Demo - Full Fidelity Flow Verification
//
// Models the complete cli_recovery demo from docs/demo/cli_recovery.md
// with ZERO skipped steps and DEEP validation.
//
// Flow Phases:
// 1. alice_carol_setup: Create Alice and Carol authorities
// 2. bob_onboarding: Create Bob, import contacts, send guardian requests, accept
// 3. group_chat_setup: Create group chat with all three
// 4. group_messaging: Messages flow between parties
// 5. bob_account_loss: Simulated device loss
// 6. recovery_initiation: Bob initiates, guardians approve
// 7. account_restoration: Threshold key recovery
// 8. post_recovery_messaging: Bob rejoins chat
//
// This spec models the explicit guardian request/accept flow,
// Not the `createAccountWithGuardians` that skips this.

module cli_recovery_demo {
  // ============ TYPE DEFINITIONS ============
  // (Self-contained - no imports since Quint type imports don't work)

  type Option[a] = Some(a) | None

  type AgentId = str
  type InvitationId = str
  type ChannelId = str
  type RecoverySessionId = str
  type GuardianRequestId = str

  // ============ SIGNAL TYPES ============

  type SignalType =
    | ChatSignal
    | ContactsSignal
    | InvitationsSignal
    | RecoverySignal

  // ============ FLOW TYPES ============

  type RecoveryFlowPhase =
    | RecoveryIdle
    | RecoveryRequested
    | RecoveryApprovalsPending
    | RecoveryCooldownActive
    | RecoverySharesSubmitted
    | RecoveryCompleted
    | RecoveryCancelled

  type ChatFlowPhase =
    | ChatNone
    | ChatContactExists
    | ChatDMStarted
    | ChatChannelCreated
    | ChatMembersInvited
    | ChatMessagesExchanged

  type InvitationFlowPhase =
    | InvitationNone
    | InvitationCreated
    | InvitationExported
    | InvitationImported
    | InvitationAccepted
    | InvitationDeclined
    | InvitationContactCreated

  // Demo phase tracking
  type DemoPhase =
    | PhaseInit
    | PhaseAliceCarolSetup
    | PhaseBobOnboarding
    | PhaseGroupChatSetup
    | PhaseGroupMessaging
    | PhaseBobAccountLoss
    | PhaseRecoveryInitiation
    | PhaseAccountRestoration
    | PhasePostRecoveryMessaging
    | PhaseComplete

  // ============ AGENT STATE ============

  type AgentState = {
    hasAccount: bool,
    authorityId: str,
    contacts: Set[AgentId],
    guardians: Set[AgentId],
    guardianFor: Set[AgentId],
    pendingInvitations: Set[InvitationId],
    sentInvitations: Set[InvitationId],
    channels: Set[ChannelId],
    activeRecoverySession: Option[RecoverySessionId],
    pendingRecoveryApprovals: Set[GuardianRequestId],
    lastEmittedSignals: Set[SignalType]
  }

  // ============ INVITATION RECORD ============

  type InvitationRecord = {
    id: InvitationId,
    sender: AgentId,
    isExported: bool,
    isImported: bool,
    importedBy: Option[AgentId],
    isAccepted: bool,
    isDeclined: bool
  }

  // ============ GUARDIAN REQUEST ============

  type GuardianRequest = {
    id: GuardianRequestId,
    requester: AgentId,
    guardian: AgentId,
    isPending: bool,
    isAccepted: bool,
    isDeclined: bool
  }

  // ============ RECOVERY SESSION ============

  type RecoverySession = {
    id: RecoverySessionId,
    subject: AgentId,
    guardians: Set[AgentId],
    approvals: Set[AgentId],
    threshold: int,
    cooldownRemaining: int,
    phase: RecoveryFlowPhase
  }

  // ============ CHANNEL RECORD ============

  type ChannelRecord = {
    id: ChannelId,
    name: str,
    creator: AgentId,
    members: Set[AgentId],
    messageCount: int
  }

  // ============ STATE VARIABLES ============

  var agents: AgentId -> AgentState
  var allAgents: Set[AgentId]
  var invitations: InvitationId -> InvitationRecord
  var guardianRequests: GuardianRequestId -> GuardianRequest
  var recoverySessions: RecoverySessionId -> RecoverySession
  var channels: ChannelId -> ChannelRecord
  var recoveryFlowPhase: AgentId -> RecoveryFlowPhase
  var chatFlowPhase: AgentId -> ChatFlowPhase
  var invitationFlowPhase: AgentId -> InvitationFlowPhase
  var currentDemoPhase: DemoPhase

  // ============ HELPERS ============

  pure def defaultAgentState(aid: AgentId): AgentState = {
    hasAccount: false,
    authorityId: "",
    contacts: Set(),
    guardians: Set(),
    guardianFor: Set(),
    pendingInvitations: Set(),
    sentInvitations: Set(),
    channels: Set(),
    activeRecoverySession: None,
    pendingRecoveryApprovals: Set(),
    lastEmittedSignals: Set()
  }

  pure def getAgent(agentMap: AgentId -> AgentState, aid: AgentId): AgentState =
    if (aid.in(agentMap.keys())) agentMap.get(aid) else defaultAgentState(aid)

  // ============ HELPER: Check if contact is a guardian ============

  pure def isContactGuardian(agentState: AgentState, contactId: AgentId): bool =
    agentState.contacts.contains(contactId) and agentState.guardians.contains(contactId)

  // ============ INITIALIZATION ============

  action init = all {
    allAgents' = Set("bob", "alice", "carol"),
    agents' = Map(
      "bob" -> defaultAgentState("bob"),
      "alice" -> defaultAgentState("alice"),
      "carol" -> defaultAgentState("carol")
    ),
    invitations' = Map(),
    guardianRequests' = Map(),
    recoverySessions' = Map(),
    channels' = Map(),
    recoveryFlowPhase' = Map(
      "bob" -> RecoveryIdle,
      "alice" -> RecoveryIdle,
      "carol" -> RecoveryIdle
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
    currentDemoPhase' = PhaseInit
  }

  // ============ ACCOUNT CREATION ============

  action createAccount(agent: AgentId): bool = {
    val state = getAgent(agents, agent)
    all {
      not(state.hasAccount),
      agents' = agents.set(agent, {
        ...state,
        hasAccount: true,
        authorityId: agent
      }),
      allAgents' = allAgents,
      invitations' = invitations,
      guardianRequests' = guardianRequests,
      recoverySessions' = recoverySessions,
      channels' = channels,
      recoveryFlowPhase' = recoveryFlowPhase,
      chatFlowPhase' = chatFlowPhase,
      invitationFlowPhase' = invitationFlowPhase,
      currentDemoPhase' = currentDemoPhase
    }
  }

  action setDemoPhase(phase: DemoPhase): bool = all {
    currentDemoPhase' = phase,
    allAgents' = allAgents,
    agents' = agents,
    invitations' = invitations,
    guardianRequests' = guardianRequests,
    recoverySessions' = recoverySessions,
    channels' = channels,
    recoveryFlowPhase' = recoveryFlowPhase,
    chatFlowPhase' = chatFlowPhase,
    invitationFlowPhase' = invitationFlowPhase
  }

  // ============ INVITATION FLOW ============

  action createContactInvitation(sender: AgentId, invId: InvitationId): bool = {
    val senderState = getAgent(agents, sender)
    all {
      senderState.hasAccount,
      not(invId.in(invitations.keys())),
      invitations' = invitations.put(invId, {
        id: invId,
        sender: sender,
        isExported: false,
        isImported: false,
        importedBy: None,
        isAccepted: false,
        isDeclined: false
      }),
      agents' = agents.set(sender, {
        ...senderState,
        sentInvitations: senderState.sentInvitations.union(Set(invId)),
        lastEmittedSignals: Set(ContactsSignal)
      }),
      invitationFlowPhase' = invitationFlowPhase.set(sender, InvitationCreated),
      allAgents' = allAgents,
      guardianRequests' = guardianRequests,
      recoverySessions' = recoverySessions,
      channels' = channels,
      recoveryFlowPhase' = recoveryFlowPhase,
      chatFlowPhase' = chatFlowPhase,
      currentDemoPhase' = currentDemoPhase
    }
  }

  action exportInvitation(invId: InvitationId): bool = {
    val inv = invitations.get(invId)
    all {
      invId.in(invitations.keys()),
      not(inv.isExported),
      invitations' = invitations.put(invId, { ...inv, isExported: true }),
      invitationFlowPhase' = invitationFlowPhase.set(inv.sender, InvitationExported),
      allAgents' = allAgents,
      agents' = agents,
      guardianRequests' = guardianRequests,
      recoverySessions' = recoverySessions,
      channels' = channels,
      recoveryFlowPhase' = recoveryFlowPhase,
      chatFlowPhase' = chatFlowPhase,
      currentDemoPhase' = currentDemoPhase
    }
  }

  action importInvitation(recipient: AgentId, invId: InvitationId): bool = {
    val recipientState = getAgent(agents, recipient)
    val inv = invitations.get(invId)
    all {
      recipientState.hasAccount,
      invId.in(invitations.keys()),
      inv.isExported,
      not(inv.isImported),
      recipient != inv.sender,
      invitations' = invitations.put(invId, {
        ...inv,
        isImported: true,
        importedBy: Some(recipient)
      }),
      agents' = agents.set(recipient, {
        ...recipientState,
        pendingInvitations: recipientState.pendingInvitations.union(Set(invId)),
        lastEmittedSignals: Set(ContactsSignal)
      }),
      invitationFlowPhase' = invitationFlowPhase.set(recipient, InvitationImported),
      allAgents' = allAgents,
      guardianRequests' = guardianRequests,
      recoverySessions' = recoverySessions,
      channels' = channels,
      recoveryFlowPhase' = recoveryFlowPhase,
      chatFlowPhase' = chatFlowPhase,
      currentDemoPhase' = currentDemoPhase
    }
  }

  action acceptContactInvitation(recipient: AgentId, invId: InvitationId): bool = {
    val recipientState = getAgent(agents, recipient)
    val inv = invitations.get(invId)
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
      invitations' = invitations.put(invId, { ...inv, isAccepted: true }),
      agents' = agents
        .set(recipient, {
          ...recipientState,
          pendingInvitations: recipientState.pendingInvitations.exclude(Set(invId)),
          contacts: recipientState.contacts.union(Set(inv.sender)),
          lastEmittedSignals: Set(ContactsSignal)
        })
        .set(inv.sender, {
          ...senderState,
          contacts: senderState.contacts.union(Set(recipient)),
          lastEmittedSignals: Set(ContactsSignal)
        }),
      invitationFlowPhase' = invitationFlowPhase
        .set(recipient, InvitationContactCreated)
        .set(inv.sender, InvitationContactCreated),
      allAgents' = allAgents,
      guardianRequests' = guardianRequests,
      recoverySessions' = recoverySessions,
      channels' = channels,
      recoveryFlowPhase' = recoveryFlowPhase,
      chatFlowPhase' = chatFlowPhase,
      currentDemoPhase' = currentDemoPhase
    }
  }

  // ============ GUARDIAN REQUEST FLOW ============

  action sendGuardianRequest(requester: AgentId, guardian: AgentId, requestId: GuardianRequestId): bool = {
    val requesterState = getAgent(agents, requester)
    val guardianState = getAgent(agents, guardian)
    all {
      requesterState.hasAccount,
      guardianState.hasAccount,
      requesterState.contacts.contains(guardian),
      not(requesterState.guardians.contains(guardian)),
      not(requestId.in(guardianRequests.keys())),
      guardianRequests' = guardianRequests.put(requestId, {
        id: requestId,
        requester: requester,
        guardian: guardian,
        isPending: true,
        isAccepted: false,
        isDeclined: false
      }),
      agents' = agents.set(guardian, {
        ...guardianState,
        pendingRecoveryApprovals: guardianState.pendingRecoveryApprovals.union(Set(requestId)),
        lastEmittedSignals: Set(RecoverySignal, ContactsSignal)
      }),
      allAgents' = allAgents,
      invitations' = invitations,
      recoverySessions' = recoverySessions,
      channels' = channels,
      recoveryFlowPhase' = recoveryFlowPhase,
      chatFlowPhase' = chatFlowPhase,
      invitationFlowPhase' = invitationFlowPhase,
      currentDemoPhase' = currentDemoPhase
    }
  }

  action acceptGuardianRequest(guardian: AgentId, requestId: GuardianRequestId): bool = {
    val request = guardianRequests.get(requestId)
    val guardianState = getAgent(agents, guardian)
    val requesterState = getAgent(agents, request.requester)
    all {
      requestId.in(guardianRequests.keys()),
      request.guardian == guardian,
      request.isPending,
      not(request.isAccepted),
      not(request.isDeclined),
      guardianRequests' = guardianRequests.put(requestId, {
        ...request,
        isPending: false,
        isAccepted: true
      }),
      agents' = agents
        .set(request.requester, {
          ...requesterState,
          guardians: requesterState.guardians.union(Set(guardian)),
          lastEmittedSignals: Set(RecoverySignal, ContactsSignal)
        })
        .set(guardian, {
          ...guardianState,
          guardianFor: guardianState.guardianFor.union(Set(request.requester)),
          pendingRecoveryApprovals: guardianState.pendingRecoveryApprovals.exclude(Set(requestId)),
          lastEmittedSignals: Set(RecoverySignal, ContactsSignal)
        }),
      allAgents' = allAgents,
      invitations' = invitations,
      recoverySessions' = recoverySessions,
      channels' = channels,
      recoveryFlowPhase' = recoveryFlowPhase,
      chatFlowPhase' = chatFlowPhase,
      invitationFlowPhase' = invitationFlowPhase,
      currentDemoPhase' = currentDemoPhase
    }
  }

  // ============ CHAT FLOW ============

  action createGroupChannel(creator: AgentId, channelId: ChannelId, name: str, members: Set[AgentId]): bool = {
    val creatorState = getAgent(agents, creator)
    all {
      creatorState.hasAccount,
      not(channelId.in(channels.keys())),
      members.forall(m => creatorState.contacts.contains(m) or m == creator),
      channels' = channels.put(channelId, {
        id: channelId,
        name: name,
        creator: creator,
        members: members.union(Set(creator)),
        messageCount: 0
      }),
      agents' = members.union(Set(creator)).fold(agents, (acc, m) => {
        val ms = getAgent(acc, m)
        acc.set(m, {
          ...ms,
          channels: ms.channels.union(Set(channelId)),
          lastEmittedSignals: Set(ChatSignal)
        })
      }),
      chatFlowPhase' = chatFlowPhase.set(creator, ChatChannelCreated),
      allAgents' = allAgents,
      invitations' = invitations,
      guardianRequests' = guardianRequests,
      recoverySessions' = recoverySessions,
      recoveryFlowPhase' = recoveryFlowPhase,
      invitationFlowPhase' = invitationFlowPhase,
      currentDemoPhase' = currentDemoPhase
    }
  }

  action sendMessage(sender: AgentId, channelId: ChannelId): bool = {
    val channel = channels.get(channelId)
    all {
      channelId.in(channels.keys()),
      channel.members.contains(sender),
      channels' = channels.put(channelId, {
        ...channel,
        messageCount: channel.messageCount + 1
      }),
      agents' = channel.members.fold(agents, (acc, m) => {
        val ms = getAgent(acc, m)
        acc.set(m, { ...ms, lastEmittedSignals: Set(ChatSignal) })
      }),
      chatFlowPhase' = chatFlowPhase.set(sender, ChatMessagesExchanged),
      allAgents' = allAgents,
      invitations' = invitations,
      guardianRequests' = guardianRequests,
      recoverySessions' = recoverySessions,
      recoveryFlowPhase' = recoveryFlowPhase,
      invitationFlowPhase' = invitationFlowPhase,
      currentDemoPhase' = currentDemoPhase
    }
  }

  // ============ RECOVERY FLOW ============

  action simulateDeviceLoss(agent: AgentId): bool = {
    val state = getAgent(agents, agent)
    all {
      state.hasAccount,
      agents' = agents.set(agent, {
        ...state,
        lastEmittedSignals: Set(RecoverySignal)
      }),
      recoveryFlowPhase' = recoveryFlowPhase.set(agent, RecoveryRequested),
      allAgents' = allAgents,
      invitations' = invitations,
      guardianRequests' = guardianRequests,
      recoverySessions' = recoverySessions,
      channels' = channels,
      chatFlowPhase' = chatFlowPhase,
      invitationFlowPhase' = invitationFlowPhase,
      currentDemoPhase' = currentDemoPhase
    }
  }

  action initiateRecovery(subject: AgentId, sessionId: RecoverySessionId, cooldown: int): bool = {
    val state = getAgent(agents, subject)
    all {
      state.hasAccount,
      state.guardians.size() >= 1,
      not(sessionId.in(recoverySessions.keys())),
      recoverySessions' = recoverySessions.put(sessionId, {
        id: sessionId,
        subject: subject,
        guardians: state.guardians,
        approvals: Set(),
        threshold: 2,
        cooldownRemaining: cooldown,
        phase: RecoveryApprovalsPending
      }),
      agents' = agents.set(subject, {
        ...state,
        activeRecoverySession: Some(sessionId),
        lastEmittedSignals: Set(RecoverySignal)
      }),
      recoveryFlowPhase' = recoveryFlowPhase.set(subject, RecoveryRequested),
      allAgents' = allAgents,
      invitations' = invitations,
      guardianRequests' = guardianRequests,
      channels' = channels,
      chatFlowPhase' = chatFlowPhase,
      invitationFlowPhase' = invitationFlowPhase,
      currentDemoPhase' = currentDemoPhase
    }
  }

  action approveRecovery(guardian: AgentId, sessionId: RecoverySessionId): bool = {
    val session = recoverySessions.get(sessionId)
    val isPending = match session.phase { | RecoveryApprovalsPending => true | _ => false }
    val guardianState = getAgent(agents, guardian)
    val updatedApprovals = session.approvals.union(Set(guardian))
    val thresholdReached = updatedApprovals.size() >= session.threshold
    val nextPhase = if (thresholdReached and session.cooldownRemaining > 0)
      RecoveryCooldownActive else RecoveryApprovalsPending
    all {
      sessionId.in(recoverySessions.keys()),
      isPending,
      session.guardians.contains(guardian),
      not(session.approvals.contains(guardian)),
      recoverySessions' = recoverySessions.put(sessionId, {
        ...session,
        approvals: updatedApprovals,
        phase: nextPhase
      }),
      agents' = agents.set(guardian, {
        ...guardianState,
        lastEmittedSignals: Set(RecoverySignal)
      }),
      recoveryFlowPhase' = if (thresholdReached)
        recoveryFlowPhase.set(session.subject, RecoveryApprovalsPending)
        else recoveryFlowPhase,
      allAgents' = allAgents,
      invitations' = invitations,
      guardianRequests' = guardianRequests,
      channels' = channels,
      chatFlowPhase' = chatFlowPhase,
      invitationFlowPhase' = invitationFlowPhase,
      currentDemoPhase' = currentDemoPhase
    }
  }

  action tickCooldown(sessionId: RecoverySessionId): bool = {
    val session = recoverySessions.get(sessionId)
    val isCooldown = match session.phase { | RecoveryCooldownActive => true | _ => false }
    val remaining = session.cooldownRemaining - 1
    val nextPhase = if (remaining <= 0) RecoverySharesSubmitted else RecoveryCooldownActive
    all {
      sessionId.in(recoverySessions.keys()),
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
      allAgents' = allAgents,
      agents' = agents,
      invitations' = invitations,
      guardianRequests' = guardianRequests,
      channels' = channels,
      chatFlowPhase' = chatFlowPhase,
      invitationFlowPhase' = invitationFlowPhase,
      currentDemoPhase' = currentDemoPhase
    }
  }

  action completeRecovery(sessionId: RecoverySessionId): bool = {
    val session = recoverySessions.get(sessionId)
    val isSharesSubmitted = match session.phase { | RecoverySharesSubmitted => true | _ => false }
    val subjectState = getAgent(agents, session.subject)
    all {
      sessionId.in(recoverySessions.keys()),
      isSharesSubmitted,
      session.approvals.size() >= session.threshold,
      recoverySessions' = recoverySessions.put(sessionId, {
        ...session,
        phase: RecoveryCompleted
      }),
      agents' = agents.set(session.subject, {
        ...subjectState,
        activeRecoverySession: None,
        lastEmittedSignals: Set(RecoverySignal, ContactsSignal)
      }),
      recoveryFlowPhase' = recoveryFlowPhase.set(session.subject, RecoveryCompleted),
      allAgents' = allAgents,
      invitations' = invitations,
      guardianRequests' = guardianRequests,
      channels' = channels,
      chatFlowPhase' = chatFlowPhase,
      invitationFlowPhase' = invitationFlowPhase,
      currentDemoPhase' = currentDemoPhase
    }
  }

  // ============ DEEP VALIDATION INVARIANTS ============

  val guardiansAreContacts: bool =
    allAgents.forall(agent => {
      val state = getAgent(agents, agent)
      state.guardians.forall(g => state.contacts.contains(g))
    })

  val guardianBidirectionality: bool =
    allAgents.forall(agent => {
      val state = getAgent(agents, agent)
      state.guardians.forall(g => {
        val guardianState = getAgent(agents, g)
        guardianState.guardianFor.contains(agent)
      })
    })

  val acceptedRequestsCreateRelationships: bool =
    guardianRequests.keys().forall(rid => {
      val request = guardianRequests.get(rid)
      if (request.isAccepted) {
        val requesterState = getAgent(agents, request.requester)
        val guardianState = getAgent(agents, request.guardian)
        requesterState.guardians.contains(request.guardian) and
        guardianState.guardianFor.contains(request.requester)
      } else true
    })

  val demoInvariants: bool =
    guardiansAreContacts and
    guardianBidirectionality and
    acceptedRequestsCreateRelationships

  // ============ FULL CLI RECOVERY DEMO SCENARIO ============

  run fullCliRecoveryDemoTest = {
    init

    // ========== PHASE 1: ALICE & CAROL SETUP ==========
    .then(createAccount("alice"))
    .then(createAccount("carol"))
    .then(setDemoPhase(PhaseAliceCarolSetup))

    // ========== PHASE 2: BOB ONBOARDING ==========
    .then(createAccount("bob"))

    // Step 2a: Alice creates contact invitation for Bob
    .then(createContactInvitation("alice", "alice-to-bob-contact"))
    .then(exportInvitation("alice-to-bob-contact"))

    // Step 2b: Bob imports and accepts Alice's invitation
    .then(importInvitation("bob", "alice-to-bob-contact"))
    .then(acceptContactInvitation("bob", "alice-to-bob-contact"))

    // DEEP VALIDATION: Alice is now Bob's contact
    .then(all {
      assert(getAgent(agents, "bob").contacts.contains("alice")),
      assert(getAgent(agents, "alice").contacts.contains("bob")),
      assert(not(getAgent(agents, "bob").guardians.contains("alice"))),
      allAgents' = allAgents,
      agents' = agents,
      invitations' = invitations,
      guardianRequests' = guardianRequests,
      recoverySessions' = recoverySessions,
      channels' = channels,
      recoveryFlowPhase' = recoveryFlowPhase,
      chatFlowPhase' = chatFlowPhase,
      invitationFlowPhase' = invitationFlowPhase,
      currentDemoPhase' = currentDemoPhase
    })

    // Step 2c: Carol creates contact invitation for Bob
    .then(createContactInvitation("carol", "carol-to-bob-contact"))
    .then(exportInvitation("carol-to-bob-contact"))

    // Step 2d: Bob imports and accepts Carol's invitation
    .then(importInvitation("bob", "carol-to-bob-contact"))
    .then(acceptContactInvitation("bob", "carol-to-bob-contact"))

    // DEEP VALIDATION: Carol is now Bob's contact
    .then(all {
      assert(getAgent(agents, "bob").contacts.contains("carol")),
      assert(getAgent(agents, "carol").contacts.contains("bob")),
      assert(not(getAgent(agents, "bob").guardians.contains("carol"))),
      allAgents' = allAgents,
      agents' = agents,
      invitations' = invitations,
      guardianRequests' = guardianRequests,
      recoverySessions' = recoverySessions,
      channels' = channels,
      recoveryFlowPhase' = recoveryFlowPhase,
      chatFlowPhase' = chatFlowPhase,
      invitationFlowPhase' = invitationFlowPhase,
      currentDemoPhase' = currentDemoPhase
    })

    // Step 2e: Bob REQUESTS Alice to become a guardian
    .then(sendGuardianRequest("bob", "alice", "bob-requests-alice-guardian"))

    // Step 2f: Bob REQUESTS Carol to become a guardian
    .then(sendGuardianRequest("bob", "carol", "bob-requests-carol-guardian"))

    // Step 2g: Alice ACCEPTS the guardian request
    .then(acceptGuardianRequest("alice", "bob-requests-alice-guardian"))

    // DEEP VALIDATION: Alice is now BOTH contact AND guardian
    .then(all {
      assert(getAgent(agents, "bob").contacts.contains("alice")),
      assert(getAgent(agents, "bob").guardians.contains("alice")),
      assert(isContactGuardian(getAgent(agents, "bob"), "alice")),
      assert(getAgent(agents, "alice").guardianFor.contains("bob")),
      allAgents' = allAgents,
      agents' = agents,
      invitations' = invitations,
      guardianRequests' = guardianRequests,
      recoverySessions' = recoverySessions,
      channels' = channels,
      recoveryFlowPhase' = recoveryFlowPhase,
      chatFlowPhase' = chatFlowPhase,
      invitationFlowPhase' = invitationFlowPhase,
      currentDemoPhase' = currentDemoPhase
    })

    // Step 2h: Carol ACCEPTS the guardian request
    .then(acceptGuardianRequest("carol", "bob-requests-carol-guardian"))

    // DEEP VALIDATION: Carol is now BOTH contact AND guardian
    .then(all {
      assert(getAgent(agents, "bob").contacts.contains("carol")),
      assert(getAgent(agents, "bob").guardians.contains("carol")),
      assert(isContactGuardian(getAgent(agents, "bob"), "carol")),
      assert(getAgent(agents, "carol").guardianFor.contains("bob")),
      assert(getAgent(agents, "bob").guardians.size() == 2),
      assert(demoInvariants),
      allAgents' = allAgents,
      agents' = agents,
      invitations' = invitations,
      guardianRequests' = guardianRequests,
      recoverySessions' = recoverySessions,
      channels' = channels,
      recoveryFlowPhase' = recoveryFlowPhase,
      chatFlowPhase' = chatFlowPhase,
      invitationFlowPhase' = invitationFlowPhase,
      currentDemoPhase' = currentDemoPhase
    })

    .then(setDemoPhase(PhaseBobOnboarding))

    // ========== PHASE 3: GROUP CHAT SETUP ==========
    // Bob creates the guardian chat - he's the only one with both Alice and Carol as contacts
    .then(createGroupChannel("bob", "guardian-chat", "Guardian Chat", Set("alice", "carol")))
    .then(setDemoPhase(PhaseGroupChatSetup))

    // ========== PHASE 4: GROUP MESSAGING ==========
    .then(sendMessage("alice", "guardian-chat"))
    .then(sendMessage("bob", "guardian-chat"))
    .then(sendMessage("carol", "guardian-chat"))
    .then(setDemoPhase(PhaseGroupMessaging))

    .then(all {
      assert(channels.get("guardian-chat").messageCount == 3),
      allAgents' = allAgents,
      agents' = agents,
      invitations' = invitations,
      guardianRequests' = guardianRequests,
      recoverySessions' = recoverySessions,
      channels' = channels,
      recoveryFlowPhase' = recoveryFlowPhase,
      chatFlowPhase' = chatFlowPhase,
      invitationFlowPhase' = invitationFlowPhase,
      currentDemoPhase' = currentDemoPhase
    })

    // ========== PHASE 5: BOB ACCOUNT LOSS ==========
    .then(simulateDeviceLoss("bob"))
    .then(setDemoPhase(PhaseBobAccountLoss))

    // ========== PHASE 6: RECOVERY INITIATION ==========
    .then(initiateRecovery("bob", "bob-recovery-session", 1))
    .then(approveRecovery("alice", "bob-recovery-session"))
    .then(approveRecovery("carol", "bob-recovery-session"))
    .then(setDemoPhase(PhaseRecoveryInitiation))

    .then(all {
      assert(recoverySessions.get("bob-recovery-session").approvals.size() >= 2),
      assert(recoverySessions.get("bob-recovery-session").approvals.contains("alice")),
      assert(recoverySessions.get("bob-recovery-session").approvals.contains("carol")),
      allAgents' = allAgents,
      agents' = agents,
      invitations' = invitations,
      guardianRequests' = guardianRequests,
      recoverySessions' = recoverySessions,
      channels' = channels,
      recoveryFlowPhase' = recoveryFlowPhase,
      chatFlowPhase' = chatFlowPhase,
      invitationFlowPhase' = invitationFlowPhase,
      currentDemoPhase' = currentDemoPhase
    })

    // ========== PHASE 7: ACCOUNT RESTORATION ==========
    .then(tickCooldown("bob-recovery-session"))
    .then(completeRecovery("bob-recovery-session"))
    .then(setDemoPhase(PhaseAccountRestoration))

    .then(all {
      val phase = recoverySessions.get("bob-recovery-session").phase
      assert(match phase { | RecoveryCompleted => true | _ => false }),
      allAgents' = allAgents,
      agents' = agents,
      invitations' = invitations,
      guardianRequests' = guardianRequests,
      recoverySessions' = recoverySessions,
      channels' = channels,
      recoveryFlowPhase' = recoveryFlowPhase,
      chatFlowPhase' = chatFlowPhase,
      invitationFlowPhase' = invitationFlowPhase,
      currentDemoPhase' = currentDemoPhase
    })

    // ========== PHASE 8: POST-RECOVERY MESSAGING ==========
    .then(sendMessage("bob", "guardian-chat"))
    .then(setDemoPhase(PhasePostRecoveryMessaging))

    // ========== FINAL DEEP VALIDATION ==========
    .then(all {
      assert(getAgent(agents, "bob").guardians.contains("alice")),
      assert(getAgent(agents, "bob").guardians.contains("carol")),
      assert(getAgent(agents, "bob").contacts.contains("alice")),
      assert(getAgent(agents, "bob").contacts.contains("carol")),
      assert(isContactGuardian(getAgent(agents, "bob"), "alice")),
      assert(isContactGuardian(getAgent(agents, "bob"), "carol")),
      assert(getAgent(agents, "bob").channels.contains("guardian-chat")),
      assert(channels.get("guardian-chat").messageCount == 4),
      assert(demoInvariants),
      setDemoPhase(PhaseComplete)
    })
  }

  // ============ TARGETED TEST: Guardian Request Flow ============

  run guardianRequestFlowTest = {
    init
    .then(createAccount("alice"))
    .then(createAccount("bob"))
    .then(createContactInvitation("alice", "alice-bob-inv"))
    .then(exportInvitation("alice-bob-inv"))
    .then(importInvitation("bob", "alice-bob-inv"))
    .then(acceptContactInvitation("bob", "alice-bob-inv"))
    .then(all {
      assert(getAgent(agents, "bob").contacts.contains("alice")),
      assert(not(getAgent(agents, "bob").guardians.contains("alice"))),
      assert(not(isContactGuardian(getAgent(agents, "bob"), "alice"))),
      allAgents' = allAgents,
      agents' = agents,
      invitations' = invitations,
      guardianRequests' = guardianRequests,
      recoverySessions' = recoverySessions,
      channels' = channels,
      recoveryFlowPhase' = recoveryFlowPhase,
      chatFlowPhase' = chatFlowPhase,
      invitationFlowPhase' = invitationFlowPhase,
      currentDemoPhase' = currentDemoPhase
    })
    .then(sendGuardianRequest("bob", "alice", "bob-alice-guardian-req"))
    .then(acceptGuardianRequest("alice", "bob-alice-guardian-req"))
    .then(all {
      assert(getAgent(agents, "bob").contacts.contains("alice")),
      assert(getAgent(agents, "bob").guardians.contains("alice")),
      assert(isContactGuardian(getAgent(agents, "bob"), "alice")),
      assert(getAgent(agents, "alice").guardianFor.contains("bob")),
      assert(demoInvariants),
      allAgents' = allAgents,
      agents' = agents,
      invitations' = invitations,
      guardianRequests' = guardianRequests,
      recoverySessions' = recoverySessions,
      channels' = channels,
      recoveryFlowPhase' = recoveryFlowPhase,
      chatFlowPhase' = chatFlowPhase,
      invitationFlowPhase' = invitationFlowPhase,
      currentDemoPhase' = currentDemoPhase
    })
  }
}
