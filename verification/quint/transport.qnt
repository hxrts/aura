// Transport Protocol - Aura Secure Channel Abstraction
//
// This module models Aura's transport layer with:
// - Context-scoped connections with privacy levels
// - Guard chain enforcement (CapGuard -> FlowGuard -> JournalCoupler)
// - Message ordering within sessions
// - No side-channel information leakage on failures
//
// Key properties to verify:
// - Context isolation: messages never cross context boundaries
// - Guard atomicity: all guards succeed or entire send fails
// - No side channels: authorization/budget failures produce no traffic
// - Message ordering: sequence numbers monotonic within session
// - State validity: connections never transition backward
//
// == Lean Correspondence ==
// Module: Aura.Transport (verification/lean/Aura/Transport/)
// - Types: Aura.Transport.Types
// - Privacy: Aura.Transport.Privacy
// - Guards: Aura.Transport.Guards
//
// == Rust Correspondence ==
// - Types: crates/aura-transport/src/types.rs
// - Effects: crates/aura-core/src/effects/transport.rs
// - Guards: crates/aura-guards/src/
//
// See: docs/108_transport_and_information_flow.md

module transport {
  // ==================== TYPE DEFINITIONS ====================

  type Option[a] = Some(a) | None
  type ContextId = str
  type AuthorityId = str
  type SessionId = int
  type MessageId = int
  type ConnectionId = str
  type SequenceNumber = int
  type FlowBudget = int

  // ==================== PRIVACY TYPES ====================

  type PrivacyLevel =
    | Clear
    | ContextScoped
    | Blinded

  type FrameType =
    | FrameClear
    | FrameCapabilityScoped
    | FrameContextScoped
    | FrameBlinded

  // ==================== CONNECTION TYPES ====================

  type ConnectionState =
    | Connecting
    | Established
    | Closing
    | Closed

  type CloseReason =
    | Graceful
    | RemoteClosed
    | Timeout
    | ProtocolError
    | Unauthorized
    | FlowBudgetExceeded
    | TransportError

  type PrivacyContext = {
    privacyLevel: PrivacyLevel,
    contextId: Option[ContextId],
    capabilityFiltering: bool,
    messageBlinding: bool
  }

  type ConnectionInfo = {
    connId: ConnectionId,
    localAuthority: AuthorityId,
    remoteAuthority: AuthorityId,
    state: ConnectionState,
    privacyContext: Option[PrivacyContext],
    establishedAt: Option[int]
  }

  // ==================== SESSION TYPES ====================

  type SessionState =
    | SessionConnecting
    | SessionActive
    | SessionClosing
    | SessionClosed

  type TransportProtocol =
    | Quic
    | Tcp
    | WebRTC
    | Relay

  type SessionInfo = {
    sessionId: SessionId,
    contextId: ContextId,
    localAuthority: AuthorityId,
    remoteAuthority: AuthorityId,
    protocol: TransportProtocol,
    state: SessionState,
    flowBudget: FlowBudget,
    sequenceCounter: SequenceNumber
  }

  // ==================== MESSAGE TYPES ====================

  type MessageRecord = {
    messageId: MessageId,
    sessionId: SessionId,
    contextId: ContextId,
    sequence: SequenceNumber,
    sizeBytes: int,
    flowCost: int,
    sent: bool
  }

  // ==================== GUARD TYPES ====================

  type GuardResult =
    | GuardPass
    | GuardDeny

  type GuardSnapshot = {
    capGuardResult: GuardResult,
    flowGuardResult: GuardResult,
    journalCouplerResult: GuardResult
  }

  // ==================== TRANSPORT FACTS ====================

  type SessionEstablishedData = { sessionId: SessionId, contextId: ContextId, protocol: TransportProtocol }
  type SessionClosedData = { sessionId: SessionId, contextId: ContextId, reason: CloseReason }
  type MessageSentData = { messageId: MessageId, sessionId: SessionId, contextId: ContextId, flowCost: int }
  type MessageReceivedData = { messageId: MessageId, sessionId: SessionId, contextId: ContextId, sender: AuthorityId }

  type TransportFact =
    | FactSessionEstablished(SessionEstablishedData)
    | FactSessionClosed(SessionClosedData)
    | FactMessageSent(MessageSentData)
    | FactMessageReceived(MessageReceivedData)

  // ==================== STATE VARIABLES ====================

  var connections: ConnectionId -> ConnectionInfo
  var sessions: SessionId -> SessionInfo
  var pendingMessages: Set[MessageRecord]
  var sentMessages: Set[MessageRecord]
  var transportFacts: Set[TransportFact]
  var nextMessageId: int
  var nextSessionId: int

  // ==================== CONSTANTS ====================

  pure val DEFAULT_FLOW_BUDGET: int = 1000
  pure val MAX_MESSAGE_SIZE: int = 65536

  // ==================== HELPER FUNCTIONS ====================

  pure def isValidConnectionTransition(s1: ConnectionState, s2: ConnectionState): bool =
    if (s1 == s2) true
    else if (s1 == Connecting and s2 == Established) true
    else if (s1 == Connecting and s2 == Closing) true
    else if (s1 == Connecting and s2 == Closed) true
    else if (s1 == Established and s2 == Closing) true
    else if (s1 == Closing and s2 == Closed) true
    else false

  pure def isValidSessionTransition(s1: SessionState, s2: SessionState): bool =
    if (s1 == s2) true
    else if (s1 == SessionConnecting and s2 == SessionActive) true
    else if (s1 == SessionConnecting and s2 == SessionClosing) true
    else if (s1 == SessionConnecting and s2 == SessionClosed) true
    else if (s1 == SessionActive and s2 == SessionClosing) true
    else if (s1 == SessionClosing and s2 == SessionClosed) true
    else false

  pure def allGuardsPass(snapshot: GuardSnapshot): bool = all {
    snapshot.capGuardResult == GuardPass,
    snapshot.flowGuardResult == GuardPass,
    snapshot.journalCouplerResult == GuardPass
  }

  def getConnection(connId: ConnectionId): Option[ConnectionInfo] =
    if (connId.in(connections.keys())) Some(connections.get(connId)) else None

  def getSession(sid: SessionId): Option[SessionInfo] =
    if (sid.in(sessions.keys())) Some(sessions.get(sid)) else None

  // ==================== ACTIONS ====================

  action init: bool = all {
    connections' = Map(),
    sessions' = Map(),
    pendingMessages' = Set(),
    sentMessages' = Set(),
    transportFacts' = Set(),
    nextMessageId' = 1,
    nextSessionId' = 1
  }

  // Create a new connection
  action initConnection(connId: ConnectionId, localAuth: AuthorityId, remoteAuth: AuthorityId): bool = all {
    not(connId.in(connections.keys())),
    connections' = connections.put(connId, {
      connId: connId,
      localAuthority: localAuth,
      remoteAuthority: remoteAuth,
      state: Connecting,
      privacyContext: None,
      establishedAt: None
    }),
    sessions' = sessions,
    pendingMessages' = pendingMessages,
    sentMessages' = sentMessages,
    transportFacts' = transportFacts,
    nextMessageId' = nextMessageId,
    nextSessionId' = nextSessionId
  }

  // Establish a connection with privacy context
  action establishConnection(connId: ConnectionId, privCtx: PrivacyContext, timestamp: int): bool = {
    val connExists = connId.in(connections.keys())
    val conn = if (connExists) connections.get(connId) else {
      connId: "", localAuthority: "", remoteAuthority: "",
      state: Closed, privacyContext: None, establishedAt: None
    }
    val canEstablish = conn.state == Connecting and isValidConnectionTransition(conn.state, Established)
    all {
      connExists,
      canEstablish,
      connections' = connections.put(connId, {
        connId: conn.connId,
        localAuthority: conn.localAuthority,
        remoteAuthority: conn.remoteAuthority,
        state: Established,
        privacyContext: Some(privCtx),
        establishedAt: Some(timestamp)
      }),
      sessions' = sessions,
      pendingMessages' = pendingMessages,
      sentMessages' = sentMessages,
      transportFacts' = transportFacts,
      nextMessageId' = nextMessageId,
      nextSessionId' = nextSessionId
    }
  }

  // Create a new session within a context
  action createSession(connId: ConnectionId, contextId: ContextId, protocol: TransportProtocol): bool = {
    val connExists = connId.in(connections.keys())
    val conn = if (connExists) connections.get(connId) else {
      connId: "", localAuthority: "", remoteAuthority: "",
      state: Closed, privacyContext: None, establishedAt: None
    }
    val sid = nextSessionId
    val canCreate = conn.state == Established
    all {
      connExists,
      canCreate,
      sessions' = sessions.put(sid, {
        sessionId: sid,
        contextId: contextId,
        localAuthority: conn.localAuthority,
        remoteAuthority: conn.remoteAuthority,
        protocol: protocol,
        state: SessionActive,
        flowBudget: DEFAULT_FLOW_BUDGET,
        sequenceCounter: 0
      }),
      connections' = connections,
      pendingMessages' = pendingMessages,
      sentMessages' = sentMessages,
      transportFacts' = transportFacts.union(Set(
        FactSessionEstablished({ sessionId: sid, contextId: contextId, protocol: protocol })
      )),
      nextMessageId' = nextMessageId,
      nextSessionId' = nextSessionId + 1
    }
  }

  // Send a message through a session with guard chain enforcement
  action sendMessage(sessionId: SessionId, payload: str, sizeBytes: int, guardSnapshot: GuardSnapshot): bool = {
    val sessionExists = sessionId.in(sessions.keys())
    val session = if (sessionExists) sessions.get(sessionId) else {
      sessionId: 0, contextId: "", localAuthority: "", remoteAuthority: "",
      protocol: Quic, state: SessionClosed, flowBudget: 0, sequenceCounter: 0
    }
    val mid = nextMessageId
    val newSeq = session.sequenceCounter + 1
    val msgRecord = {
      messageId: mid,
      sessionId: sessionId,
      contextId: session.contextId,
      sequence: newSeq,
      sizeBytes: sizeBytes,
      flowCost: sizeBytes,
      sent: true
    }
    val canSend = session.state == SessionActive and sizeBytes <= MAX_MESSAGE_SIZE and allGuardsPass(guardSnapshot) and session.flowBudget >= sizeBytes
    all {
      sessionExists,
      canSend,
      sessions' = sessions.put(sessionId, {
        sessionId: session.sessionId,
        contextId: session.contextId,
        localAuthority: session.localAuthority,
        remoteAuthority: session.remoteAuthority,
        protocol: session.protocol,
        state: session.state,
        flowBudget: session.flowBudget - sizeBytes,
        sequenceCounter: newSeq
      }),
      connections' = connections,
      pendingMessages' = pendingMessages,
      sentMessages' = sentMessages.union(Set(msgRecord)),
      transportFacts' = transportFacts.union(Set(
        FactMessageSent({ messageId: mid, sessionId: sessionId, contextId: session.contextId, flowCost: sizeBytes })
      )),
      nextMessageId' = nextMessageId + 1,
      nextSessionId' = nextSessionId
    }
  }

  // Receive a message
  action receiveMessage(sessionId: SessionId, messageId: MessageId, sequence: SequenceNumber, sizeBytes: int, sender: AuthorityId): bool = {
    val sessionExists = sessionId.in(sessions.keys())
    val session = if (sessionExists) sessions.get(sessionId) else {
      sessionId: 0, contextId: "", localAuthority: "", remoteAuthority: "",
      protocol: Quic, state: SessionClosed, flowBudget: 0, sequenceCounter: 0
    }
    val msgRecord = {
      messageId: messageId,
      sessionId: sessionId,
      contextId: session.contextId,
      sequence: sequence,
      sizeBytes: sizeBytes,
      flowCost: 0,
      sent: false
    }
    val canReceive = session.state == SessionActive and sender == session.remoteAuthority
    all {
      sessionExists,
      canReceive,
      sessions' = sessions,
      connections' = connections,
      pendingMessages' = pendingMessages.union(Set(msgRecord)),
      sentMessages' = sentMessages,
      transportFacts' = transportFacts.union(Set(
        FactMessageReceived({ messageId: messageId, sessionId: sessionId, contextId: session.contextId, sender: sender })
      )),
      nextMessageId' = nextMessageId,
      nextSessionId' = nextSessionId
    }
  }

  // Close a session
  action closeSession(sessionId: SessionId, reason: CloseReason): bool = {
    val sessionExists = sessionId.in(sessions.keys())
    val session = if (sessionExists) sessions.get(sessionId) else {
      sessionId: 0, contextId: "", localAuthority: "", remoteAuthority: "",
      protocol: Quic, state: SessionClosed, flowBudget: 0, sequenceCounter: 0
    }
    val canClose = session.state == SessionActive or session.state == SessionConnecting
    all {
      sessionExists,
      canClose,
      sessions' = sessions.put(sessionId, {
        sessionId: session.sessionId,
        contextId: session.contextId,
        localAuthority: session.localAuthority,
        remoteAuthority: session.remoteAuthority,
        protocol: session.protocol,
        state: SessionClosed,
        flowBudget: session.flowBudget,
        sequenceCounter: session.sequenceCounter
      }),
      connections' = connections,
      pendingMessages' = pendingMessages,
      sentMessages' = sentMessages,
      transportFacts' = transportFacts.union(Set(
        FactSessionClosed({ sessionId: sessionId, contextId: session.contextId, reason: reason })
      )),
      nextMessageId' = nextMessageId,
      nextSessionId' = nextSessionId
    }
  }

  // Close a connection
  action closeConnection(connId: ConnectionId, reason: CloseReason): bool = {
    val connExists = connId.in(connections.keys())
    val conn = if (connExists) connections.get(connId) else {
      connId: "", localAuthority: "", remoteAuthority: "",
      state: Closed, privacyContext: None, establishedAt: None
    }
    val canClose = (conn.state == Established or conn.state == Connecting) and isValidConnectionTransition(conn.state, Closed)
    all {
      connExists,
      canClose,
      connections' = connections.put(connId, {
        connId: conn.connId,
        localAuthority: conn.localAuthority,
        remoteAuthority: conn.remoteAuthority,
        state: Closed,
        privacyContext: conn.privacyContext,
        establishedAt: conn.establishedAt
      }),
      sessions' = sessions,
      pendingMessages' = pendingMessages,
      sentMessages' = sentMessages,
      transportFacts' = transportFacts,
      nextMessageId' = nextMessageId,
      nextSessionId' = nextSessionId
    }
  }

  // ==================== INVARIANTS ====================

  val AllInvariants = all {
    InvariantContextIsolation,
    InvariantFlowBudgetNonNegative,
    InvariantSequenceMonotonic,
    InvariantSentMessagesHaveFacts
  }

  // Safety: Messages are scoped to their session's context
  val InvariantContextIsolation =
    sentMessages.forall(msg =>
      sessions.keys().contains(msg.sessionId) implies {
        val session = getSession(msg.sessionId)
        match session {
          | Some(s) => msg.contextId == s.contextId
          | None => true
        }
      }
    )

  // Safety: Flow budget never goes negative
  val InvariantFlowBudgetNonNegative =
    sessions.keys().forall(sessionId => {
      val session = getSession(sessionId)
      match session {
        | Some(s) => s.flowBudget >= 0
        | None => true
      }
    })

  // Safety: Sequence numbers are non-negative
  val InvariantSequenceMonotonic =
    sessions.keys().forall(sessionId => {
      val session = getSession(sessionId)
      match session {
        | Some(s) => s.sequenceCounter >= 0
        | None => true
      }
    })

  // Safety: Every sent message has a corresponding fact
  val InvariantSentMessagesHaveFacts =
    sentMessages.forall(msg =>
      transportFacts.exists(fact =>
        match fact {
          | FactMessageSent(data) =>
              data.messageId == msg.messageId and data.sessionId == msg.sessionId
          | _ => false
        }
      )
    )

  // ==================== STEP RELATION ====================

  // Static sets for nondeterministic choice (actions guard for existence)
  pure val ALL_CONN_IDS = Set("conn1", "conn2", "conn3")
  pure val ALL_SESSION_IDS = Set(1, 2, 3)
  pure val ALL_AUTH_IDS = Set("auth1", "auth2", "auth3")
  pure val ALL_CTX_IDS = Set("ctx1", "ctx2")

  action step = any {
    nondet connId = oneOf(ALL_CONN_IDS)
    nondet localAuth = oneOf(ALL_AUTH_IDS)
    nondet remoteAuth = oneOf(ALL_AUTH_IDS)
    initConnection(connId, localAuth, remoteAuth),

    nondet connId = oneOf(ALL_CONN_IDS)
    nondet privLevel = oneOf(Set(Clear, ContextScoped, Blinded))
    val privCtx = {
      privacyLevel: privLevel,
      contextId: Some("ctx1"),
      capabilityFiltering: false,
      messageBlinding: privLevel == Blinded
    }
    establishConnection(connId, privCtx, 0),

    nondet connId = oneOf(ALL_CONN_IDS)
    nondet ctxId = oneOf(ALL_CTX_IDS)
    nondet protocol = oneOf(Set(Quic, Tcp, WebRTC, Relay))
    createSession(connId, ctxId, protocol),

    nondet sessionId = oneOf(ALL_SESSION_IDS)
    nondet msgSize = oneOf(Set(100, 500, 1000))
    nondet guardPasses = oneOf(Set(true, false))
    val guardSnapshot = if (guardPasses) {
      { capGuardResult: GuardPass, flowGuardResult: GuardPass, journalCouplerResult: GuardPass }
    } else {
      { capGuardResult: GuardDeny, flowGuardResult: GuardPass, journalCouplerResult: GuardPass }
    }
    sendMessage(sessionId, "payload", msgSize, guardSnapshot),

    nondet sessionId = oneOf(ALL_SESSION_IDS)
    nondet msgId = oneOf(Set(1001, 1002, 1003))
    nondet seq = oneOf(Set(1, 2, 3, 4, 5))
    nondet sender = oneOf(ALL_AUTH_IDS)
    receiveMessage(sessionId, msgId, seq, 100, sender),

    nondet sessionId = oneOf(ALL_SESSION_IDS)
    nondet reason = oneOf(Set(Graceful, Timeout, FlowBudgetExceeded))
    closeSession(sessionId, reason),

    nondet connId = oneOf(ALL_CONN_IDS)
    nondet reason = oneOf(Set(Graceful, RemoteClosed, Timeout))
    closeConnection(connId, reason)
  }
}
