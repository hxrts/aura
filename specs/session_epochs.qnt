// Session Epoch and Presence Ticket Management Specification
// Manages device sessions, epoch transitions, and presence verification
module session_epochs {
  // ==================== TYPES ====================
  
  type Option = Some({value: str}) | None
  type DeviceId = str
  type SessionId = str
  type EpochNumber = int
  type PresenceTicket = int
  type Timestamp = int
  type Credential = {
    deviceId: DeviceId,
    sessionId: SessionId,
    epoch: EpochNumber,
    validUntil: Timestamp
  }
  
  type DeviceState = {
    id: DeviceId,
    currentSession: Option,
    lastPresence: Timestamp,
    isCompromised: bool,
    isSuspended: bool
  }
  
  type SessionState = {
    id: SessionId,
    deviceId: DeviceId,
    epoch: EpochNumber,
    createdAt: Timestamp,
    lastActivity: Timestamp,
    presenceTickets: Set[PresenceTicket],
    isActive: bool,
    parentSession: Option  // For sub-sessions
  }
  
  type EpochState = {
    number: EpochNumber,
    startTime: Timestamp,
    sessions: Set[SessionId],
    revokedSessions: Set[SessionId],
    presenceRequirement: int  // Required presence tickets per epoch
  }
  
  // ==================== STATE ====================
  
  var devices: DeviceId -> DeviceState
  var sessions: SessionId -> SessionState
  var epochs: EpochNumber -> EpochState
  var currentEpoch: EpochNumber
  var currentTime: Timestamp
  var credentials: Set[Credential]
  var thresholdApprovers: int  // M-of-N for critical operations

  pure def unchangedExcept(vars: Set[str]): bool = all {
    if ("devices".in(vars)) true else devices' = devices,
    if ("sessions".in(vars)) true else sessions' = sessions,
    if ("epochs".in(vars)) true else epochs' = epochs,
    if ("currentEpoch".in(vars)) true else currentEpoch' = currentEpoch,
    if ("currentTime".in(vars)) true else currentTime' = currentTime,
    if ("credentials".in(vars)) true else credentials' = credentials,
    if ("thresholdApprovers".in(vars)) true else thresholdApprovers' = thresholdApprovers
  }

  pure def optionSession(opt: Option): str =
    match opt {
      | Some({value: s}) => s
      | None => ""
    }
  
  // ==================== CONSTANTS ====================
  
  pure val EPOCH_DURATION = 1000
  pure val PRESENCE_INTERVAL = 100
  pure val MAX_INACTIVE_TIME = 500
  pure val MIN_THRESHOLD = 2
  pure val SESSION_TIMEOUT = 2000
  
  // ==================== INITIALIZATION ====================
  
  action init: bool = all {
    devices' = Map(),
    sessions' = Map(),
    epochs' = Map().set(0, {
      number: 0,
      startTime: 0,
      sessions: Set(),
      revokedSessions: Set(),
      presenceRequirement: 1
    }),
    currentEpoch' = 0,
    currentTime' = 0,
    credentials' = Set(),
    thresholdApprovers' = MIN_THRESHOLD
  }
  
  // ==================== DEVICE MANAGEMENT ====================
  
  action registerDevice(deviceId: DeviceId): bool = all {
    not(deviceId.in(devices.keys())),
    devices' = devices.set(deviceId, {
      id: deviceId,
      currentSession: None,
      lastPresence: currentTime,
      isCompromised: false,
      isSuspended: false
    }),
    unchangedExcept(Set("devices"))
  }
  
  action markDeviceCompromised(deviceId: DeviceId): bool = 
    val device = devices.get(deviceId) {
    val deviceSessions = sessions.keys().filter(sId =>
      sessions.get(sId).deviceId == deviceId and
      sessions.get(sId).isActive
    ) {
      all {
        deviceId.in(devices.keys()),
        devices' = devices.set(deviceId, {
          ...device,
          isCompromised: true,
          isSuspended: true
        }),
        // Revoke all active sessions for this device
        sessions' = deviceSessions.fold(sessions, (acc, sId) =>
          val s = acc.get(sId) {
            acc.set(sId, {...s, isActive: false})
          }
        ),
        unchangedExcept(Set("devices", "sessions"))
      }
    }}
  
  // ==================== SESSION MANAGEMENT ====================
  
  action createSession(deviceId: DeviceId, sessionId: SessionId): bool = 
    val device = devices.get(deviceId) {
    val epoch = epochs.get(currentEpoch) {
    val capsule = generateSessionCapsule(sessionId, deviceId, currentEpoch) {
      all {
        deviceId.in(devices.keys()),
        not(sessionId.in(sessions.keys())),
        not(device.isCompromised),
        not(device.isSuspended),
        // Update device
        devices' = devices.set(deviceId, {
          ...device,
          currentSession: Some({value: sessionId})
        }),
        // Create session with capsule
        sessions' = sessions.set(sessionId, {
          id: sessionId,
          deviceId: deviceId,
          epoch: currentEpoch,
          createdAt: currentTime,
          lastActivity: currentTime,
          presenceTickets: Set(),
          isActive: true,
          parentSession: None
        }),
        // Update epoch
        epochs' = epochs.set(currentEpoch, {
          ...epoch,
          sessions: epoch.sessions.union(Set(sessionId))
        }),
        // Issue credential
        credentials' = credentials.union(Set({
          deviceId: deviceId,
          sessionId: sessionId,
          epoch: currentEpoch,
          validUntil: currentTime + SESSION_TIMEOUT
        })),
        unchangedExcept(Set("devices", "sessions", "epochs", "credentials"))
      }
    }}}
  
  action createSubSession(parentSessionId: SessionId, subSessionId: SessionId): bool = 
    val parentSession = sessions.get(parentSessionId) {
    val deviceId = parentSession.deviceId {
      all {
        parentSessionId.in(sessions.keys()),
        not(subSessionId.in(sessions.keys())),
        parentSession.isActive,
        // Create sub-session with restricted capabilities
        sessions' = sessions.set(subSessionId, {
          id: subSessionId,
          deviceId: deviceId,
          epoch: currentEpoch,
          createdAt: currentTime,
          lastActivity: currentTime,
          presenceTickets: Set(),
          isActive: true,
          parentSession: Some({value: parentSessionId})
        }),
        unchangedExcept(Set("sessions"))
      }
    }}
  
  action issuePresenceTicket(sessionId: SessionId): bool = 
    val session = sessions.get(sessionId) {
    val nonce = generatePresenceNonce(sessionId, currentTime) {
    val ticket = generatePresenceTicket(sessionId, session.epoch, nonce) {
      all {
        sessionId.in(sessions.keys()),
        session.isActive,
        currentTime - session.lastActivity <= MAX_INACTIVE_TIME,
        // Verify this is a valid ticket for the current epoch
        verifyPresenceTicket(ticket, sessionId, session.epoch),
        sessions' = sessions.set(sessionId, {
          ...session,
          lastActivity: currentTime,
          presenceTickets: session.presenceTickets.union(Set(ticket))
        }),
        unchangedExcept(Set("sessions"))
      }
    }}}
  
  // ==================== EPOCH MANAGEMENT ====================
  
  action advanceEpoch: bool = 
    val newEpochNumber = currentEpoch + 1 {
    val currentEpochData = epochs.get(currentEpoch) {
      all {
        currentTime >= epochs.get(currentEpoch).startTime + EPOCH_DURATION,
        // Requires threshold approval
        computeThresholdApproval("advanceEpoch", thresholdApprovers),
        epochs' = epochs.set(newEpochNumber, {
          number: newEpochNumber,
          startTime: currentTime,
          sessions: Set(),
          revokedSessions: Set(),
          presenceRequirement: epochs.get(currentEpoch).presenceRequirement
        }),
        currentEpoch' = newEpochNumber,
        // Invalidate old credentials
        credentials' = credentials.filter(c => c.validUntil > currentTime),
        // Deactivate sessions without sufficient presence
        sessions' = sessions.keys().fold(sessions, (acc, sId) =>
          val s = acc.get(sId) {
            if (s.epoch == currentEpoch and 
                s.presenceTickets.size() < currentEpochData.presenceRequirement) 
              acc.set(sId, {...s, isActive: false})
            else
              acc
          }
        ),
        unchangedExcept(Set("epochs", "currentEpoch", "credentials", "sessions"))
      }
    }}
  
  action revokeSession(sessionId: SessionId): bool = 
    val session = sessions.get(sessionId) {
    val epoch = epochs.get(session.epoch) {
    val subSessions = sessions.keys().filter(sId =>
      match sessions.get(sId).parentSession {
        | Some({value: pid}) => pid == sessionId
        | None => false
      }
    ) {
      all {
        sessionId.in(sessions.keys()),
        // Requires threshold approval
        computeThresholdApproval("revokeSession", thresholdApprovers),
        sessions' = sessions.set(sessionId, {...session, isActive: false}),
        epochs' = epochs.set(session.epoch, {
          ...epoch,
          revokedSessions: epoch.revokedSessions.union(Set(sessionId))
        }),
        // Revoke credential
        credentials' = credentials.exclude(
          credentials.filter(c => c.sessionId == sessionId)
        ),
        // Also revoke sub-sessions
        sessions' = subSessions.fold(sessions', (acc, sId) =>
          val s = acc.get(sId) {
            acc.set(sId, {...s, isActive: false})
          }
        ),
        unchangedExcept(Set("sessions", "epochs", "credentials"))
      }
    }}}
  
  // ==================== TIME ADVANCEMENT ====================
  
  action advanceTime(delta: int): bool = all {
    delta > 0,
    currentTime' = currentTime + delta,
    // Clean expired credentials
    credentials' = credentials.filter(c => c.validUntil > currentTime'),
    // Deactivate timed-out sessions
    sessions' = sessions.keys().fold(sessions, (acc, sId) => {
      val s = acc.get(sId)
      if (s.isActive and currentTime' - s.lastActivity > MAX_INACTIVE_TIME) {
        acc.set(sId, {...s, isActive: false})
      } else {
        acc
      }
    }),
    unchangedExcept(Set("currentTime", "credentials", "sessions"))
  }
  
  // ==================== CRYPTOGRAPHIC FUNCTIONS ====================
  
  // Generate presence ticket using HKDF
  pure def generatePresenceTicket(sessionId: SessionId, epoch: EpochNumber, nonce: int): int = {
    // Real implementation: HKDF(seed_capsule || session_epoch || handshake_nonce)
    // handshake_secret = HKDF_SHA256(seed_capsule || session_epoch || handshake_nonce, "", "", 32)
    // Simulated as deterministic combination
    val seedCapsule = sessionId.length() * 101  // Simulate seed capsule from session
    val handshakeSecret = (seedCapsule + epoch * 37 + nonce * 23) % 2147483647
    handshakeSecret
  }
  
  // Generate context capsule MAC for tamper detection
  pure def generateCapsuleMAC(sessionId: SessionId, capsuleData: int): int = {
    // Real implementation: BLAKE3_keyed(seed_capsule, cbor(capsule))
    // capsule_mac = BLAKE3_keyed(seed_capsule, capsule_cbor)
    // Simulated as deterministic MAC
    val seedCapsule = sessionId.length() * 101
    (seedCapsule * 31 + capsuleData * 17) % 2147483647
  }
  
  // Verify presence ticket is valid for given epoch
  pure def verifyPresenceTicket(ticket: int, sessionId: SessionId, epoch: EpochNumber): bool = {
    // In real implementation, would recompute and compare
    // Here we check ticket is non-zero and within valid range
    ticket > 0 and ticket < 2147483647
  }
  
  // Generate deterministic nonce for presence ticket
  pure def generatePresenceNonce(sessionId: SessionId, timestamp: int): int = {
    // Real implementation uses secure randomness
    // For spec: deterministic based on session and time
    (sessionId.length() * 53 + timestamp * 7) % 1000000
  }
  
  // Compute threshold approval signature for critical operations
  pure def computeThresholdApproval(operation: str, approverCount: int): bool = {
    // Real implementation requires M-of-N threshold signatures
    // For spec: check we have minimum threshold approvers
    approverCount >= MIN_THRESHOLD
  }
  
  // Generate session capsule for context binding
  pure def generateSessionCapsule(sessionId: SessionId, deviceId: DeviceId, epoch: EpochNumber): int = {
    // Real implementation: CBOR encoding of session context
    // capsule = CBOR({session_id, device_id, epoch, seed_capsule})
    // Simulated as deterministic combination
    (sessionId.length() * 47 + deviceId.length() * 29 + epoch * 13) % 2147483647
  }
  
  // Derive epoch-specific key from session
  pure def deriveEpochKey(sessionId: SessionId, epoch: EpochNumber): int = {
    // Real implementation: HKDF(master_key, salt=epoch, info="epoch_key") 
    // Simulated derivation
    (sessionId.length() * 61 + epoch * 43) % 2147483647
  }
  
  // ==================== HELPER FUNCTIONS ====================
  
  pure def activeSessionCount: int = 
    sessions.values().filter(s => s.isActive).size()
  
  pure def compromisedDeviceCount: int = 
    devices.values().filter(d => d.isCompromised).size()
  
  pure def epochSessionCount(epoch: EpochNumber): int = 
    if (epoch.in(epochs.keys())) {
      epochs.get(epoch).sessions.size()
    } else {
      0
    }
  
  pure def hasValidPresence(sessionId: SessionId): bool = {
    if (sessionId.in(sessions.keys())) {
      val session = sessions.get(sessionId)
      val epoch = epochs.get(session.epoch)
      session.presenceTickets.size() >= epoch.presenceRequirement
    } else {
      false
    }
  }

  pure def max(a: int, b: int): int = if (a > b) a else b
  
  // ==================== INVARIANTS ====================
  
  // Safety: No compromised device has active sessions
  val CompromisedDeviceSafety = 
    devices.values().forall(d =>
      d.isCompromised implies 
      sessions.values().forall(s =>
        (s.deviceId == d.id) implies not(s.isActive)
      )
    )
  
  // Safety: Sub-sessions inherit parent status
  val SubSessionConsistency = 
    sessions.values().forall(s =>
      s.parentSession != None implies {
        val parent = sessions.get(optionSession(s.parentSession))
        not(parent.isActive) implies not(s.isActive)
      }
    )
  
  // Safety: Credentials match active sessions
  val CredentialValidity = 
    credentials.forall(c =>
      c.sessionId.in(sessions.keys()) implies
      sessions.get(c.sessionId).isActive
    )
  
  // Progress: Epochs advance monotonically
  val EpochMonotonicity = 
    epochs.keys().forall(e1 =>
      epochs.keys().forall(e2 =>
        e1 < e2 implies epochs.get(e1).startTime < epochs.get(e2).startTime
      )
    )
  
  // Liveness: Active sessions maintain presence
  val PresenceRequirement = 
    sessions.values().forall(s =>
      (s.isActive and currentTime - s.createdAt > PRESENCE_INTERVAL) implies
      s.presenceTickets.size() > 0
    )
  
  // ==================== TEMPORAL PROPERTIES ====================
  
  // Liveness: Sessions timeout without activity
  temporal SessionTimeout = 
    always(
      sessions.values().forall(s =>
        (s.isActive and currentTime - s.lastActivity > MAX_INACTIVE_TIME) implies
        eventually(not(s.isActive))
      )
    )
  
  // Safety: Revoked sessions stay revoked
  temporal RevocationPermanence = 
    always(
      sessions.values().forall(s =>
        not(s.isActive) implies always(not(s.isActive))
      )
    )
  
  // Liveness: Epochs eventually advance
  temporal EpochProgress = 
    always(
      currentTime >= epochs.get(currentEpoch).startTime + EPOCH_DURATION implies
      eventually(currentEpoch > epochs.keys().fold(0, (acc, k) => if (k > acc) k else acc))
    )
  
  // Safety: Compromised devices stay compromised
  temporal CompromiseIrreversibility = 
    always(
      devices.values().forall(d =>
        d.isCompromised implies always(d.isCompromised)
      )
    )
  
  // Correctness: Presence tickets prevent false timeouts
  temporal PresenceProtection = 
    always(
      sessions.values().forall(s =>
        (s.isActive and hasValidPresence(s.id)) implies
        (currentTime - s.lastActivity <= MAX_INACTIVE_TIME)
      )
    )
}
