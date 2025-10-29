module protocol_sessions {
  import protocol_core as core from "protocol_core"

  type DeviceId = core.DeviceId
  type SessionId = core.SessionId
  type EpochNumber = int
  type Timestamp = int

  type Option[a] = Some(a) | None

  type DeviceRecord = {
    currentSession: Option[SessionId],
    lastPresence: Timestamp,
    compromised: bool,
    suspended: bool
  }

  type SessionRecord = {
    device: DeviceId,
    epoch: EpochNumber,
    createdAt: Timestamp,
    lastActivity: Timestamp,
    tickets: Set[int],
    active: bool,
    parent: Option[SessionId]
  }

  type EpochRecord = {
    number: EpochNumber,
    startTime: Timestamp,
    sessions: Set[SessionId],
    revoked: Set[SessionId],
    presenceRequirement: int
  }

  type Credential = {
    device: DeviceId,
    session: SessionId,
    epoch: EpochNumber,
    validUntil: Timestamp
  }

  // ==================== STATE ====================

  var devices: DeviceId -> DeviceRecord
  var sessions: SessionId -> SessionRecord
  var epochs: EpochNumber -> EpochRecord
  var credentials: Set[Credential]
  var currentEpoch: EpochNumber
  var currentTime: Timestamp
  var thresholdApprovers: int

  // ==================== CONSTANTS ====================

  pure val EPOCH_DURATION = 1000
  pure val MAX_INACTIVE = 500
  pure val SESSION_TIMEOUT = 2000

  // ==================== HELPERS ====================

  pure def getOrDefault[K, V](m: K -> V, key: K, default: V): V =
    if key.in(m.keys()) then m.get(key) else default

  pure def optionMatches[a](opt: Option[a], predicate: a -> bool): bool =
    match opt {
      | Some(value) => predicate(value)
      | None => false
    }

  pure def computeThresholdApproval(label: str, approvers: int): bool =
    approvers >= 2 and label.length() > 0

  pure def presenceTicket(sessionId: SessionId, epoch: EpochNumber, nonce: int): int =
    (sessionId.length() * 101 + epoch * 37 + nonce * 23) % 2147483647

  // ==================== INITIALISATION ====================

  action init: bool = all {
    devices' = Map(),
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

  // ==================== DEVICE ACTIONS ====================

  action registerDevice(device: DeviceId): bool = all {
    not device.in(devices.keys()),
    devices' = devices.set(device, {
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

  action markCompromised(device: DeviceId): bool =
    val record = devices.get(device) {
      device.in(devices.keys()) {
        devices' = devices.set(device, {
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

  // ==================== SESSION ACTIONS ====================

  action createSession(device: DeviceId, session: SessionId): bool =
    val deviceRecord = devices.get(device) {
    val epochRecord = epochs.get(currentEpoch) {
      device.in(devices.keys()) and
      not session.in(sessions.keys()) and
      not deviceRecord.compromised and
      not deviceRecord.suspended {
        val updatedDevice = {
          ...deviceRecord,
          currentSession: Some(session),
          lastPresence: currentTime
        }
        val sessionRecord = {
          device: device,
          epoch: currentEpoch,
          createdAt: currentTime,
          lastActivity: currentTime,
          tickets: Set(),
          active: true,
          parent: None
        }
        devices' = devices.set(device, updatedDevice),
        sessions' = sessions.set(session, sessionRecord),
        epochs' = epochs.set(currentEpoch, {
          ...epochRecord,
          sessions: epochRecord.sessions.union(Set(session))
        }),
        credentials' = credentials.union(Set({
          device: device,
          session: session,
          epoch: currentEpoch,
          validUntil: currentTime + SESSION_TIMEOUT
        })),
        currentEpoch' = currentEpoch,
        currentTime' = currentTime,
        thresholdApprovers' = thresholdApprovers
      }
    }}

  action createSubSession(parent: SessionId, child: SessionId): bool =
    val parentSession = sessions.get(parent) {
      parent.in(sessions.keys()) and
      parentSession.active and
      not child.in(sessions.keys()) {
        sessions' = sessions.set(child, {
          device: parentSession.device,
          epoch: currentEpoch,
          createdAt: currentTime,
          lastActivity: currentTime,
          tickets: Set(),
          active: true,
          parent: Some(parent)
        }),
        devices' = devices,
        epochs' = epochs,
        credentials' = credentials,
        currentEpoch' = currentEpoch,
        currentTime' = currentTime,
        thresholdApprovers' = thresholdApprovers
      }
    }

  action issueTicket(session: SessionId, nonce: int): bool =
    val record = sessions.get(session) {
      session.in(sessions.keys()) and
      record.active and
      currentTime - record.lastActivity <= MAX_INACTIVE {
        val ticket = presenceTicket(session, record.epoch, nonce)
        sessions' = sessions.set(session, {
          ...record,
          lastActivity: currentTime,
          tickets: record.tickets.union(Set(ticket))
        }),
        devices' = devices,
        epochs' = epochs,
        credentials' = credentials,
        currentEpoch' = currentEpoch,
        currentTime' = currentTime,
        thresholdApprovers' = thresholdApprovers
      }
    }

  // ==================== EPOCH MANAGEMENT ====================

  action advanceEpoch: bool =
    val currentRecord = epochs.get(currentEpoch) {
      currentTime >= currentRecord.startTime + EPOCH_DURATION and
      computeThresholdApproval("advanceEpoch", thresholdApprovers) {
        val nextEpoch = currentEpoch + 1
        val nextRecord = {
          number: nextEpoch,
          startTime: currentTime,
          sessions: Set(),
          revoked: Set(),
          presenceRequirement: currentRecord.presenceRequirement
        }
        epochs' = epochs.set(nextEpoch, nextRecord),
        currentEpoch' = nextEpoch,
        devices' = devices,
        credentials' = credentials.filter(c => c.validUntil > currentTime),
        sessions' = sessions.keys().fold(sessions, (acc, sid) =>
          val s = acc.get(sid) {
            if (s.epoch = currentEpoch and s.tickets.size() < currentRecord.presenceRequirement)
              acc.set(sid, {...s, active: false})
            else
              acc
          }
        ),
        currentTime' = currentTime,
        thresholdApprovers' = thresholdApprovers
      }
    }

  action revokeSession(session: SessionId): bool =
    val record = sessions.get(session) {
    val epochRecord = epochs.get(record.epoch) {
      computeThresholdApproval("revokeSession", thresholdApprovers) {
        val updatedEpoch = {
          ...epochRecord,
          revoked: epochRecord.revoked.union(Set(session))
        }
        val childSessions = sessions.keys().filter(sid => optionMatches(sessions.get(sid).parent, value => value = session))
        epochs' = epochs.set(record.epoch, updatedEpoch),
        sessions' = childSessions.fold(sessions.set(session, {...record, active: false}), (acc, sid) =>
          acc.set(sid, {...acc.get(sid), active: false})
        ),
        devices' = devices,
        credentials' = credentials.filter(c => c.session != session),
        currentEpoch' = currentEpoch,
        currentTime' = currentTime,
        thresholdApprovers' = thresholdApprovers
      }
    }}

  // ==================== TIME ====================

  action advanceTime(delta: int): bool = all {
    delta > 0,
    currentTime' = currentTime + delta,
    credentials' = credentials.filter(c => c.validUntil > currentTime'),
    sessions' = sessions.keys().fold(sessions, (acc, sid) =>
      val s = acc.get(sid) {
        if (s.active and currentTime' - s.lastActivity > MAX_INACTIVE)
          acc.set(sid, {...s, active: false})
        else
          acc
      }
    ),
    devices' = devices,
    epochs' = epochs,
    currentEpoch' = currentEpoch,
    thresholdApprovers' = thresholdApprovers
  }

  // ==================== INVARIANTS ====================

  val InvariantDevicesRegisteredSessions =
    sessions.keys().forall(sid =>
      sessions.get(sid).device.in(devices.keys())
    )

  val InvariantActiveSessionsRecent =
    sessions.keys().forall(sid =>
      sessions.get(sid).active implies currentTime - sessions.get(sid).lastActivity <= MAX_INACTIVE
    )

  val InvariantRevokedInactive =
    epochs.keys().forall(epochNum =>
      epochs.get(epochNum).revoked.forall(sid =>
        sid.in(sessions.keys()) and not sessions.get(sid).active
      )
    )

  val InvariantCredentialsMatchEpoch =
    credentials.forall(c =>
      c.epoch <= currentEpoch and c.session.in(sessions.keys())
    )

  val InvariantPresenceThreshold =
    epochs.keys().forall(epochNum =>
      epochs.get(epochNum).presenceRequirement >= 0
    )
}
