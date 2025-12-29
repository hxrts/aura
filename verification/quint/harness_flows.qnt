// Flow Test Harness
//
// Test driver for TUI flow verification.
// Wraps tui_flows actions for integration testing and ITF trace generation.
//
// Provides simplified scenarios for:
// - Guardian recovery flow
// - Block lifecycle flow
// - Neighborhood formation flow
// - Chat/messaging flow
// - Invitation lifecycle flow
// - Social graph flow (contacts + blocks)

module harness_flows {
  // ============ IMPORTS ============
  import tui_flows from "./tui_flows"

  // ============ TYPE ALIASES ============
  type AgentId = str
  type InvitationId = str
  type ChannelId = str
  type HomeId = str
  type NeighborhoodId = str
  type RecoverySessionId = str

  // ============ HARNESS STATE ============

  // Track which scenarios have been executed
  var executedScenarios: Set[str]

  // Track scenario outcomes for assertion
  var scenarioOutcomes: str -> bool

  // ============ INITIALIZATION ============

  action init = all {
    tui_flows::init,
    executedScenarios' = Set(),
    scenarioOutcomes' = Map()
  }

  // ============ HELPER ACTIONS ============

  // Keep harness state unchanged
  action harnessUnchanged = all {
    executedScenarios' = executedScenarios,
    scenarioOutcomes' = scenarioOutcomes
  }

  // Mark scenario as executed with outcome
  action markScenario(name: str, success: bool): bool = all {
    executedScenarios' = executedScenarios.union(Set(name)),
    scenarioOutcomes' = scenarioOutcomes.put(name, success)
  }

  // ============ SCENARIO: GUARDIAN RECOVERY ============

  // Setup: Create accounts for bob (subject), alice, carol (guardians)
  action setupGuardianRecovery = all {
    tui_flows::createAccountWithGuardians("bob", Set("alice", "carol"), 2),
    harnessUnchanged
  }

  // Setup guardians: Create accounts for alice and carol
  action setupGuardians = all {
    tui_flows::createAccountWithGuardians("alice", Set("bob", "carol"), 2),
    harnessUnchanged
  }

  action setupCarol = all {
    tui_flows::createAccountWithGuardians("carol", Set("bob", "alice"), 2),
    harnessUnchanged
  }

  // Initiate recovery for bob
  action initiateRecoveryForBob(sessionId: RecoverySessionId, cooldown: int): bool = all {
    tui_flows::initiateRecovery("bob", sessionId, cooldown),
    harnessUnchanged
  }

  // Alice approves recovery
  action aliceApproves(sessionId: RecoverySessionId): bool = all {
    tui_flows::approveRecovery("alice", sessionId),
    harnessUnchanged
  }

  // Carol approves recovery
  action carolApproves(sessionId: RecoverySessionId): bool = all {
    tui_flows::approveRecovery("carol", sessionId),
    harnessUnchanged
  }

  // Tick cooldown
  action tickCooldown(sessionId: RecoverySessionId): bool = all {
    tui_flows::tickRecoveryCooldown(sessionId),
    harnessUnchanged
  }

  // Complete recovery
  action completeRecoverySession(sessionId: RecoverySessionId): bool = all {
    tui_flows::completeRecovery(sessionId),
    markScenario("guardian_recovery", true)
  }

  // ============ SCENARIO: INVITATION FLOW ============

  // Alice creates invitation
  action aliceCreatesInvitation(invId: InvitationId): bool = all {
    tui_flows::createInvitation("alice", invId, tui_flows::ContactInvitation),
    harnessUnchanged
  }

  // Export invitation
  action exportInv(invId: InvitationId): bool = all {
    tui_flows::exportInvitation(invId),
    harnessUnchanged
  }

  // Bob imports invitation
  action bobImportsInvitation(invId: InvitationId): bool = all {
    tui_flows::importInvitation("bob", invId),
    harnessUnchanged
  }

  // Bob accepts invitation
  action bobAcceptsInvitation(invId: InvitationId): bool = all {
    tui_flows::acceptInvitation("bob", invId),
    markScenario("invitation_accept", true)
  }

  // Bob declines invitation
  action bobDeclinesInvitation(invId: InvitationId): bool = all {
    tui_flows::declineInvitation("bob", invId),
    markScenario("invitation_decline", true)
  }

  // ============ SCENARIO: CHAT FLOW ============

  // Bob starts DM with alice
  action bobStartsDMWithAlice = all {
    tui_flows::startDirectChat("bob", "alice"),
    harnessUnchanged
  }

  // Bob creates channel
  action bobCreatesChannel(channelId: ChannelId, name: str): bool = all {
    tui_flows::createChannel("bob", channelId, name, Set("alice")),
    harnessUnchanged
  }

  // Bob sends message
  action bobSendsMessage(channelId: ChannelId): bool = all {
    tui_flows::sendMessage("bob", channelId),
    markScenario("chat_message", true)
  }

  // ============ SCENARIO: BLOCK FLOW ============

  // Bob creates block
  action bobCreatesBlock(blockId: HomeId): bool = all {
    tui_flows::createBlock("bob", blockId),
    harnessUnchanged
  }

  // Alice joins block
  action aliceJoinsBlock(blockId: HomeId): bool = all {
    tui_flows::joinBlock("alice", blockId),
    harnessUnchanged
  }

  // Bob promotes alice to steward
  action bobPromotesAlice(blockId: HomeId): bool = all {
    tui_flows::promoteSteward("bob", blockId, "alice"),
    markScenario("block_steward_promotion", true)
  }

  // ============ SCENARIO: NEIGHBORHOOD FLOW ============

  // Bob creates neighborhood
  action bobCreatesNeighborhood(neighborhoodId: NeighborhoodId, name: str): bool = all {
    tui_flows::createNeighborhood("bob", neighborhoodId, name),
    harnessUnchanged
  }

  // Bob links block to neighborhood
  action bobLinksBlock(blockId: HomeId, neighborhoodId: NeighborhoodId): bool = all {
    tui_flows::linkBlockToNeighborhood("bob", blockId, neighborhoodId),
    markScenario("neighborhood_link", true)
  }

  // ============ SCENARIO: SOCIAL GRAPH FLOW ============

  // Bob sets nickname for alice
  action bobSetsNicknameForAlice(nickname: str): bool = all {
    tui_flows::setNickname("bob", "alice", nickname),
    harnessUnchanged
  }

  // Bob invites alice to block
  action bobInvitesAliceToBlock(blockId: HomeId): bool = all {
    tui_flows::inviteContactToBlock("bob", blockId, "alice"),
    harnessUnchanged
  }

  // Bob removes alice from block
  action bobRemovesAliceFromBlock(blockId: HomeId): bool = all {
    tui_flows::removeContactFromBlock("bob", blockId, "alice"),
    harnessUnchanged
  }

  // Bob filters contacts by block
  action bobFiltersContactsByBlock(blockId: HomeId): bool = all {
    tui_flows::filterContactsByBlock("bob", blockId),
    markScenario("social_graph_filter", true)
  }

  // Mark social graph scenario complete
  action markSocialGraphComplete: bool = all {
    tui_flows::allAgents' = tui_flows::allAgents,
    tui_flows::agents' = tui_flows::agents,
    tui_flows::invitations' = tui_flows::invitations,
    tui_flows::recoverySessions' = tui_flows::recoverySessions,
    tui_flows::blocks' = tui_flows::blocks,
    tui_flows::channels' = tui_flows::channels,
    tui_flows::neighborhoods' = tui_flows::neighborhoods,
    tui_flows::recoveryFlowPhase' = tui_flows::recoveryFlowPhase,
    tui_flows::blockFlowPhase' = tui_flows::blockFlowPhase,
    tui_flows::neighborhoodFlowPhase' = tui_flows::neighborhoodFlowPhase,
    tui_flows::chatFlowPhase' = tui_flows::chatFlowPhase,
    tui_flows::invitationFlowPhase' = tui_flows::invitationFlowPhase,
    tui_flows::socialGraphFlowPhase' = tui_flows::socialGraphFlowPhase,
    markScenario("social_graph_complete", true)
  }

  // ============ COMPOSITE SCENARIOS ============

  // Full guardian recovery scenario
  run fullGuardianRecoveryScenario = {
    init
    .then(setupGuardianRecovery)
    .then(setupGuardians)
    .then(setupCarol)
    .then(initiateRecoveryForBob("recovery-harness", 1))
    .then(aliceApproves("recovery-harness"))
    .then(carolApproves("recovery-harness"))
    .then(tickCooldown("recovery-harness"))
    .then(completeRecoverySession("recovery-harness"))
    .then(all {
      assert(executedScenarios.contains("guardian_recovery")),
      assert(scenarioOutcomes.get("guardian_recovery") == true),
      tui_flows::allAgents' = tui_flows::allAgents,
      tui_flows::agents' = tui_flows::agents,
      tui_flows::invitations' = tui_flows::invitations,
      tui_flows::recoverySessions' = tui_flows::recoverySessions,
      tui_flows::blocks' = tui_flows::blocks,
      tui_flows::channels' = tui_flows::channels,
      tui_flows::neighborhoods' = tui_flows::neighborhoods,
      tui_flows::recoveryFlowPhase' = tui_flows::recoveryFlowPhase,
      tui_flows::blockFlowPhase' = tui_flows::blockFlowPhase,
      tui_flows::neighborhoodFlowPhase' = tui_flows::neighborhoodFlowPhase,
      tui_flows::chatFlowPhase' = tui_flows::chatFlowPhase,
      tui_flows::invitationFlowPhase' = tui_flows::invitationFlowPhase,
      tui_flows::socialGraphFlowPhase' = tui_flows::socialGraphFlowPhase,
      executedScenarios' = executedScenarios,
      scenarioOutcomes' = scenarioOutcomes
    })
  }

  // Full invitation → chat scenario
  run fullInvitationChatScenario = {
    init
    .then(setupGuardianRecovery)
    .then(setupGuardians)
    .then(aliceCreatesInvitation("invite-harness"))
    .then(exportInv("invite-harness"))
    .then(bobImportsInvitation("invite-harness"))
    .then(bobAcceptsInvitation("invite-harness"))
    .then(bobStartsDMWithAlice)
    .then(bobCreatesChannel("channel-harness", "Guardians"))
    .then(bobSendsMessage("channel-harness"))
    .then(all {
      assert(executedScenarios.contains("invitation_accept")),
      assert(executedScenarios.contains("chat_message")),
      tui_flows::allAgents' = tui_flows::allAgents,
      tui_flows::agents' = tui_flows::agents,
      tui_flows::invitations' = tui_flows::invitations,
      tui_flows::recoverySessions' = tui_flows::recoverySessions,
      tui_flows::blocks' = tui_flows::blocks,
      tui_flows::channels' = tui_flows::channels,
      tui_flows::neighborhoods' = tui_flows::neighborhoods,
      tui_flows::recoveryFlowPhase' = tui_flows::recoveryFlowPhase,
      tui_flows::blockFlowPhase' = tui_flows::blockFlowPhase,
      tui_flows::neighborhoodFlowPhase' = tui_flows::neighborhoodFlowPhase,
      tui_flows::chatFlowPhase' = tui_flows::chatFlowPhase,
      tui_flows::invitationFlowPhase' = tui_flows::invitationFlowPhase,
      tui_flows::socialGraphFlowPhase' = tui_flows::socialGraphFlowPhase,
      executedScenarios' = executedScenarios,
      scenarioOutcomes' = scenarioOutcomes
    })
  }

  // Full block → neighborhood scenario
  run fullBlockNeighborhoodScenario = {
    init
    .then(setupGuardianRecovery)
    .then(setupGuardians)
    .then(bobCreatesBlock("block-harness"))
    .then(aliceJoinsBlock("block-harness"))
    .then(bobPromotesAlice("block-harness"))
    .then(bobCreatesNeighborhood("neighborhood-harness", "Downtown"))
    .then(bobLinksBlock("block-harness", "neighborhood-harness"))
    .then(all {
      assert(executedScenarios.contains("block_steward_promotion")),
      assert(executedScenarios.contains("neighborhood_link")),
      tui_flows::allAgents' = tui_flows::allAgents,
      tui_flows::agents' = tui_flows::agents,
      tui_flows::invitations' = tui_flows::invitations,
      tui_flows::recoverySessions' = tui_flows::recoverySessions,
      tui_flows::blocks' = tui_flows::blocks,
      tui_flows::channels' = tui_flows::channels,
      tui_flows::neighborhoods' = tui_flows::neighborhoods,
      tui_flows::recoveryFlowPhase' = tui_flows::recoveryFlowPhase,
      tui_flows::blockFlowPhase' = tui_flows::blockFlowPhase,
      tui_flows::neighborhoodFlowPhase' = tui_flows::neighborhoodFlowPhase,
      tui_flows::chatFlowPhase' = tui_flows::chatFlowPhase,
      tui_flows::invitationFlowPhase' = tui_flows::invitationFlowPhase,
      tui_flows::socialGraphFlowPhase' = tui_flows::socialGraphFlowPhase,
      executedScenarios' = executedScenarios,
      scenarioOutcomes' = scenarioOutcomes
    })
  }

  // Full social graph scenario: contact_import → block_creation → contact_block_assignment → filter
  // This covers Flow 6: Social Graph (Contacts + Blocks)
  // Note: Uses invitation flow to establish contact relationship before Social Graph operations
  run fullSocialGraphScenario = {
    init
    // Setup: Create accounts for bob and alice
    .then(setupGuardianRecovery)  // Creates bob account
    .then(setupGuardians)          // Creates alice account
    // Establish contact via invitation (alice -> bob)
    .then(aliceCreatesInvitation("social-invite"))
    .then(exportInv("social-invite"))
    .then(bobImportsInvitation("social-invite"))
    .then(bobAcceptsInvitation("social-invite"))  // Now alice is bob's contact
    // Bob creates a block
    .then(bobCreatesBlock("social-block"))
    // Bob sets nickname for alice (alice is now a contact!)
    .then(bobSetsNicknameForAlice("My Friend Alice"))
    // Bob invites alice to his block (alice becomes a resident)
    .then(bobInvitesAliceToBlock("social-block"))
    // Bob filters contacts by block to view only block members
    .then(bobFiltersContactsByBlock("social-block"))
    // Complete the scenario
    .then(markSocialGraphComplete)
    .then(all {
      assert(executedScenarios.contains("social_graph_filter")),
      assert(executedScenarios.contains("social_graph_complete")),
      assert(scenarioOutcomes.get("social_graph_complete") == true),
      // Verify state: alice should be a resident of the block
      assert(tui_flows::blocks.get("social-block").residents.contains("alice")),
      // Verify nickname was set
      assert(tui_flows::agents.get("bob").contactNicknames.get("alice") == "My Friend Alice"),
      tui_flows::allAgents' = tui_flows::allAgents,
      tui_flows::agents' = tui_flows::agents,
      tui_flows::invitations' = tui_flows::invitations,
      tui_flows::recoverySessions' = tui_flows::recoverySessions,
      tui_flows::blocks' = tui_flows::blocks,
      tui_flows::channels' = tui_flows::channels,
      tui_flows::neighborhoods' = tui_flows::neighborhoods,
      tui_flows::recoveryFlowPhase' = tui_flows::recoveryFlowPhase,
      tui_flows::blockFlowPhase' = tui_flows::blockFlowPhase,
      tui_flows::neighborhoodFlowPhase' = tui_flows::neighborhoodFlowPhase,
      tui_flows::chatFlowPhase' = tui_flows::chatFlowPhase,
      tui_flows::invitationFlowPhase' = tui_flows::invitationFlowPhase,
      tui_flows::socialGraphFlowPhase' = tui_flows::socialGraphFlowPhase,
      executedScenarios' = executedScenarios,
      scenarioOutcomes' = scenarioOutcomes
    })
  }

  // ============ INVARIANTS ============

  // All executed scenarios should have outcomes
  val InvariantScenariosHaveOutcomes =
    executedScenarios.forall(s => s.in(scenarioOutcomes.keys()))

  // Successful scenarios should be marked true
  val InvariantSuccessfulScenarios =
    executedScenarios.forall(s =>
      if (s.in(scenarioOutcomes.keys())) scenarioOutcomes.get(s) else true
    )
}
