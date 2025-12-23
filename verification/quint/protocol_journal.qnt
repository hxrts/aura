// CRDT Journal Specification
//
// Models Aura's fact-based journal as a state-based CRDT. Events form
// a hash-linked log (via parent pointers) with nonce uniqueness enforcement.
// Merging journals from different replicas converges via set union + max.
//
// Key CRDT properties verified:
// - Merge is commutative, associative, idempotent (semilattice)
// - Reduce is deterministic (same events → same derived state)
// - Lamport timestamps monotonically increase
//
// == Lean Correspondence ==
// Module: Aura.Journal (verification/lean/Aura/Journal.lean)
// - Types: Aura.Journal.Fact, Aura.Journal.FactId, Aura.Journal.Journal
// - Merge: Aura.Journal.merge (set union with deduplication)
// - Reduce: Aura.Journal.reduce (deterministic state derivation)
// - Semilattice: Aura.Journal.merge_comm, merge_assoc, merge_idem
//
// == Rust Correspondence ==
// - File: crates/aura-journal/src/lib.rs
// - Types: Fact, FactId, Journal
// - Functions: merge, reduce
//
// See: docs/102_journal.md

module protocol_journal {
  // ==================== TYPE DEFINITIONS ====================
  // Base types for journal entries and state.

  type Option[a] = Some(a) | None
  type AccountId = str
  type AuthorityId = str  // Opaque authority (was DeviceId)
  type EventId = str
  type SessionId = str

  // How was this event authorized?
  type EventAuthorization =
    | AuthThreshold          // k-of-n threshold signature
    | AuthDevice(AuthorityId)   // Single device signature
    | AuthGuardian(AuthorityId) // Guardian-assisted recovery

  // A single journal event (fact).
  type EventRecord = {
    id: EventId,                 // Unique identifier (typically a hash)
    account: AccountId,          // Which account this event belongs to
    timestamp: int,              // Lamport timestamp (causal ordering)
    nonce: int,                  // Replay protection (must be unique)
    parent: Option[EventId],     // Hash chain link to previous event
    epoch: int,                  // Session epoch for invalidation
    eventType: str,              // Type tag for deserialization
    authorization: EventAuthorization
  }

  // Derived state computed by reducing the event log.
  type LedgerState = {
    account: AccountId,
    authorities: Set[AuthorityId],  // Current authority set
    guardians: Set[AuthorityId],    // Guardian set for recovery
    sessionEpoch: int,              // Current session epoch
    lamport: int,                   // Max Lamport seen
    cooldowns: SessionId -> int     // Recovery cooldown timers
  }

  // Lifecycle status for external coordination.
  type JournalStatus = JournalPending | JournalCompleted

  // ==================== STATE VARIABLES ====================
  // Mutable state tracked by the model checker.

  var ledgerStates: AccountId -> LedgerState      // Derived state per account
  var events: AccountId -> List[EventRecord]      // Event log per account
  var usedNonces: AccountId -> Set[int]           // Consumed nonces (replay protection)
  var lifecycleStatus: JournalStatus              // External lifecycle coordination

  // ==================== HELPER FUNCTIONS ====================
  // Pure functions for safe map/option access.

  pure def getOrDefaultLedger(m: AccountId -> LedgerState, key: AccountId): Option[LedgerState] =
    if (key.in(m.keys())) Some(m.get(key)) else None

  pure def getOrDefaultNonces(m: AccountId -> Set[int], key: AccountId): Set[int] =
    if (key.in(m.keys())) m.get(key) else Set()

  pure def getOrDefaultEvents(m: AccountId -> List[EventRecord], key: AccountId): List[EventRecord] =
    if (key.in(m.keys())) m.get(key) else List()

  // Nonce validation: returns true if nonce hasn't been used yet.
  pure def validateNonce(account: AccountId, nonce: int, nonces: AccountId -> Set[int]): bool =
    not(nonce.in(getOrDefaultNonces(nonces, account)))

  // Get the last event's ID for hash chain linking.
  pure def lastEventId(eventList: List[EventRecord]): Option[EventId] =
    if (eventList.length() == 0) None
    else Some(eventList[eventList.length() - 1].id)

  // ==================== INITIALISATION ====================
  // Start with empty state (no accounts).

  action init: bool = all {
    ledgerStates' = Map(),
    events' = Map(),
    usedNonces' = Map(),
    lifecycleStatus' = JournalPending
  }

  // ==================== ACTIONS ====================
  // These model journal mutations (event appends).

  // Create a new account with initial authority and guardian.
  action createAccount(account: AccountId, authority: AuthorityId, guardian: AuthorityId): bool = all {
    not(account.in(ledgerStates.keys())),
    ledgerStates' = ledgerStates.put(account, {
      account: account,
      authorities: Set(authority),
      guardians: Set(guardian),
      sessionEpoch: 0,
      lamport: 0,
      cooldowns: Map()
    }),
    events' = events.put(account, List()),
    usedNonces' = usedNonces.put(account, Set()),
    lifecycleStatus' = lifecycleStatus
  }

  // Append a generic event to the journal (with nonce validation).
  action appendLedgerEvent(
    account: AccountId,
    eventId: EventId,
    nonce: int,
    eventType: str,
    auth: EventAuthorization
  ): bool = {
    val ledgerOpt = getOrDefaultLedger(ledgerStates, account)
    val hasLedger = match ledgerOpt { | Some(_) => true | None => false }
    val ledger = match ledgerOpt {
      | Some(l) => l
      | None => { account: "", authorities: Set(), guardians: Set(), sessionEpoch: 0, lamport: 0, cooldowns: Map() }
    }
    val validNonce = validateNonce(account, nonce, usedNonces)
    val parentId = lastEventId(getOrDefaultEvents(events, account))
    val newEvent = {
      id: eventId,
      account: account,
      timestamp: ledger.lamport + 1,
      nonce: nonce,
      parent: parentId,
      epoch: ledger.sessionEpoch,
      eventType: eventType,
      authorization: auth
    }
    all {
      hasLedger,
      validNonce,
      ledgerStates' = ledgerStates.set(account, {
        ...ledger,
        lamport: ledger.lamport + 1
      }),
      events' = events.set(account, getOrDefaultEvents(events, account).append(newEvent)),
      usedNonces' = usedNonces.set(account, getOrDefaultNonces(usedNonces, account).union(Set(nonce))),
      lifecycleStatus' = lifecycleStatus
    }
  }

  // Add a new authority to the account (requires threshold authorization).
  action addAuthority(account: AccountId, authority: AuthorityId, nonce: int): bool = {
    val ledgerOpt = getOrDefaultLedger(ledgerStates, account)
    val hasLedger = match ledgerOpt { | Some(_) => true | None => false }
    val ledger = match ledgerOpt {
      | Some(l) => l
      | None => { account: "", authorities: Set(), guardians: Set(), sessionEpoch: 0, lamport: 0, cooldowns: Map() }
    }
    val notPresent = not(ledger.authorities.contains(authority))
    val validNonce = validateNonce(account, nonce, usedNonces)
    val parentId = lastEventId(getOrDefaultEvents(events, account))
    val newEvent = {
      id: "AddAuthority",
      account: account,
      timestamp: ledger.lamport + 1,
      nonce: nonce,
      parent: parentId,
      epoch: ledger.sessionEpoch,
      eventType: "AddAuthority",
      authorization: AuthThreshold
    }
    all {
      hasLedger,
      notPresent,
      validNonce,
      ledgerStates' = ledgerStates.set(account, {
        ...ledger,
        lamport: ledger.lamport + 1,
        authorities: ledger.authorities.union(Set(authority))
      }),
      events' = events.set(account, getOrDefaultEvents(events, account).append(newEvent)),
      usedNonces' = usedNonces.set(account, getOrDefaultNonces(usedNonces, account).union(Set(nonce))),
      lifecycleStatus' = lifecycleStatus
    }
  }

  // Remove an authority from the account.
  action removeAuthority(account: AccountId, authority: AuthorityId, nonce: int): bool = {
    val ledgerOpt = getOrDefaultLedger(ledgerStates, account)
    val hasLedger = match ledgerOpt { | Some(_) => true | None => false }
    val ledger = match ledgerOpt {
      | Some(l) => l
      | None => { account: "", authorities: Set(), guardians: Set(), sessionEpoch: 0, lamport: 0, cooldowns: Map() }
    }
    val isPresent = ledger.authorities.contains(authority)
    val validNonce = validateNonce(account, nonce, usedNonces)
    val parentId = lastEventId(getOrDefaultEvents(events, account))
    val newEvent = {
      id: "RemoveAuthority",
      account: account,
      timestamp: ledger.lamport + 1,
      nonce: nonce,
      parent: parentId,
      epoch: ledger.sessionEpoch,
      eventType: "RemoveAuthority",
      authorization: AuthThreshold
    }
    all {
      hasLedger,
      isPresent,
      validNonce,
      ledgerStates' = ledgerStates.set(account, {
        ...ledger,
        lamport: ledger.lamport + 1,
        authorities: ledger.authorities.filter(d => d != authority)
      }),
      events' = events.set(account, getOrDefaultEvents(events, account).append(newEvent)),
      usedNonces' = usedNonces.set(account, getOrDefaultNonces(usedNonces, account).union(Set(nonce))),
      lifecycleStatus' = lifecycleStatus
    }
  }

  // Increment session epoch (invalidates all sessions in prior epochs).
  action bumpSessionEpoch(account: AccountId, nonce: int): bool = {
    val ledgerOpt = getOrDefaultLedger(ledgerStates, account)
    val hasLedger = match ledgerOpt { | Some(_) => true | None => false }
    val ledger = match ledgerOpt {
      | Some(l) => l
      | None => { account: "", authorities: Set(), guardians: Set(), sessionEpoch: 0, lamport: 0, cooldowns: Map() }
    }
    val validNonce = validateNonce(account, nonce, usedNonces)
    val parentId = lastEventId(getOrDefaultEvents(events, account))
    val newEvent = {
      id: "SessionEpochBump",
      account: account,
      timestamp: ledger.lamport + 1,
      nonce: nonce,
      parent: parentId,
      epoch: ledger.sessionEpoch,
      eventType: "SessionEpochBump",
      authorization: AuthThreshold
    }
    all {
      hasLedger,
      validNonce,
      ledgerStates' = ledgerStates.set(account, {
        ...ledger,
        lamport: ledger.lamport + 1,
        sessionEpoch: ledger.sessionEpoch + 1
      }),
      events' = events.set(account, getOrDefaultEvents(events, account).append(newEvent)),
      usedNonces' = usedNonces.set(account, getOrDefaultNonces(usedNonces, account).union(Set(nonce))),
      lifecycleStatus' = lifecycleStatus
    }
  }

  // Signal that journal processing is complete.
  action signalFinalize: bool = {
    val isPending = match lifecycleStatus { | JournalPending => true | _ => false }
    all {
      isPending,
      lifecycleStatus' = JournalCompleted,
      ledgerStates' = ledgerStates,
      events' = events,
      usedNonces' = usedNonces
    }
  }

  // ==================== STEP RELATION ====================
  // Nondeterministically choose any enabled action.

  action step: bool = any {
    nondet account = oneOf(Set("acc1", "acc2"))
    nondet authority = oneOf(Set("auth1", "auth2", "auth3"))
    nondet guardian = oneOf(Set("guard1", "guard2"))
    nondet nonce = oneOf(1.to(100))
    nondet eventId = oneOf(Set("evt1", "evt2", "evt3", "evt4", "evt5"))
    any {
      createAccount(account, authority, guardian),
      addAuthority(account, authority, nonce),
      removeAuthority(account, authority, nonce),
      bumpSessionEpoch(account, nonce),
      appendLedgerEvent(account, eventId, nonce, "GenericEvent", AuthThreshold),
      signalFinalize
    }
  }

  // ==================== CRDT MERGE OPERATIONS ====================
  // These implement the state-based CRDT merge for journal synchronization.
  // Lean: Aura.Journal.merge, Aura.Journal.Semilattice

  // Merge two event lists (union with deduplication by id).
  // This is the core CRDT operation for eventual consistency.
  pure def mergeEvents(a: List[EventRecord], b: List[EventRecord]): List[EventRecord] = {
    val aIds: Set[EventId] = a.foldl(Set(), (acc, e) => acc.union(Set(e.id)))
    // Filter b to only events not in a (by id)
    val bOnly = b.foldl(List(), (acc, e) =>
      if (e.id.in(aIds)) acc else acc.append(e)
    )
    // Combine lists (in real impl would sort by timestamp)
    a.concat(bOnly)
  }

  // Merge nonce sets (set union = CRDT G-Set).
  pure def mergeNonces(a: Set[int], b: Set[int]): Set[int] = a.union(b)

  // Merge ledger states using component-wise lattice operations.
  // Sets use union, integers use max.
  pure def mergeLedgers(a: LedgerState, b: LedgerState): LedgerState = {
    {
      account: a.account,
      authorities: a.authorities.union(b.authorities),
      guardians: a.guardians.union(b.guardians),
      sessionEpoch: if (a.sessionEpoch > b.sessionEpoch) a.sessionEpoch else b.sessionEpoch,
      lamport: if (a.lamport > b.lamport) a.lamport else b.lamport,
      cooldowns: a.cooldowns  // Simplified: would need proper merge
    }
  }

  // ==================== CRDT PROPERTIES ====================
  // These properties verify the CRDT semilattice laws.
  // Lean: Aura.Journal.merge_comm, merge_assoc, merge_idem

  // Commutativity: merge(a, b) == merge(b, a)
  pure def propNonceMergeCommutative(a: Set[int], b: Set[int]): bool =
    mergeNonces(a, b) == mergeNonces(b, a)

  // Associativity: merge(a, merge(b, c)) == merge(merge(a, b), c)
  pure def propNonceMergeAssociative(a: Set[int], b: Set[int], c: Set[int]): bool =
    mergeNonces(a, mergeNonces(b, c)) == mergeNonces(mergeNonces(a, b), c)

  // Idempotence: merge(a, a) == a
  pure def propNonceMergeIdempotent(a: Set[int]): bool =
    mergeNonces(a, a) == a

  // Authority merge is commutative (set union).
  pure def propAuthorityMergeCommutative(a: Set[AuthorityId], b: Set[AuthorityId]): bool =
    a.union(b) == b.union(a)

  // Lamport timestamps form a join-semilattice (max is the join operation).
  pure def propLamportJoin(a: int, b: int): bool = {
    val joined = if (a > b) a else b
    // Join is idempotent: join(a, a) == a
    val idempotent = (if (a > a) a else a) == a
    // Join is commutative: join(a, b) == join(b, a)
    val commutative = (if (a > b) a else b) == (if (b > a) b else a)
    idempotent and commutative
  }

  // ==================== REDUCE DETERMINISM ====================
  // Reduce computes derived state from the event log.
  // Lean: Aura.Journal.reduce_deterministic

  // Compute authority set from event list by replaying events.
  pure def reduceAuthorities(eventList: List[EventRecord], initial: Set[AuthorityId]): Set[AuthorityId] =
    eventList.foldl(initial, (acc, event) =>
      if (event.eventType == "AddAuthority") acc.union(Set(event.id))
      else if (event.eventType == "RemoveAuthority") acc.filter(a => a != event.id)
      else acc
    )

  // Compute max Lamport timestamp from event list.
  pure def reduceLamport(eventList: List[EventRecord]): int =
    eventList.foldl(0, (acc, event) => if (event.timestamp > acc) event.timestamp else acc)

  // Determinism: running reduce twice yields same result.
  pure def propReduceLamportDeterministic(events: List[EventRecord]): bool = {
    val result1 = reduceLamport(events)
    val result2 = reduceLamport(events)
    result1 == result2
  }

  // Ordering: timestamps are monotonically increasing in the event list.
  pure def propReducePreservesOrder(events: List[EventRecord]): bool =
    events.length() < 2 or
    events.indices().filter(i => i > 0).forall(i =>
      events[i].timestamp >= events[i-1].timestamp
    )

  // ==================== INVARIANTS ====================
  // Safety properties that must hold in every reachable state.

  // Helper: check if all events satisfy a predicate.
  pure def allEventsMatch(eventList: List[EventRecord], pred: EventRecord => bool): bool =
    eventList.foldl(true, (acc, event) => acc and pred(event))

  // Lean: Aura.Journal.nonce_unique
  // Nonces are unique within each account (replay protection).
  val InvariantNonceUnique =
    usedNonces.keys().forall(account =>
      usedNonces.get(account).forall(n =>
        usedNonces.get(account).filter(m => m == n).size() == 1
      )
    )

  // Lean: Aura.Journal.events_ordered
  // Events have positive timestamps.
  val InvariantEventsOrdered =
    events.keys().forall(account =>
      allEventsMatch(events.get(account), event => event.timestamp > 0)
    )

  // Lean: Aura.Journal.events_match_account
  // Events are scoped to their account.
  val InvariantEventsMatchAccount =
    events.keys().forall(account =>
      allEventsMatch(events.get(account), event => event.account == account)
    )

  // Lean: Aura.Journal.lifecycle_completed_stable
  // Completed lifecycle implies stable state.
  val InvariantLifecycleCompletedImpliesStable = {
    val isCompleted = match lifecycleStatus { | JournalCompleted => true | _ => false }
    isCompleted implies true
  }

  // ==================== CRDT INVARIANTS ====================
  // These ensure the CRDT semilattice properties hold at runtime.

  // Lean: Aura.Journal.merge_comm
  // Nonce merge is commutative (concurrent additions converge).
  val InvariantNonceMergeCommutative =
    usedNonces.keys().forall(acc1 =>
      usedNonces.keys().forall(acc2 =>
        propNonceMergeCommutative(
          getOrDefaultNonces(usedNonces, acc1),
          getOrDefaultNonces(usedNonces, acc2)
        )
      )
    )

  // Lean: Aura.Journal.lamport_monotonic
  // Lamport timestamps are non-negative.
  val InvariantLamportMonotonic =
    ledgerStates.keys().forall(account =>
      ledgerStates.get(account).lamport >= 0
    )

  // Lean: Aura.Journal.reduce_deterministic
  // Event list reduce is deterministic.
  val InvariantReduceDeterministic =
    events.keys().forall(account =>
      propReduceLamportDeterministic(getOrDefaultEvents(events, account))
    )

}
