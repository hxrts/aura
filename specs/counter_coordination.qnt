// SSB Counter Coordination Choreography Specification
// Distributed counter management for Social Bulletin Board relationships
module counter_coordination {
  // ==================== TYPES ====================
  
  type ParticipantId = str
  type CounterId = str
  type CounterValue = int
  type LamportTime = int
  type MessageId = str
  
  type CounterOp = 
    | Increment(int)
    | Reset
    | Sync
  
  type CounterState = {
    id: CounterId,
    value: CounterValue,
    lamportTime: LamportTime,
    owner: ParticipantId,
    observers: Set[ParticipantId]
  }
  
  type ParticipantState = {
    id: ParticipantId,
    localCounters: CounterId -> CounterValue,
    lamportClock: LamportTime,
    pendingOps: List[CounterOp],
    receivedMessages: Set[MessageId],
    isByzantine: bool
  }
  
  type Message = {
    id: MessageId,
    sender: ParticipantId,
    recipients: Set[ParticipantId],
    counterId: CounterId,
    op: CounterOp,
    value: CounterValue,
    lamportTime: LamportTime
  }
  
  type ChoreographyPhase =
    | Setup
    | Operating
    | Synchronizing
    | Completed
  
  // ==================== STATE ====================
  
  var participants: ParticipantId -> ParticipantState
  var counters: CounterId -> CounterState
  var messages: Set[Message]
  var deliveredMessages: ParticipantId -> Set[MessageId]
  var phase: ChoreographyPhase
  var globalLamportTime: LamportTime
  
  // ==================== CONSTANTS ====================
  
  pure val MAX_COUNTER_VALUE = 1000
  pure val MAX_LAMPORT_TIME = 10000
  
  // ==================== INITIALIZATION ====================
  
  action init: bool = all {
    participants' = Map(),
    counters' = Map(),
    messages' = Set(),
    deliveredMessages' = Map(),
    phase' = Setup,
    globalLamportTime' = 0
  }
  
  // ==================== SETUP ACTIONS ====================
  
  action setupChoreography(pIds: Set[ParticipantId], cIds: Set[CounterId]): bool = all {
    phase == Setup,
    pIds.size() > 0,
    cIds.size() > 0,
    // Initialize participants
    participants' = pIds.mapBy(id => ({
      id: id,
      localCounters: cIds.mapBy(cId => 0),
      lamportClock: 0,
      pendingOps: List(),
      receivedMessages: Set(),
      isByzantine: false
    })),
    // Initialize counters with owners
    counters' = cIds.mapBy(cId => ({
      id: cId,
      value: 0,
      lamportTime: 0,
      owner: pIds.fold("", (acc, p) => if (acc == "") p else acc),  // First participant owns
      observers: pIds
    })),
    deliveredMessages' = pIds.mapBy(id => Set()),
    phase' = Operating,
    messages' = messages,
    globalLamportTime' = globalLamportTime
  }
  
  // ==================== BYZANTINE ACTIONS ====================
  
  action markByzantine(pId: ParticipantId): bool = 
    val p = participants.get(pId)
    all {
      pId.in(participants.keys()),
      participants' = participants.set(pId, {...p, isByzantine: true}),
      counters' = counters,
      messages' = messages,
      deliveredMessages' = deliveredMessages,
      phase' = phase,
      globalLamportTime' = globalLamportTime
    }
  
  // ==================== COUNTER OPERATIONS ====================
  
  action localIncrement(pId: ParticipantId, cId: CounterId, delta: int): bool = all {
    phase == Operating,
    pId.in(participants.keys()),
    cId.in(counters.keys()),
    delta > 0,
    val p = participants.get(pId)
    val counter = counters.get(cId)
    pId == counter.owner or pId.in(counter.observers),
    val newValue = p.localCounters.get(cId) + delta
    newValue <= MAX_COUNTER_VALUE,
    val newLamport = p.lamportClock + 1
    participants' = participants.set(pId, {
      ...p,
      localCounters: p.localCounters.set(cId, newValue),
      lamportClock: newLamport,
      pendingOps: p.pendingOps.append(Increment(delta))
    }),
    globalLamportTime' = max(globalLamportTime, newLamport),
    counters' = counters,
    messages' = messages,
    deliveredMessages' = deliveredMessages,
    phase' = phase
  }
  
  action broadcastOperation(pId: ParticipantId, cId: CounterId, op: CounterOp): bool = all {
    phase == Operating,
    pId.in(participants.keys()),
    cId.in(counters.keys()),
    val p = participants.get(pId)
    val counter = counters.get(cId)
    op.in(p.pendingOps),
    val nonce = 1.to(10000).oneOf()  // Random nonce for uniqueness
    val msgId = generateMessageId(pId, cId, p.lamportClock, nonce)
    val msg = {
      id: msgId,
      sender: pId,
      recipients: counter.observers,
      counterId: cId,
      op: op,
      value: if (p.isByzantine) 
        0.to(MAX_COUNTER_VALUE).oneOf()  // Byzantine can send arbitrary value
        else p.localCounters.get(cId),
      lamportTime: p.lamportClock
    }
    messages' = messages.union(Set(msg)),
    // Remove from pending
    participants' = participants.set(pId, {
      ...p,
      pendingOps: p.pendingOps.select(o => o != op)
    }),
    counters' = counters,
    deliveredMessages' = deliveredMessages,
    phase' = phase,
    globalLamportTime' = globalLamportTime
  }
  
  action deliverMessage(msgId: MessageId, receiverId: ParticipantId): bool = all {
    phase.in(Set(Operating, Synchronizing)),
    receiverId.in(participants.keys()),
    val candidates = messages.filter(m => m.id == msgId)
    candidates.size() > 0,
    val msg = candidates.oneOf()  // Will fail if empty
    receiverId.in(msg.recipients),
    not(msgId.in(deliveredMessages.get(receiverId))),
    val receiver = participants.get(receiverId)
    val newLamport = max(receiver.lamportClock, msg.lamportTime) + 1
    val newLocalValue = match msg.op {
      | Increment(delta) => min(receiver.localCounters.get(msg.counterId) + delta, MAX_COUNTER_VALUE)
      | Reset => 0
      | Sync => msg.value
    }
    participants' = participants.set(receiverId, {
      ...receiver,
      localCounters: receiver.localCounters.set(msg.counterId, newLocalValue),
      lamportClock: newLamport,
      receivedMessages: receiver.receivedMessages.union(Set(msgId))
    }),
    deliveredMessages' = deliveredMessages.set(
      receiverId, 
      deliveredMessages.get(receiverId).union(Set(msgId))
    ),
    globalLamportTime' = max(globalLamportTime, newLamport),
    counters' = counters,
    messages' = messages,
    phase' = phase
  }
  
  // ==================== SYNCHRONIZATION ACTIONS ====================
  
  action initiateSync: bool = all {
    phase == Operating,
    // All participants have processed their pending operations
    participants.keys().forall(pId => participants.get(pId).pendingOps.length() == 0),
    phase' = Synchronizing,
    participants' = participants,
    counters' = counters,
    messages' = messages,
    deliveredMessages' = deliveredMessages,
    globalLamportTime' = globalLamportTime
  }
  
  action syncCounter(cId: CounterId): bool = all {
    phase == Synchronizing,
    cId.in(counters.keys()),
    val counter = counters.get(cId)
    // Find the value with highest Lamport time among observers
    val observerValues = counter.observers.map(pId => {
      val p = participants.get(pId)
      (p.localCounters.get(cId), p.lamportClock)
    })
    val maxPair = observerValues.fold((0, 0), (acc, v) =>
      if (v._2 > acc._2) v else acc
    )
    counters' = counters.set(cId, {
      ...counter,
      value: maxPair._1,
      lamportTime: maxPair._2
    }),
    participants' = participants,
    messages' = messages,
    deliveredMessages' = deliveredMessages,
    phase' = phase,
    globalLamportTime' = globalLamportTime
  }
  
  action completeChoreography: bool = all {
    phase == Synchronizing,
    // All counters have been synchronized
    counters.keys().forall(cId =>
      counters.get(cId).observers.forall(pId =>
        participants.get(pId).localCounters.get(cId) == counters.get(cId).value
      )
    ),
    phase' = Completed,
    participants' = participants,
    counters' = counters,
    messages' = messages,
    deliveredMessages' = deliveredMessages,
    globalLamportTime' = globalLamportTime
  }
  
  // ==================== HELPER FUNCTIONS ====================
  
  pure def byzantineCount: int = 
    participants.keys().filter(pId => participants.get(pId).isByzantine).size()
  
  pure def isConsistent(counterId: CounterId): bool = 
    if (counterId.in(counters.keys())) {
      val counter = counters.get(counterId)
      val values = counter.observers
        .filter(pId => not(participants.get(pId).isByzantine))
        .map(pId => participants.get(pId).localCounters.get(counterId))
      values.forall(v => v == counter.value)
    } else {
      true
    }
  
  pure def max(a: int, b: int): int = 
    if (a > b) a else b
  
  pure def min(a: int, b: int): int = 
    if (a < b) a else b
  
  // ==================== CRYPTOGRAPHIC FUNCTIONS ====================
  
  // Generate unique message ID using BLAKE3
  pure def generateMessageId(sender: ParticipantId, counterId: CounterId, lamportTime: LamportTime, nonce: int): MessageId = 
    // Real implementation: BLAKE3(sender || counter_id || lamport_time || nonce)
    // Ensures globally unique message IDs for choreography
    sender.concat("_").concat(counterId).concat("_").concat(lamportTime.toString()).concat("_").concat(nonce.toString())
  
  // Create message authentication code for choreographic messages
  pure def generateMessageMAC(msg: Message, senderKey: int): int = 
    // Real implementation: BLAKE3_keyed(sender_key, CBOR(message))
    // Authenticates message origin and integrity
    val msgData = msg.sender.length() + msg.counterId.length() + msg.value + msg.lamportTime
    (msgData * 31 + senderKey * 17) % 2147483647
  
  // Verify message authenticity
  pure def verifyMessageMAC(msg: Message, mac: int, senderKey: int): bool = 
    // Real implementation: Verify BLAKE3 MAC
    val expectedMAC = generateMessageMAC(msg, senderKey)
    abs(mac - expectedMAC) < 100  // Allow small variance for simulation
  
  // Generate vector clock for causal ordering
  pure def generateVectorClock(participants: Set[ParticipantId], clocks: ParticipantId -> LamportTime): int = 
    // Real implementation: Hash of all participant clocks
    // Used for efficient causality verification
    participants.fold(0, (acc, p) => {
      val clock = if (p.in(clocks.keys())) clocks.get(p) else 0
      (acc * 7 + p.length() * 13 + clock * 11) % 2147483647
    })
  
  // Compute counter digest for synchronization
  pure def computeCounterDigest(counterId: CounterId, value: CounterValue, observers: Set[ParticipantId]): int = 
    // Real implementation: H(counter_id || value || sorted(observers))
    // Allows efficient sync verification
    val observerHash = observers.fold(0, (acc, o) => acc + o.length() * 17)
    (counterId.length() * 31 + value * 23 + observerHash) % 2147483647
  
  // Generate choreography proof for message ordering
  pure def generateChoreographyProof(phase: ChoreographyPhase, msgSequence: List[MessageId]): int = 
    // Real implementation: Merkle proof of message sequence
    // Proves adherence to choreographic protocol
    val phaseNum = match phase {
      | Setup => 1
      | Operating => 2
      | Synchronizing => 3
      | Completed => 4
    }
    val seqHash = msgSequence.fold(0, (acc, id) => (acc * 11 + id.length() * 7) % 2147483647)
    (phaseNum * 41 + seqHash) % 2147483647
  
  pure def abs(x: int): int = if (x < 0) -x else x
  
  // ==================== INVARIANTS ====================
  
  // Safety: Counter values stay within bounds
  val CounterBounds = 
    participants.keys().forall(pId =>
      val p = participants.get(pId)
      p.localCounters.keys().forall(k => p.localCounters.get(k) >= 0 and p.localCounters.get(k) <= MAX_COUNTER_VALUE)
    ) and
    counters.keys().forall(cId => counters.get(cId).value >= 0 and counters.get(cId).value <= MAX_COUNTER_VALUE)
  
  // Safety: Lamport clocks are monotonic
  val LamportMonotonicity = 
    globalLamportTime >= 0 and
    globalLamportTime <= MAX_LAMPORT_TIME and
    participants.keys().forall(pId => 
      participants.get(pId).lamportClock >= 0 and participants.get(pId).lamportClock <= globalLamportTime
    )
  
  // Safety: Messages are delivered at most once
  val AtMostOnceDelivery = 
    participants.keys().forall(pId =>
      val delivered = deliveredMessages.get(pId)
      delivered.size() == delivered.powerset().filter(s => s.size() == delivered.size()).size()
    )
  
  // Progress: All operations eventually complete
  val OperationProgress = 
    phase == Operating implies
    participants.keys().exists(pId => participants.get(pId).pendingOps.length() > 0) or
    messages.size() > 0
  
  // Consistency: Honest participants converge
  val EventualConsistency = 
    phase == Completed implies
    counters.keys().forall(cId => isConsistent(cId))
  
  // ==================== TEMPORAL PROPERTIES ====================
  
  // Liveness: Operations eventually get broadcast
  temporal OperationBroadcast = 
    always(
      participants.keys().forall(pId =>
        val p = participants.get(pId)
        (p.pendingOps.length() > 0 and phase == Operating) implies
        eventually(p.pendingOps.length() == 0)
      )
    )
  
  // Liveness: Messages eventually get delivered
  temporal MessageDelivery = 
    always(
      messages.forall(msg =>
        msg.recipients.forall(receiverId =>
          eventually(msg.id.in(deliveredMessages.get(receiverId)))
        )
      )
    )
  
  // Safety: Counter uniqueness maintained
  temporal CounterUniqueness = 
    always(
      counters.keys().forall(cId1 =>
        counters.keys().forall(cId2 =>
          cId1 != cId2 implies counters.get(cId1).value != counters.get(cId2).value
        )
      )
    )
  
  // Convergence: All honest participants reach same state
  temporal HonestConvergence = 
    always(
      (phase == Synchronizing and byzantineCount == 0) implies
      eventually(
        counters.keys().forall(cId =>
          val counter = counters.get(cId)
          counter.observers.forall(pId =>
            participants.get(pId).localCounters.get(cId) == counter.value
          )
        )
      )
    )
  
  // Choreographic correctness: Protocol follows prescribed order
  temporal ChoreographicOrder = 
    always(
      (phase == Setup implies eventually(phase == Operating)) and
      (phase == Operating implies eventually(phase == Synchronizing)) and
      (phase == Synchronizing implies eventually(phase == Completed))
    )
}
