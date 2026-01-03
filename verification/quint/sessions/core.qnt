// Session Management Protocol Specification
//
// Models authority sessions with presence tracking and epoch management.
// Sessions track activity, issue presence tickets, and auto-expire on inactivity.
// Supports hierarchical sub-sessions inheriting from parent authority.
//
// Key behaviors:
// - Authorities register and create sessions bound to current epoch
// - Sessions issue presence tickets to prove liveness
// - Inactivity beyond MAX_INACTIVE marks session inactive
// - Epoch transitions can revoke sessions missing presence requirements
//
// Safety: Compromised authorities cannot create new sessions.
// Liveness: Active sessions with recent tickets persist across epochs.
//
// See: docs/100_authority_and_identity.md, docs/108_transport_and_information_flow.md

module protocol_sessions {
  // ============ TYPE DEFINITIONS ============
  // Base types for session management.

  type Option[a] = Some(a) | None
  type AuthorityId = str      // Participant identity
  type SessionId = str        // Session identifier
  type EpochNumber = int      // Protocol epoch
  type Timestamp = int        // Wall-clock time

  // Per-authority state tracking session and status.
  type AuthorityRecord = {
    currentSession: Option[SessionId],  // Active session if any
    lastPresence: Timestamp,            // Last activity timestamp
    compromised: bool,                  // Compromised flag (blocks new sessions)
    suspended: bool                     // Temporary suspension
  }

  // Per-session state tracking activity and parent.
  type SessionRecord = {
    authority: AuthorityId,       // Owning authority
    epoch: EpochNumber,           // Epoch when created
    createdAt: Timestamp,         // Creation time
    lastActivity: Timestamp,      // Last ticket issue time
    tickets: Set[int],            // Presence tickets issued
    active: bool,                 // Currently active
    parent: Option[SessionId]     // Parent session (for sub-sessions)
  }

  // Per-epoch state tracking sessions and requirements.
  type EpochRecord = {
    number: EpochNumber,         // Epoch identifier
    startTime: Timestamp,        // When epoch started
    sessions: Set[SessionId],    // Sessions created in this epoch
    revoked: Set[SessionId],     // Sessions revoked in this epoch
    presenceRequirement: int     // Tickets needed to survive epoch
  }

  // Session credential for authentication.
  type Credential = {
    authority: AuthorityId,
    session: SessionId,
    epoch: EpochNumber,
    validUntil: Timestamp
  }

  // ============ STATE VARIABLES ============
  // Global mutable state tracked by the model checker.

  var authorities: AuthorityId -> AuthorityRecord   // Authority registry
  var sessions: SessionId -> SessionRecord          // Active sessions
  var epochs: EpochNumber -> EpochRecord            // Epoch history
  var credentials: Set[Credential]                  // Valid credentials
  var currentEpoch: EpochNumber                     // Current epoch
  var currentTime: Timestamp                        // Global clock
  var thresholdApprovers: int                       // Approval threshold

  // ============ CONSTANTS ============
  // Protocol timing parameters.

  pure val EPOCH_DURATION = 1000    // Time units per epoch
  pure val MAX_INACTIVE = 500       // Max inactivity before auto-expire
  pure val SESSION_TIMEOUT = 2000   // Credential validity duration

  // ============ HELPERS ============
  // Pure functions for safe state access.

  pure def getOrDefaultAuthorityRecord(m: AuthorityId -> AuthorityRecord, key: AuthorityId): Option[AuthorityRecord] =
    if (key.in(m.keys())) Some(m.get(key)) else None

  pure def getOrDefaultSessionRecord(m: SessionId -> SessionRecord, key: SessionId): Option[SessionRecord] =
    if (key.in(m.keys())) Some(m.get(key)) else None

  pure def getOrDefaultEpochRecord(m: EpochNumber -> EpochRecord, key: EpochNumber): Option[EpochRecord] =
    if (key.in(m.keys())) Some(m.get(key)) else None

  pure def optionMatches(opt: Option[SessionId], predicate: SessionId => bool): bool =
    match opt {
      | Some(value) => predicate(value)
      | None => false
    }

  // Check if threshold approval is met.
  pure def computeThresholdApproval(label: str, approvers: int): bool =
    approvers >= 2 and label != ""

  // Generate presence ticket hash (simplified deterministic hash).
  pure def presenceTicket(sessionId: SessionId, epoch: EpochNumber, nonce: int): int =
    (epoch * 101 + nonce * 37) % 2147483647

  // ============ INITIALISATION ============
  // Start with epoch 0 and empty registries.

  action init: bool = all {
    authorities' = Map(),
    sessions' = Map(),
    epochs' = Map().set(0, {
      number: 0,
      startTime: 0,
      sessions: Set(),
      revoked: Set(),
      presenceRequirement: 1
    }),
    credentials' = Set(),
    currentEpoch' = 0,
    currentTime' = 0,
    thresholdApprovers' = 2
  }

  // ============ AUTHORITY ACTIONS ============
  // Authority registration and status management.

  // Register a new authority (fresh, not compromised).
  action registerAuthority(authority: AuthorityId): bool = all {
    not(authority.in(authorities.keys())),
    authorities' = authorities.set(authority, {
      currentSession: None,
      lastPresence: currentTime,
      compromised: false,
      suspended: false
    }),
    sessions' = sessions,
    epochs' = epochs,
    credentials' = credentials,
    currentEpoch' = currentEpoch,
    currentTime' = currentTime,
    thresholdApprovers' = thresholdApprovers
  }

  // Mark authority as compromised (blocks new sessions, suspends).
  action markCompromised(authority: AuthorityId): bool = {
    val recordOpt = getOrDefaultAuthorityRecord(authorities, authority)
    val hasRecord = match recordOpt { | Some(_) => true | None => false }
    val record = match recordOpt {
      | Some(r) => r
      | None => { currentSession: None, lastPresence: 0, compromised: false, suspended: false }
    }
    all {
      hasRecord,
      authorities' = authorities.set(authority, {
        ...record,
        compromised: true,
        suspended: true
      }),
      sessions' = sessions,
      epochs' = epochs,
      credentials' = credentials,
      currentEpoch' = currentEpoch,
      currentTime' = currentTime,
      thresholdApprovers' = thresholdApprovers
    }
  }

  // ============ SESSION ACTIONS ============
  // Session lifecycle management.

  // Create new session for authority (requires not compromised/suspended).
  action createSession(authority: AuthorityId, session: SessionId): bool = {
    val authorityRecordOpt = getOrDefaultAuthorityRecord(authorities, authority)
    val hasAuthority = match authorityRecordOpt { | Some(_) => true | None => false }
    val authorityRecord = match authorityRecordOpt {
      | Some(r) => r
      | None => { currentSession: None, lastPresence: 0, compromised: false, suspended: false }
    }
    val epochRecordOpt = getOrDefaultEpochRecord(epochs, currentEpoch)
    val hasEpoch = match epochRecordOpt { | Some(_) => true | None => false }
    val epochRecord = match epochRecordOpt {
      | Some(r) => r
      | None => { number: 0, startTime: 0, sessions: Set(), revoked: Set(), presenceRequirement: 1 }
    }
    val sessionNotExists = not(session.in(sessions.keys()))
    val notCompromised = not(authorityRecord.compromised)
    val notSuspended = not(authorityRecord.suspended)
    val updatedAuthority = {
      ...authorityRecord,
      currentSession: Some(session),
      lastPresence: currentTime
    }
    val sessionRecord = {
      authority: authority,
      epoch: currentEpoch,
      createdAt: currentTime,
      lastActivity: currentTime,
      tickets: Set(),
      active: true,
      parent: None
    }
    all {
      hasAuthority,
      hasEpoch,
      sessionNotExists,
      notCompromised,
      notSuspended,
      authorities' = authorities.set(authority, updatedAuthority),
      sessions' = sessions.set(session, sessionRecord),
      epochs' = epochs.set(currentEpoch, {
        ...epochRecord,
        sessions: epochRecord.sessions.union(Set(session))
      }),
      credentials' = credentials.union(Set({
        authority: authority,
        session: session,
        epoch: currentEpoch,
        validUntil: currentTime + SESSION_TIMEOUT
      })),
      currentEpoch' = currentEpoch,
      currentTime' = currentTime,
      thresholdApprovers' = thresholdApprovers
    }
  }

  // Create sub-session inheriting from parent (for delegation).
  action createSubSession(parent: SessionId, child: SessionId): bool = {
    val parentSessionOpt = getOrDefaultSessionRecord(sessions, parent)
    val hasParent = match parentSessionOpt { | Some(_) => true | None => false }
    val parentSession = match parentSessionOpt {
      | Some(r) => r
      | None => { authority: "", epoch: 0, createdAt: 0, lastActivity: 0, tickets: Set(), active: false, parent: None }
    }
    val childNotExists = not(child.in(sessions.keys()))
    val parentActive = parentSession.active
    all {
      hasParent,
      childNotExists,
      parentActive,
      sessions' = sessions.set(child, {
        authority: parentSession.authority,
        epoch: currentEpoch,
        createdAt: currentTime,
        lastActivity: currentTime,
        tickets: Set(),
        active: true,
        parent: Some(parent)
      }),
      authorities' = authorities,
      epochs' = epochs,
      credentials' = credentials,
      currentEpoch' = currentEpoch,
      currentTime' = currentTime,
      thresholdApprovers' = thresholdApprovers
    }
  }

  // Issue presence ticket (proves session is alive, updates lastActivity).
  action issueTicket(session: SessionId, nonce: int): bool = {
    val recordOpt = getOrDefaultSessionRecord(sessions, session)
    val hasRecord = match recordOpt { | Some(_) => true | None => false }
    val record = match recordOpt {
      | Some(r) => r
      | None => { authority: "", epoch: 0, createdAt: 0, lastActivity: 0, tickets: Set(), active: false, parent: None }
    }
    val isActive = record.active
    val notInactive = currentTime - record.lastActivity <= MAX_INACTIVE
    val ticket = presenceTicket(session, record.epoch, nonce)
    all {
      hasRecord,
      isActive,
      notInactive,
      sessions' = sessions.set(session, {
        ...record,
        lastActivity: currentTime,
        tickets: record.tickets.union(Set(ticket))
      }),
      authorities' = authorities,
      epochs' = epochs,
      credentials' = credentials,
      currentEpoch' = currentEpoch,
      currentTime' = currentTime,
      thresholdApprovers' = thresholdApprovers
    }
  }

  // ============ EPOCH MANAGEMENT ============
  // Epoch transitions and session revocation.

  // Advance to next epoch (expires sessions without enough tickets).
  action advanceEpoch: bool = {
    val currentRecordOpt = getOrDefaultEpochRecord(epochs, currentEpoch)
    val hasRecord = match currentRecordOpt { | Some(_) => true | None => false }
    val currentRecord = match currentRecordOpt {
      | Some(r) => r
      | None => { number: 0, startTime: 0, sessions: Set(), revoked: Set(), presenceRequirement: 1 }
    }
    val enoughTime = currentTime >= currentRecord.startTime + EPOCH_DURATION
    val approved = computeThresholdApproval("advanceEpoch", thresholdApprovers)
    val nextEpochNum = currentEpoch + 1
    val nextRecord = {
      number: nextEpochNum,
      startTime: currentTime,
      sessions: Set(),
      revoked: Set(),
      presenceRequirement: currentRecord.presenceRequirement
    }
    val updatedSessions = sessions.keys().fold(sessions, (acc, sid) =>
      if (sid.in(acc.keys())) {
        val s = acc.get(sid)
        if (s.epoch == currentEpoch and s.tickets.size() < currentRecord.presenceRequirement)
          acc.set(sid, {...s, active: false})
        else
          acc
      } else acc
    )
    all {
      hasRecord,
      enoughTime,
      approved,
      epochs' = epochs.set(nextEpochNum, nextRecord),
      currentEpoch' = nextEpochNum,
      credentials' = credentials.filter(c => c.validUntil > currentTime),
      sessions' = updatedSessions,
      authorities' = authorities,
      currentTime' = currentTime,
      thresholdApprovers' = thresholdApprovers
    }
  }

  // Revoke a session (also revokes child sessions).
  action revokeSession(session: SessionId): bool = {
    val recordOpt = getOrDefaultSessionRecord(sessions, session)
    val hasRecord = match recordOpt { | Some(_) => true | None => false }
    val record = match recordOpt {
      | Some(r) => r
      | None => { authority: "", epoch: 0, createdAt: 0, lastActivity: 0, tickets: Set(), active: false, parent: None }
    }
    val epochRecordOpt = getOrDefaultEpochRecord(epochs, record.epoch)
    val hasEpoch = match epochRecordOpt { | Some(_) => true | None => false }
    val epochRecord = match epochRecordOpt {
      | Some(r) => r
      | None => { number: 0, startTime: 0, sessions: Set(), revoked: Set(), presenceRequirement: 1 }
    }
    val approved = computeThresholdApproval("revokeSession", thresholdApprovers)
    val updatedEpoch = {
      ...epochRecord,
      revoked: epochRecord.revoked.union(Set(session))
    }
    val childSessions = sessions.keys().filter(sid =>
      if (sid.in(sessions.keys())) optionMatches(sessions.get(sid).parent, p => p == session) else false
    )
    val updatedSessions = childSessions.fold(
      sessions.set(session, {...record, active: false}),
      (acc, sid) =>
        if (sid.in(acc.keys())) acc.set(sid, {...acc.get(sid), active: false}) else acc
    )
    all {
      hasRecord,
      hasEpoch,
      approved,
      epochs' = epochs.set(record.epoch, updatedEpoch),
      sessions' = updatedSessions,
      credentials' = credentials.filter(c => c.session != session),
      authorities' = authorities,
      currentEpoch' = currentEpoch,
      currentTime' = currentTime,
      thresholdApprovers' = thresholdApprovers
    }
  }

  // ============ TIME ============
  // Time progression and auto-expiration.

  // Advance time (expires inactive sessions, removes stale credentials).
  action advanceTime(delta: int): bool = {
    val newTime = currentTime + delta
    val updatedSessions = sessions.keys().fold(sessions, (acc, sid) =>
      if (sid.in(acc.keys())) {
        val s = acc.get(sid)
        if (s.active and newTime - s.lastActivity > MAX_INACTIVE)
          acc.set(sid, {...s, active: false})
        else
          acc
      } else acc
    )
    all {
      delta > 0,
      currentTime' = newTime,
      credentials' = credentials.filter(c => c.validUntil > newTime),
      sessions' = updatedSessions,
      authorities' = authorities,
      epochs' = epochs,
      currentEpoch' = currentEpoch,
      thresholdApprovers' = thresholdApprovers
    }
  }

  // ============ INVARIANTS ============
  // Safety properties that must hold in every reachable state.

  // Sessions belong to registered authorities.
  val InvariantAuthoritiesRegisteredSessions =
    sessions.keys().forall(sid =>
      sessions.get(sid).authority.in(authorities.keys())
    )

  // Active sessions have recent activity (within MAX_INACTIVE).
  val InvariantActiveSessionsRecent =
    sessions.keys().forall(sid =>
      sessions.get(sid).active implies currentTime - sessions.get(sid).lastActivity <= MAX_INACTIVE
    )

  // Revoked sessions are inactive.
  val InvariantRevokedInactive =
    epochs.keys().forall(epochNum =>
      epochs.get(epochNum).revoked.forall(sid =>
        sid.in(sessions.keys()) and not(sessions.get(sid).active)
      )
    )

  // Credentials reference existing sessions and valid epochs.
  val InvariantCredentialsMatchEpoch =
    credentials.forall(c =>
      c.epoch <= currentEpoch and c.session.in(sessions.keys())
    )

  // Presence requirement is non-negative.
  val InvariantPresenceThreshold =
    epochs.keys().forall(epochNum =>
      epochs.get(epochNum).presenceRequirement >= 0
    )
}
