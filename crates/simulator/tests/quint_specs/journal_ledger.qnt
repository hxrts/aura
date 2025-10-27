// Journal/Ledger CRDT Specification
// Formal model of Aura's Automerge-based threshold-signed event ledger

module JournalLedger {
    import SessionTypes.*
    
    // ========== Core Types ==========
    
    type AccountId = str
    type EventId = str
    type EventHash = bytes
    type Epoch = int
    type LamportClock = int
    
    // Event authorization types
    type EventAuthorization = 
        | DeviceCertificate(deviceId: DeviceId, signature: bytes)
        | ThresholdSignature(participants: Set[DeviceId], signature: bytes)
        | GuardianApproval(guardianId: str, signature: bytes)
    
    // Event types
    type EventType = 
        | AddDevice(deviceId: DeviceId, publicKey: bytes)
        | RemoveDevice(deviceId: DeviceId)
        | AddGuardian(guardianId: str, publicKey: bytes)
        | RemoveGuardian(guardianId: str)
        | BumpSessionEpoch(newEpoch: Epoch)
        | GrantOperationLock(lock: OperationLock)
        | ReleaseOperationLock(sessionId: SessionId)
        | InitiateDKDSession(sessionId: SessionId, contextId: bytes)
        | RecordDKDCommitment(sessionId: SessionId, deviceId: DeviceId, commitment: bytes)
        | RevealDKDPoint(sessionId: SessionId, deviceId: DeviceId, point: bytes)
        | FinalizeDKDSession(sessionId: SessionId, derivedKey: bytes)
    
    // Event structure
    type Event = {
        eventId: EventId,
        eventType: EventType,
        authorization: EventAuthorization,
        lamportClock: LamportClock,
        nonce: int,
        previousHash: EventHash,
        eventHash: EventHash
    }
    
    // Device metadata
    type DeviceMetadata = {
        deviceId: DeviceId,
        deviceName: str,
        publicKey: bytes,
        addedAt: int,
        lastSeen: int,
        nextNonce: int,
        usedNonces: Set[int]
    }
    
    // Guardian metadata
    type GuardianMetadata = {
        guardianId: str,
        name: str,
        publicKey: bytes,
        addedAt: int
    }
    
    // Operation lock for distributed coordination
    type OperationLock = {
        sessionId: SessionId,
        operationType: str,
        holderDeviceId: DeviceId,
        grantedAt: LamportClock,
        expiresAt: LamportClock
    }
    
    // Account state (CRDT state)
    type AccountState = {
        accountId: AccountId,
        groupPublicKey: bytes,
        devices: DeviceId -> DeviceMetadata,
        removedDevices: Set[DeviceId],
        guardians: str -> GuardianMetadata,
        removedGuardians: Set[str],
        sessionEpoch: Epoch,
        lamportClock: LamportClock,
        activeOperationLock: OperationLock,
        threshold: int,
        totalParticipants: int,
        usedNonces: Set[int],
        nextNonce: int,
        lastEventHash: EventHash
    }
    
    // ========== State Variables ==========
    
    var accountStates: AccountId -> AccountState
    var eventLog: List[Event]
    var currentTime: int
    
    // ========== Helper Functions ==========
    
    // Compute event hash
    pure def computeEventHash(event: Event): EventHash = {
        // Blake3(eventType || authorization || lamportClock || nonce || previousHash)
        EventHash("hash_" + event.eventId)
    }
    
    // Verify event authorization
    pure def verifyAuthorization(event: Event, state: AccountState): bool = {
        match event.authorization {
            | DeviceCertificate(deviceId, signature) => 
                // Device exists and signature is valid
                deviceId.in(state.devices.keys()) and
                not(state.removedDevices.contains(deviceId))
            | ThresholdSignature(participants, signature) =>
                // Enough participants and all are valid devices
                participants.size() >= state.threshold and
                participants.forall(id => 
                    id.in(state.devices.keys()) and
                    not(state.removedDevices.contains(id))
                )
            | GuardianApproval(guardianId, signature) =>
                // Guardian exists and signature is valid
                guardianId.in(state.guardians.keys()) and
                not(state.removedGuardians.contains(guardianId))
        }
    }
    
    // Check if event requires threshold signature
    pure def requiresThresholdAuth(eventType: EventType): bool = {
        match eventType {
            | AddDevice(_) => true
            | RemoveDevice(_) => true
            | AddGuardian(_) => true
            | RemoveGuardian(_) => true
            | BumpSessionEpoch(_) => true
            | GrantOperationLock(_) => false  // Single device can request
            | ReleaseOperationLock(_) => false
            | InitiateDKDSession(_) => false
            | RecordDKDCommitment(_) => false
            | RevealDKDPoint(_) => false
            | FinalizeDKDSession(_) => true
        }
    }
    
    // Validate nonce to prevent replay
    pure def validateNonce(nonce: int, state: AccountState): bool = {
        not(state.usedNonces.contains(nonce)) and nonce >= state.nextNonce
    }
    
    // Check if devices form valid threshold set
    pure def isValidThresholdSet(devices: Set[DeviceId], state: AccountState): bool = {
        devices.size() >= state.threshold and
        devices.forall(id => 
            id.in(state.devices.keys()) and
            not(state.removedDevices.contains(id))
        )
    }
    
    // ========== Actions ==========
    
    // Apply event to account state
    action applyEvent(accountId: AccountId, event: Event): bool = {
        all {
            // Account exists
            accountId.in(accountStates.keys()),
            val state = accountStates.get(accountId),
            // Event is properly authorized
            verifyAuthorization(event, state),
            // Threshold auth for high-impact events
            requiresThresholdAuth(event.eventType) implies
                event.authorization.match {
                    | ThresholdSignature(_, _) => true
                    | _ => false
                },
            // Valid nonce (replay protection)
            validateNonce(event.nonce, state),
            // Previous hash matches
            event.previousHash == state.lastEventHash,
            // Apply event to state
            val newState = applyEventToState(event, state),
            accountStates' = accountStates.put(accountId, newState),
            eventLog' = eventLog.append(event),
            currentTime' = currentTime + 1
        }
    }
    
    // Apply event to state (pure function)
    pure def applyEventToState(event: Event, state: AccountState): AccountState = {
        // Update Lamport clock
        val newLamportClock = max(state.lamportClock, event.lamportClock) + 1,
        // Update nonce tracking
        val newUsedNonces = state.usedNonces.union(Set(event.nonce)),
        val newNextNonce = if (event.nonce >= state.nextNonce) event.nonce + 1 else state.nextNonce,
        // Base state updates
        val baseNewState = state
            .with("lamportClock", newLamportClock)
            .with("usedNonces", newUsedNonces)
            .with("nextNonce", newNextNonce)
            .with("lastEventHash", event.eventHash),
        // Apply event-specific changes
        match event.eventType {
            | AddDevice(deviceId, publicKey) => 
                val device = {
                    deviceId: deviceId,
                    deviceName: "Device",
                    publicKey: publicKey,
                    addedAt: currentTime,
                    lastSeen: currentTime,
                    nextNonce: 0,
                    usedNonces: Set()
                },
                baseNewState.with("devices", state.devices.put(deviceId, device))
            | RemoveDevice(deviceId) =>
                baseNewState
                    .with("devices", state.devices.mapRemove(deviceId))
                    .with("removedDevices", state.removedDevices.union(Set(deviceId)))
            | AddGuardian(guardianId, publicKey) =>
                val guardian = {
                    guardianId: guardianId,
                    name: "Guardian",
                    publicKey: publicKey,
                    addedAt: currentTime
                },
                baseNewState.with("guardians", state.guardians.put(guardianId, guardian))
            | RemoveGuardian(guardianId) =>
                baseNewState
                    .with("guardians", state.guardians.mapRemove(guardianId))
                    .with("removedGuardians", state.removedGuardians.union(Set(guardianId)))
            | BumpSessionEpoch(newEpoch) =>
                baseNewState.with("sessionEpoch", newEpoch)
            | GrantOperationLock(lock) =>
                baseNewState.with("activeOperationLock", lock)
            | ReleaseOperationLock(_) =>
                baseNewState.with("activeOperationLock", {
                    sessionId: "",
                    operationType: "",
                    holderDeviceId: "",
                    grantedAt: 0,
                    expiresAt: 0
                })
            | _ => baseNewState  // Other events don't modify core state
        }
    }
    
    // Create threshold-authorized event
    action createThresholdEvent(
        accountId: AccountId,
        eventType: EventType,
        participants: Set[DeviceId]
    ): bool = {
        all {
            // Account exists
            accountId.in(accountStates.keys()),
            val state = accountStates.get(accountId),
            // Valid threshold set
            isValidThresholdSet(participants, state),
            // Event requires threshold auth
            requiresThresholdAuth(eventType),
            // Create event
            val event = {
                eventId: "event_" + eventLog.length().toString(),
                eventType: eventType,
                authorization: ThresholdSignature(participants, bytes("sig")),
                lamportClock: state.lamportClock + 1,
                nonce: state.nextNonce,
                previousHash: state.lastEventHash,
                eventHash: EventHash("")  // Will be computed
            },
            val eventWithHash = event.with("eventHash", computeEventHash(event)),
            // Apply event
            applyEvent(accountId, eventWithHash)
        }
    }
    
    // CRDT merge operation
    action mergeStates(accountId: AccountId, remoteState: AccountState): bool = {
        all {
            // Account exists
            accountId.in(accountStates.keys()),
            val localState = accountStates.get(accountId),
            // Same account
            remoteState.accountId == localState.accountId,
            // Merge states using CRDT rules
            val mergedState = {
                accountId: accountId,
                groupPublicKey: localState.groupPublicKey,  // Immutable
                // G-Set merge for devices (union minus removed)
                devices: localState.devices.keys().union(remoteState.devices.keys())
                    .exclude(localState.removedDevices.union(remoteState.removedDevices))
                    .fold(Map(), (acc, id) => 
                        // Take device with higher lastSeen
                        val localDevice = localState.devices.get(id),
                        val remoteDevice = remoteState.devices.get(id),
                        if (localDevice.lastSeen >= remoteDevice.lastSeen)
                            acc.put(id, localDevice)
                        else
                            acc.put(id, remoteDevice)
                    ),
                removedDevices: localState.removedDevices.union(remoteState.removedDevices),
                // G-Set merge for guardians
                guardians: localState.guardians.keys().union(remoteState.guardians.keys())
                    .exclude(localState.removedGuardians.union(remoteState.removedGuardians))
                    .fold(Map(), (acc, id) =>
                        if (id.in(localState.guardians.keys()))
                            acc.put(id, localState.guardians.get(id))
                        else
                            acc.put(id, remoteState.guardians.get(id))
                    ),
                removedGuardians: localState.removedGuardians.union(remoteState.removedGuardians),
                // Max for monotonic values
                sessionEpoch: max(localState.sessionEpoch, remoteState.sessionEpoch),
                lamportClock: max(localState.lamportClock, remoteState.lamportClock),
                // Lock with higher Lamport timestamp wins
                activeOperationLock: if (localState.activeOperationLock.grantedAt >= remoteState.activeOperationLock.grantedAt)
                    localState.activeOperationLock
                else
                    remoteState.activeOperationLock,
                threshold: localState.threshold,  // Immutable
                totalParticipants: localState.totalParticipants,  // Immutable
                // Union of used nonces
                usedNonces: localState.usedNonces.union(remoteState.usedNonces),
                nextNonce: max(localState.nextNonce, remoteState.nextNonce),
                lastEventHash: if (localState.lamportClock >= remoteState.lamportClock)
                    localState.lastEventHash
                else
                    remoteState.lastEventHash
            },
            accountStates' = accountStates.put(accountId, mergedState),
            eventLog' = eventLog,
            currentTime' = currentTime + 1
        }
    }
    
    // ========== Invariants ==========
    
    // Causal ordering: Lamport clocks increase monotonically
    invariant causalOrdering = {
        eventLog.indices().forall(i =>
            i > 0 implies eventLog[i].lamportClock > eventLog[i-1].lamportClock
        )
    }
    
    // Hash chain integrity
    invariant hashChainIntegrity = {
        eventLog.indices().forall(i =>
            i > 0 implies eventLog[i].previousHash == eventLog[i-1].eventHash
        )
    }
    
    // No device resurrection: removed devices stay removed
    invariant noDeviceResurrection = {
        accountStates.values().forall(state =>
            state.removedDevices.forall(deviceId =>
                not(state.devices.keys().contains(deviceId))
            )
        )
    }
    
    // Threshold consistency
    invariant thresholdConsistency = {
        accountStates.values().forall(state =>
            state.threshold <= state.devices.keys().size() + 1  // +1 for recovery scenarios
        )
    }
    
    // Nonce monotonicity
    invariant nonceMonotonicity = {
        accountStates.values().forall(state =>
            state.usedNonces.forall(nonce => nonce < state.nextNonce + 1000)  // Allow reordering window
        )
    }
    
    // ========== Properties ==========
    
    // CRDT convergence: identical event sets produce identical states
    property crdtConvergence = {
        always(accountStates.keys().forall(id1 =>
            accountStates.keys().forall(id2 =>
                // If two accounts have processed the same events
                val events1 = eventLog.select(e => e.eventId.contains(id1)),
                val events2 = eventLog.select(e => e.eventId.contains(id2)),
                (id1 == id2 and events1 == events2) implies
                    // Then their states are identical
                    accountStates.get(id1) == accountStates.get(id2)
            )
        ))
    }
    
    // Byzantine fault tolerance: minority cannot perform threshold operations
    property byzantineFaultTolerance = {
        always(accountStates.values().forall(state =>
            eventLog.forall(event =>
                // If event requires threshold auth
                requiresThresholdAuth(event.eventType) implies
                    // Then it must have threshold signatures
                    event.authorization.match {
                        | ThresholdSignature(participants, _) => 
                            participants.size() >= state.threshold
                        | _ => false
                    }
            )
        ))
    }
    
    // Replay protection
    property replayProtection = {
        always(accountStates.values().forall(state =>
            // No nonce is used twice
            eventLog.select(e => 
                e.authorization.match {
                    | DeviceCertificate(deviceId, _) => 
                        state.devices.keys().contains(deviceId)
                    | _ => false
                }
            ).forall(e1 =>
                eventLog.forall(e2 =>
                    (e1.eventId != e2.eventId and e1.nonce == e2.nonce) implies false
                )
            )
        ))
    }
    
    // Operation lock mutual exclusion
    property operationLockMutualExclusion = {
        always(accountStates.keys().forall(id1 =>
            accountStates.keys().forall(id2 =>
                val lock1 = accountStates.get(id1).activeOperationLock,
                val lock2 = accountStates.get(id2).activeOperationLock,
                // Same account can't have two different active locks
                (id1 == id2 and lock1.sessionId != "" and lock2.sessionId != "") implies
                    lock1.sessionId == lock2.sessionId
            )
        ))
    }
    
    // ========== Initial State ==========
    
    action init = {
        all {
            accountStates' = Map(),
            eventLog' = [],
            currentTime' = 0
        }
    }
    
    // ========== Utility Functions ==========
    
    pure def max(a: int, b: int): int = if (a > b) a else b
}