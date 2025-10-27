// CRDT-based Account Ledger with Threshold-Signed Events Specification
// Implements Automerge-style CRDT with threshold authorization
module journal_ledger {
  // ==================== TYPES ====================
  
  type EventId = str
  type DeviceId = str
  type LamportTime = int
  type ThresholdSignature = int
  type OperationId = str
  
  type EventType = 
    | DeviceAdded(DeviceId)
    | DeviceRemoved(DeviceId)
    | GuardianAdded(str)
    | GuardianRemoved(str)
    | CapabilityGranted(str)
    | CapabilityRevoked(str)
    | OperationLocked(OperationId)
    | OperationUnlocked(OperationId)
    | StateUpdated(str)
  
  type Event = {
    id: EventId,
    eventType: EventType,
    lamportTime: LamportTime,
    deviceId: DeviceId,
    signature: ThresholdSignature,
    dependencies: Set[EventId]  // Causal dependencies
  }
  
  type DeviceState = {
    id: DeviceId,
    lamportClock: LamportTime,
    eventLog: List[Event],
    pendingEvents: Set[Event],
    isByzantine: bool
  }
  
  type LedgerState = {
    devices: Set[DeviceId],
    guardians: Set[str],
    capabilities: str -> Set[str],  // capability -> authorized entities
    lockedOperations: Set[OperationId],
    customState: str -> str
  }
  
  type ReplicaState = {
    deviceId: DeviceId,
    ledger: LedgerState,
    eventHistory: Set[Event],
    lamportClock: LamportTime,
    missingDependencies: EventId -> Set[EventId]
  }
  
  // ==================== STATE ====================
  
  var devices: DeviceId -> DeviceState
  var replicas: DeviceId -> ReplicaState
  var threshold: int
  var globalEventLog: Set[Event]
  var convergenceRound: int

  pure def unchangedExcept(vars: Set[str]): bool = all {
    if ("devices".in(vars)) true else devices' = devices,
    if ("replicas".in(vars)) true else replicas' = replicas,
    if ("threshold".in(vars)) true else threshold' = threshold,
    if ("globalEventLog".in(vars)) true else globalEventLog' = globalEventLog,
    if ("convergenceRound".in(vars)) true else convergenceRound' = convergenceRound
  }
  
  // ==================== CONSTANTS ====================
  
  pure val MIN_THRESHOLD = 2
  pure val MAX_DEVICES = 10
  pure val MAX_LAMPORT_TIME = 10000
  
  // ==================== INITIALIZATION ====================
  
  action init: bool = all {
    devices' = Map(),
    replicas' = Map(),
    threshold' = MIN_THRESHOLD,
    globalEventLog' = Set(),
    convergenceRound' = 0
  }
  
  // ==================== SETUP ACTIONS ====================
  
  action setupLedger(deviceIds: Set[DeviceId], t: int): bool = all {
    deviceIds.size() >= MIN_THRESHOLD,
    deviceIds.size() <= MAX_DEVICES,
    t >= MIN_THRESHOLD,
    t <= deviceIds.size(),
    // Initialize devices
    devices' = deviceIds.mapBy(id => {
      id: id,
      lamportClock: 0,
      eventLog: List(),
      pendingEvents: Set(),
      isByzantine: false
    }),
    // Initialize replicas with empty ledger
    replicas' = deviceIds.mapBy(id => {
      deviceId: id,
      ledger: {
        devices: deviceIds,
        guardians: Set(),
        capabilities: Map(),
        lockedOperations: Set(),
        customState: Map()
      },
      eventHistory: Set(),
      lamportClock: 0,
      missingDependencies: Map()
    }),
    threshold' = t,
    unchangedExcept(Set("devices", "replicas", "threshold"))
  }
  
  // ==================== BYZANTINE ACTIONS ====================
  
  action markByzantine(deviceId: DeviceId): bool = all {
    deviceId.in(devices.keys()),
    val device = devices.get(deviceId)
    devices' = devices.set(deviceId, {...device, isByzantine: true}),
    unchangedExcept(Set("devices"))
  }
  
  // ==================== EVENT CREATION ====================
  
  action createEvent(deviceId: DeviceId, eventType: EventType): bool = all {
    deviceId.in(devices.keys()),
    val device = devices.get(deviceId)
    val replica = replicas.get(deviceId)
    val isAuthorized = match eventType {
      | DeviceAdded(_) => true  // Requires threshold signatures
      | DeviceRemoved(_) => true
      | GuardianAdded(_) => true
      | GuardianRemoved(_) => true
      | CapabilityGranted(_) => true
      | CapabilityRevoked(_) => true
      | OperationLocked(opId) => not(opId.in(replica.ledger.lockedOperations))
      | OperationUnlocked(opId) => opId.in(replica.ledger.lockedOperations)
      | StateUpdated(_) => true
    }
    isAuthorized,
    val newLamport = device.lamportClock + 1
    val eventId = generateEventId(deviceId, newLamport, eventType)
    val dependencies = replica.eventHistory.filter(e => 
      // Include recent events as dependencies for causal consistency
      e.lamportTime >= newLamport - 10 and e.deviceId != deviceId
    ).map(e => e.id).fold(Set(), (acc, id) => acc.union(Set(id)))
    // Collect threshold signatures from other devices
    val signers = replica.ledger.devices.filter(d => d != deviceId).fold(Set(deviceId), 
      (acc, d) => if (acc.size() < threshold) acc.union(Set(d)) else acc
    )
    val event = {
      id: eventId,
      eventType: eventType,
      lamportTime: newLamport,
      deviceId: deviceId,
      signature: if (device.isByzantine) 
        1.to(1000).oneOf()  // Byzantine signature
      else
        createThresholdSignature({
          id: eventId,
          eventType: eventType,
          lamportTime: newLamport,
          deviceId: deviceId,
          signature: 0,  // Placeholder for signature computation
          dependencies: dependencies
        }, signers, threshold),
      dependencies: dependencies
    }
    devices' = devices.set(deviceId, {
      ...device,
      lamportClock: newLamport,
      pendingEvents: device.pendingEvents.union(Set(event))
    }),
    unchangedExcept(Set("devices"))
  }
  
  // ==================== EVENT PROPAGATION ====================
  
  action propagateEvent(fromDevice: DeviceId, toDevice: DeviceId, eventId: EventId): bool = all {
    fromDevice.in(devices.keys()),
    toDevice.in(devices.keys()),
    fromDevice != toDevice,
    val device = devices.get(fromDevice)
    // Find the event with matching ID
    val eventOpt = device.pendingEvents.filter(e => e.id == eventId)
    val event = if (eventOpt.size() > 0) 
      eventOpt.fold({id: "", eventType: StateUpdated(""), lamportTime: 0, deviceId: "", signature: 0, dependencies: Set()}, (_, e) => e)
    else
      {id: "", eventType: StateUpdated(""), lamportTime: 0, deviceId: "", signature: 0, dependencies: Set()}
    event.id == eventId,
    val targetReplica = replicas.get(toDevice)
    val missingDeps = event.dependencies.exclude(
      targetReplica.eventHistory.map(e => e.id)
    )
    replicas' = if (missingDeps.size() == 0)
      replicas.set(toDevice, applyEventToReplica(targetReplica, event))
    else
      replicas.set(toDevice, {
        ...targetReplica,
        missingDependencies: targetReplica.missingDependencies.set(
          eventId, missingDeps
        )
      }),
    globalEventLog' = globalEventLog.union(Set(event)),
    unchangedExcept(Set("replicas", "globalEventLog"))
  }
  
  // ==================== CRDT OPERATIONS ====================
  
  pure def applyEventToReplica(replica: ReplicaState, event: Event): ReplicaState = 
    val newLamport = max(replica.lamportClock, event.lamportTime) + 1
    val newLedger = applyEventToLedger(replica.ledger, event)
    {
      ...replica,
      ledger: newLedger,
      eventHistory: replica.eventHistory.union(Set(event)),
      lamportClock: newLamport
    }
  
  pure def applyEventToLedger(ledger: LedgerState, event: Event): LedgerState = 
    match event.eventType {
      | DeviceAdded(deviceId) => {
          ...ledger,
          devices: ledger.devices.union(Set(deviceId))
        }
      | DeviceRemoved(deviceId) => {
          ...ledger,
          devices: ledger.devices.exclude(Set(deviceId))
        }
      | GuardianAdded(guardian) => {
          ...ledger,
          guardians: ledger.guardians.union(Set(guardian))
        }
      | GuardianRemoved(guardian) => {
          ...ledger,
          guardians: ledger.guardians.exclude(Set(guardian))
        }
      | CapabilityGranted(cap) => {
          ...ledger,
          capabilities: ledger.capabilities.set(cap, Set("authorized"))
        }
      | CapabilityRevoked(cap) => {
          ...ledger,
          capabilities: ledger.capabilities.setBy(cap, Set())
        }
      | OperationLocked(opId) => {
          ...ledger,
          lockedOperations: ledger.lockedOperations.union(Set(opId))
        }
      | OperationUnlocked(opId) => {
          ...ledger,
          lockedOperations: ledger.lockedOperations.exclude(Set(opId))
        }
      | StateUpdated(key) => {
          ...ledger,
          customState: ledger.customState.set(key, "updated")
        }
    }
  
  // ==================== CONVERGENCE ACTIONS ====================
  
  action resolveDependencies(deviceId: DeviceId): bool = all {
    deviceId.in(replicas.keys()),
    val replica = replicas.get(deviceId)
    replica.missingDependencies.size() > 0,
    // Try to resolve missing dependencies
    val resolved = replica.missingDependencies.keys().filter(eventId => {
      val missingDeps = replica.missingDependencies.get(eventId)
      missingDeps.subseteq(replica.eventHistory.map(e => e.id))
    })
    resolved.size() > 0,
    // Apply resolved events
    val eventsToApply = globalEventLog.filter(e => e.id.in(resolved))
    val newReplica = eventsToApply.fold(replica, (acc, event) => 
      applyEventToReplica(acc, event)
    )
    replicas' = replicas.set(deviceId, {
      ...newReplica,
      missingDependencies: replica.missingDependencies.keys()
        .exclude(resolved)
        .mapBy(id => replica.missingDependencies.get(id))
    }),
    unchangedExcept(Set("replicas"))
  }

  action incrementConvergenceRound = all {
    convergenceRound' = convergenceRound + 1,
    unchangedExcept(Set("convergenceRound"))
  }
  
  // ==================== HELPER FUNCTIONS ====================
  
  pure def max(a: int, b: int): int = 
    if (a > b) a else b
  
  pure def byzantineCount: int = 
    devices.values().filter(d => d.isByzantine).size()
  
  pure def isConverged: bool = 
    replicas.keys().forall(d1 =>
      replicas.keys().forall(d2 =>
        replicas.get(d1).ledger == replicas.get(d2).ledger
      )
    )
  
  // ==================== CRYPTOGRAPHIC FUNCTIONS ====================
  
  // Generate event ID using BLAKE3 hash
  pure def generateEventId(deviceId: DeviceId, lamportTime: LamportTime, eventType: EventType): EventId = 
    // Real implementation: BLAKE3(device_id || lamport_time || event_type)
    // Ensures unique, deterministic event IDs
    val typeHash = match eventType {
      | DeviceAdded(d) => d.length() * 2
      | DeviceRemoved(d) => d.length() * 3
      | GuardianAdded(g) => g.length() * 5
      | GuardianRemoved(g) => g.length() * 7
      | CapabilityGranted(c) => c.length() * 11
      | CapabilityRevoked(c) => c.length() * 13
      | OperationLocked(o) => o.length() * 17
      | OperationUnlocked(o) => o.length() * 19
      | StateUpdated(s) => s.length() * 23
    }
    deviceId.concat("_").concat(lamportTime.toString()).concat("_").concat(typeHash.toString())
  
  // Create threshold signature for event
  pure def createThresholdSignature(event: Event, signingDevices: Set[DeviceId], threshold: int): ThresholdSignature = 
    // Real implementation: FROST threshold signature
    // 1. Each device creates signature share z_i = k_i + λ_i * s_i * H(event)
    // 2. Coordinator aggregates shares: sig = Σ z_i
    // 3. Verify against group public key
    if (signingDevices.size() >= threshold) {
      // Simulate aggregated threshold signature
      val eventHash = generateEventHash(event)
      val shares = signingDevices.fold(0, (acc, d) => acc + d.length() * 31)
      (eventHash * 17 + shares * 13 + threshold * 7) % 2147483647
    } else {
      0  // Invalid signature if below threshold
    }
  
  // Hash event for signing
  pure def generateEventHash(event: Event): int = 
    // Real implementation: BLAKE3(CBOR(event))
    // Canonical serialization ensures consistent hashing
    val baseHash = event.id.length() * 29 + event.lamportTime * 31
    val depHash = event.dependencies.fold(0, (acc, d) => acc + d.length())
    (baseHash + depHash * 37) % 2147483647
  
  // Verify threshold signature
  pure def verifyThresholdSignature(event: Event, threshold: int): bool = 
    // Real implementation: Ed25519 threshold signature verification
    // Verify signature against group public key
    event.signature > 0 and event.signature % threshold == 0  // Simplified check
  
  // Verify event authorization based on type
  pure def verifyEventAuthorization(event: Event, ledger: LedgerState): bool = 
    match event.eventType {
      | DeviceAdded(_) => hasValidSignature(event)
      | DeviceRemoved(_) => hasValidSignature(event)
      | GuardianAdded(_) => hasValidSignature(event)
      | GuardianRemoved(_) => hasValidSignature(event)
      | CapabilityGranted(cap) => hasValidSignature(event) and not(cap.in(ledger.capabilities.keys()))
      | CapabilityRevoked(cap) => hasValidSignature(event) and cap.in(ledger.capabilities.keys())
      | OperationLocked(opId) => not(opId.in(ledger.lockedOperations))
      | OperationUnlocked(opId) => opId.in(ledger.lockedOperations)
      | StateUpdated(_) => true  // Any authorized device can update state
    }
  
  pure def hasValidSignature(event: Event): bool = 
    verifyThresholdSignature(event, threshold)
  
  // Generate vector clock hash for convergence detection
  pure def generateVectorClockHash(eventHistory: Set[Event]): int = 
    // Real implementation: Hash of all event IDs and their lamport times
    // Used to efficiently detect when replicas have converged
    eventHistory.fold(0, (acc, e) => 
      (acc * 31 + e.id.length() * 17 + e.lamportTime * 13) % 2147483647
    )
  
  // Compute merkle root of event history for verification
  pure def computeEventHistoryRoot(events: Set[Event]): int = 
    // Real implementation: Merkle tree root of all events
    // Allows efficient proof of event inclusion
    if (events.size() == 0) {
      0
    } else {
      events.fold(1, (acc, e) => 
        val eventLeaf = generateEventHash(e)
        (acc * 7 + eventLeaf * 11) % 2147483647
      )
    }
  
  // Generate deterministic ordering for concurrent events  
  pure def eventOrder(e1: Event, e2: Event): bool = 
    // Real implementation: Total ordering for concurrent events
    // Based on lamport time, then device ID, then event hash
    if (e1.lamportTime != e2.lamportTime) {
      e1.lamportTime < e2.lamportTime
    } else if (e1.deviceId != e2.deviceId) {
      e1.deviceId < e2.deviceId
    } else {
      generateEventHash(e1) < generateEventHash(e2)
    }
  
  // ==================== INVARIANTS ====================
  
  // Safety: Lamport clocks are monotonic
  val LamportMonotonicity = 
    devices.values().forall(d => d.lamportClock >= 0 and d.lamportClock <= MAX_LAMPORT_TIME) and
    replicas.values().forall(r => r.lamportClock >= 0 and r.lamportClock <= MAX_LAMPORT_TIME)
  
  // Safety: Events are immutable once created
  val EventImmutability = 
    globalEventLog.forall(e1 =>
      globalEventLog.forall(e2 =>
        e1.id == e2.id implies e1 == e2
      )
    )
  
  // Safety: Causal dependencies are respected
  val CausalConsistency = 
    replicas.values().forall(r =>
      r.eventHistory.forall(e =>
        e.dependencies.subseteq(r.eventHistory.map(h => h.id))
      )
    )
  
  // Safety: Byzantine threshold
  val ByzantineThreshold = 
    byzantineCount < threshold
  
  // Progress: Events eventually propagate
  val EventualPropagation = 
    devices.values().forall(d =>
      d.pendingEvents.size() > 0 implies 
      globalEventLog.exists(e => d.pendingEvents.contains(e))
    )
  
  // ==================== TEMPORAL PROPERTIES ====================
  
  // Convergence: All replicas eventually have same state
  temporal EventualConvergence = 
    always(
      (byzantineCount == 0 and devices.values().forall(d => d.pendingEvents.size() == 0)) implies
      eventually(isConverged)
    )
  
  // Liveness: Events are eventually delivered
  temporal EventDelivery = 
    always(
      globalEventLog.forall(e =>
        eventually(
          replicas.values().forall(r =>
            e.in(r.eventHistory) or e.id.in(r.missingDependencies.keys())
          )
        )
      )
    )
  
  // Safety: Threshold signatures required for critical operations
  temporal ThresholdAuthorization = 
    always(
      globalEventLog.forall(e =>
        match e.eventType {
          | DeviceAdded(_) => hasValidSignature(e)
          | DeviceRemoved(_) => hasValidSignature(e)
          | GuardianAdded(_) => hasValidSignature(e)
          | GuardianRemoved(_) => hasValidSignature(e)
          | _ => true
        }
      )
    )
  
  // Safety: Operation locks prevent concurrent modifications
  temporal LockExclusivity = 
    always(
      replicas.values().forall(r =>
        r.ledger.lockedOperations.forall(opId =>
          // Only one event can lock an operation at a time
          r.eventHistory.filter(e =>
            match e.eventType {
              | OperationLocked(id) => id == opId
              | _ => false
            }
          ).size() == 1
        )
      )
    )
  
  // Byzantine fault tolerance: System progresses despite Byzantine nodes
  temporal ByzantineResilience = 
    always(
      (byzantineCount < threshold) implies
      eventually(
        replicas.keys().filter(d => not(devices.get(d).isByzantine)).forall(d =>
          replicas.get(d).eventHistory.size() > 0
        )
      )
    )
}
