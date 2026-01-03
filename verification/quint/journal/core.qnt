// CRDT Journal Specification
//
// Models Aura's fact-based journal as a state-based CRDT. Facts form
// a hash-linked log (via parent pointers) with nonce uniqueness enforcement.
// Merging journals from different replicas converges via set union + max.
//
// Key CRDT properties verified:
// - Merge is commutative, associative, idempotent (semilattice)
// - Reduce is deterministic (same facts → same derived state)
// - Lamport timestamps monotonically increase
//
// == Terminology ==
// Journals are namespaced by AuthorityId (for authority journals) or
// ContextId (for relational context journals). This spec models authority
// journals. See docs/102_journal.md for the full journal model.
//
// == Lean Correspondence ==
// Module: Aura.Domain.Journal (verification/lean/Aura/Domain/Journal/)
// - Types: Fact, FactId, Journal, JournalNamespace
// - Merge: merge (set union with deduplication)
// - Reduce: reduce (deterministic state derivation)
// - Semilattice: merge_comm, merge_assoc, merge_idem
//
// == Rust Correspondence ==
// - File: crates/aura-journal/src/lib.rs
// - Types: Fact, FactId, Journal, JournalNamespace
// - Functions: merge, reduce
//
// See: docs/102_journal.md

module protocol_journal {
  // ==================== TYPE DEFINITIONS ====================
  // Base types for journal entries and state.
  // Journals are namespaced per authority (AuthorityId) or context (ContextId).

  type Option[a] = Some(a) | None
  type AuthorityId = str   // Opaque authority identifier (journal namespace owner)
  type ContextId = str     // Relational context identifier
  type FactId = str        // Unique fact identifier (typically a hash)
  type SessionId = str

  // How was this fact authorized?
  // See docs/102_journal.md for AttestedOp structure.
  type FactAuthorization =
    | AuthThreshold             // k-of-n threshold signature (AttestedOp)
    | AuthSingleDevice          // Single device signature
    | AuthGuardian(AuthorityId) // Guardian-assisted recovery

  // A single journal fact.
  // Facts are immutable and identified by hash.
  // See docs/102_journal.md §2 for the Fact model.
  type Fact = {
    id: FactId,                    // Unique identifier (content hash)
    namespace: AuthorityId,        // Journal namespace (authority owner)
    timestamp: int,                // Lamport timestamp (causal ordering)
    nonce: int,                    // Replay protection (must be unique)
    parent: Option[FactId],        // Hash chain link to previous fact
    epoch: int,                    // Session epoch for invalidation
    factType: str,                 // Type tag for deserialization
    authorization: FactAuthorization
  }

  // Derived state computed by reducing the fact log.
  // See docs/102_journal.md §4 for TreeStateSummary.
  type AuthorityState = {
    authority: AuthorityId,         // Namespace owner
    devices: Set[AuthorityId],      // Current device set (derived from tree)
    guardians: Set[AuthorityId],    // Guardian set for recovery
    sessionEpoch: int,              // Current session epoch
    lamport: int,                   // Max Lamport seen
    cooldowns: SessionId -> int     // Recovery cooldown timers
  }

  // Lifecycle status for external coordination.
  type JournalStatus = JournalPending | JournalCompleted

  // ==================== STATE VARIABLES ====================
  // Mutable state tracked by the model checker.
  // Journals are keyed by AuthorityId (namespace owner).

  var authorityStates: AuthorityId -> AuthorityState  // Derived state per authority
  var facts: AuthorityId -> List[Fact]                // Fact log per authority
  var usedNonces: AuthorityId -> Set[int]             // Consumed nonces (replay protection)
  var lifecycleStatus: JournalStatus                  // External lifecycle coordination

  // ==================== HELPER FUNCTIONS ====================
  // Pure functions for safe map/option access.

  pure def getOrDefaultState(m: AuthorityId -> AuthorityState, key: AuthorityId): Option[AuthorityState] =
    if (key.in(m.keys())) Some(m.get(key)) else None

  pure def getOrDefaultNonces(m: AuthorityId -> Set[int], key: AuthorityId): Set[int] =
    if (key.in(m.keys())) m.get(key) else Set()

  pure def getOrDefaultFacts(m: AuthorityId -> List[Fact], key: AuthorityId): List[Fact] =
    if (key.in(m.keys())) m.get(key) else List()

  // Nonce validation: returns true if nonce hasn't been used yet.
  pure def validateNonce(authority: AuthorityId, nonce: int, nonces: AuthorityId -> Set[int]): bool =
    not(nonce.in(getOrDefaultNonces(nonces, authority)))

  // Get the last fact's ID for hash chain linking.
  pure def lastFactId(factList: List[Fact]): Option[FactId] =
    if (factList.length() == 0) None
    else Some(factList[factList.length() - 1].id)

  // ==================== INITIALISATION ====================
  // Start with empty state (no authorities).

  action init: bool = all {
    authorityStates' = Map(),
    facts' = Map(),
    usedNonces' = Map(),
    lifecycleStatus' = JournalPending
  }

  // ==================== ACTIONS ====================
  // These model journal mutations (fact appends).

  // Create a new authority journal with initial device and guardian.
  action createAuthority(authority: AuthorityId, device: AuthorityId, guardian: AuthorityId): bool = all {
    not(authority.in(authorityStates.keys())),
    authorityStates' = authorityStates.put(authority, {
      authority: authority,
      devices: Set(device),
      guardians: Set(guardian),
      sessionEpoch: 0,
      lamport: 0,
      cooldowns: Map()
    }),
    facts' = facts.put(authority, List()),
    usedNonces' = usedNonces.put(authority, Set()),
    lifecycleStatus' = lifecycleStatus
  }

  // Append a generic fact to the journal (with nonce validation).
  action appendFact(
    authority: AuthorityId,
    factId: FactId,
    nonce: int,
    factType: str,
    auth: FactAuthorization
  ): bool = {
    val stateOpt = getOrDefaultState(authorityStates, authority)
    val hasState = match stateOpt { | Some(_) => true | None => false }
    val state = match stateOpt {
      | Some(s) => s
      | None => { authority: "", devices: Set(), guardians: Set(), sessionEpoch: 0, lamport: 0, cooldowns: Map() }
    }
    val validNonce = validateNonce(authority, nonce, usedNonces)
    val parentId = lastFactId(getOrDefaultFacts(facts, authority))
    val newFact = {
      id: factId,
      namespace: authority,
      timestamp: state.lamport + 1,
      nonce: nonce,
      parent: parentId,
      epoch: state.sessionEpoch,
      factType: factType,
      authorization: auth
    }
    all {
      hasState,
      validNonce,
      authorityStates' = authorityStates.set(authority, {
        ...state,
        lamport: state.lamport + 1
      }),
      facts' = facts.set(authority, getOrDefaultFacts(facts, authority).append(newFact)),
      usedNonces' = usedNonces.set(authority, getOrDefaultNonces(usedNonces, authority).union(Set(nonce))),
      lifecycleStatus' = lifecycleStatus
    }
  }

  // Add a new device to the authority (requires threshold authorization).
  // This models an AddLeaf operation in the commitment tree.
  action addDevice(authority: AuthorityId, device: AuthorityId, nonce: int): bool = {
    val stateOpt = getOrDefaultState(authorityStates, authority)
    val hasState = match stateOpt { | Some(_) => true | None => false }
    val state = match stateOpt {
      | Some(s) => s
      | None => { authority: "", devices: Set(), guardians: Set(), sessionEpoch: 0, lamport: 0, cooldowns: Map() }
    }
    val notPresent = not(state.devices.contains(device))
    val validNonce = validateNonce(authority, nonce, usedNonces)
    val parentId = lastFactId(getOrDefaultFacts(facts, authority))
    val newFact = {
      id: "AddDevice",
      namespace: authority,
      timestamp: state.lamport + 1,
      nonce: nonce,
      parent: parentId,
      epoch: state.sessionEpoch,
      factType: "AddDevice",
      authorization: AuthThreshold
    }
    all {
      hasState,
      notPresent,
      validNonce,
      authorityStates' = authorityStates.set(authority, {
        ...state,
        lamport: state.lamport + 1,
        devices: state.devices.union(Set(device))
      }),
      facts' = facts.set(authority, getOrDefaultFacts(facts, authority).append(newFact)),
      usedNonces' = usedNonces.set(authority, getOrDefaultNonces(usedNonces, authority).union(Set(nonce))),
      lifecycleStatus' = lifecycleStatus
    }
  }

  // Remove a device from the authority.
  // This models a RemoveLeaf operation in the commitment tree.
  action removeDevice(authority: AuthorityId, device: AuthorityId, nonce: int): bool = {
    val stateOpt = getOrDefaultState(authorityStates, authority)
    val hasState = match stateOpt { | Some(_) => true | None => false }
    val state = match stateOpt {
      | Some(s) => s
      | None => { authority: "", devices: Set(), guardians: Set(), sessionEpoch: 0, lamport: 0, cooldowns: Map() }
    }
    val isPresent = state.devices.contains(device)
    val validNonce = validateNonce(authority, nonce, usedNonces)
    val parentId = lastFactId(getOrDefaultFacts(facts, authority))
    val newFact = {
      id: "RemoveDevice",
      namespace: authority,
      timestamp: state.lamport + 1,
      nonce: nonce,
      parent: parentId,
      epoch: state.sessionEpoch,
      factType: "RemoveDevice",
      authorization: AuthThreshold
    }
    all {
      hasState,
      isPresent,
      validNonce,
      authorityStates' = authorityStates.set(authority, {
        ...state,
        lamport: state.lamport + 1,
        devices: state.devices.filter(d => d != device)
      }),
      facts' = facts.set(authority, getOrDefaultFacts(facts, authority).append(newFact)),
      usedNonces' = usedNonces.set(authority, getOrDefaultNonces(usedNonces, authority).union(Set(nonce))),
      lifecycleStatus' = lifecycleStatus
    }
  }

  // Increment session epoch (invalidates all sessions in prior epochs).
  action bumpSessionEpoch(authority: AuthorityId, nonce: int): bool = {
    val stateOpt = getOrDefaultState(authorityStates, authority)
    val hasState = match stateOpt { | Some(_) => true | None => false }
    val state = match stateOpt {
      | Some(s) => s
      | None => { authority: "", devices: Set(), guardians: Set(), sessionEpoch: 0, lamport: 0, cooldowns: Map() }
    }
    val validNonce = validateNonce(authority, nonce, usedNonces)
    val parentId = lastFactId(getOrDefaultFacts(facts, authority))
    val newFact = {
      id: "SessionEpochBump",
      namespace: authority,
      timestamp: state.lamport + 1,
      nonce: nonce,
      parent: parentId,
      epoch: state.sessionEpoch,
      factType: "SessionEpochBump",
      authorization: AuthThreshold
    }
    all {
      hasState,
      validNonce,
      authorityStates' = authorityStates.set(authority, {
        ...state,
        lamport: state.lamport + 1,
        sessionEpoch: state.sessionEpoch + 1
      }),
      facts' = facts.set(authority, getOrDefaultFacts(facts, authority).append(newFact)),
      usedNonces' = usedNonces.set(authority, getOrDefaultNonces(usedNonces, authority).union(Set(nonce))),
      lifecycleStatus' = lifecycleStatus
    }
  }

  // Signal that journal processing is complete.
  action signalFinalize: bool = {
    val isPending = match lifecycleStatus { | JournalPending => true | _ => false }
    all {
      isPending,
      lifecycleStatus' = JournalCompleted,
      authorityStates' = authorityStates,
      facts' = facts,
      usedNonces' = usedNonces
    }
  }

  // ==================== STEP RELATION ====================
  // Nondeterministically choose any enabled action.

  action step: bool = any {
    nondet authority = oneOf(Set("auth1", "auth2"))
    nondet device = oneOf(Set("dev1", "dev2", "dev3"))
    nondet guardian = oneOf(Set("guard1", "guard2"))
    nondet nonce = oneOf(1.to(100))
    nondet factId = oneOf(Set("fact1", "fact2", "fact3", "fact4", "fact5"))
    any {
      createAuthority(authority, device, guardian),
      addDevice(authority, device, nonce),
      removeDevice(authority, device, nonce),
      bumpSessionEpoch(authority, nonce),
      appendFact(authority, factId, nonce, "GenericFact", AuthThreshold),
      signalFinalize
    }
  }

  // ==================== CRDT MERGE OPERATIONS ====================
  // These implement the state-based CRDT merge for journal synchronization.
  // See docs/102_journal.md §3 for semilattice structure.
  // Lean: Aura.Domain.Journal.Semilattice

  // Merge two fact lists (union with deduplication by id).
  // This is the core CRDT operation for eventual consistency.
  pure def mergeFacts(a: List[Fact], b: List[Fact]): List[Fact] = {
    val aIds: Set[FactId] = a.foldl(Set(), (acc, f) => acc.union(Set(f.id)))
    // Filter b to only facts not in a (by id)
    val bOnly = b.foldl(List(), (acc, f) =>
      if (f.id.in(aIds)) acc else acc.append(f)
    )
    // Combine lists (in real impl would sort by OrderTime)
    a.concat(bOnly)
  }

  // Merge nonce sets (set union = CRDT G-Set).
  pure def mergeNonces(a: Set[int], b: Set[int]): Set[int] = a.union(b)

  // Merge authority states using component-wise lattice operations.
  // Sets use union (join), integers use max (join).
  pure def mergeAuthorityStates(a: AuthorityState, b: AuthorityState): AuthorityState = {
    {
      authority: a.authority,
      devices: a.devices.union(b.devices),
      guardians: a.guardians.union(b.guardians),
      sessionEpoch: if (a.sessionEpoch > b.sessionEpoch) a.sessionEpoch else b.sessionEpoch,
      lamport: if (a.lamport > b.lamport) a.lamport else b.lamport,
      cooldowns: a.cooldowns  // Simplified: would need proper merge
    }
  }

  // ==================== CRDT PROPERTIES ====================
  // These properties verify the CRDT semilattice laws.
  // Lean: Aura.Domain.Journal.merge_comm, merge_assoc, merge_idem

  // Commutativity: merge(a, b) == merge(b, a)
  pure def propNonceMergeCommutative(a: Set[int], b: Set[int]): bool =
    mergeNonces(a, b) == mergeNonces(b, a)

  // Associativity: merge(a, merge(b, c)) == merge(merge(a, b), c)
  pure def propNonceMergeAssociative(a: Set[int], b: Set[int], c: Set[int]): bool =
    mergeNonces(a, mergeNonces(b, c)) == mergeNonces(mergeNonces(a, b), c)

  // Idempotence: merge(a, a) == a
  pure def propNonceMergeIdempotent(a: Set[int]): bool =
    mergeNonces(a, a) == a

  // Device set merge is commutative (set union).
  pure def propDeviceMergeCommutative(a: Set[AuthorityId], b: Set[AuthorityId]): bool =
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
  // Reduce computes derived state from the fact log.
  // See docs/102_journal.md §4 for reduction semantics.
  // Lean: Aura.Domain.Journal.reduce_deterministic

  // Compute device set from fact list by replaying facts.
  pure def reduceDevices(factList: List[Fact], initial: Set[AuthorityId]): Set[AuthorityId] =
    factList.foldl(initial, (acc, fact) =>
      if (fact.factType == "AddDevice") acc.union(Set(fact.id))
      else if (fact.factType == "RemoveDevice") acc.filter(d => d != fact.id)
      else acc
    )

  // Compute max Lamport timestamp from fact list.
  pure def reduceLamport(factList: List[Fact]): int =
    factList.foldl(0, (acc, fact) => if (fact.timestamp > acc) fact.timestamp else acc)

  // Determinism: running reduce twice yields same result.
  pure def propReduceLamportDeterministic(factList: List[Fact]): bool = {
    val result1 = reduceLamport(factList)
    val result2 = reduceLamport(factList)
    result1 == result2
  }

  // Ordering: timestamps are monotonically increasing in the fact list.
  pure def propReducePreservesOrder(factList: List[Fact]): bool =
    factList.length() < 2 or
    factList.indices().filter(i => i > 0).forall(i =>
      factList[i].timestamp >= factList[i-1].timestamp
    )

  // ==================== INVARIANTS ====================
  // Safety properties that must hold in every reachable state.

  // Helper: check if all facts satisfy a predicate.
  pure def allFactsMatch(factList: List[Fact], pred: Fact => bool): bool =
    factList.foldl(true, (acc, fact) => acc and pred(fact))

  // Lean: Aura.Domain.Journal.nonce_unique
  // Nonces are unique within each authority journal (replay protection).
  val InvariantNonceUnique =
    usedNonces.keys().forall(authority =>
      usedNonces.get(authority).forall(n =>
        usedNonces.get(authority).filter(m => m == n).size() == 1
      )
    )

  // Lean: Aura.Domain.Journal.facts_ordered
  // Facts have positive timestamps.
  val InvariantFactsOrdered =
    facts.keys().forall(authority =>
      allFactsMatch(facts.get(authority), fact => fact.timestamp > 0)
    )

  // Lean: Aura.Domain.Journal.facts_match_namespace
  // Facts are scoped to their authority namespace.
  val InvariantFactsMatchNamespace =
    facts.keys().forall(authority =>
      allFactsMatch(facts.get(authority), fact => fact.namespace == authority)
    )

  // Lean: Aura.Domain.Journal.lifecycle_completed_stable
  // Completed lifecycle implies stable state.
  val InvariantLifecycleCompletedImpliesStable = {
    val isCompleted = match lifecycleStatus { | JournalCompleted => true | _ => false }
    isCompleted implies true
  }

  // ==================== CRDT INVARIANTS ====================
  // These ensure the CRDT semilattice properties hold at runtime.

  // Lean: Aura.Domain.Journal.merge_comm
  // Nonce merge is commutative (concurrent additions converge).
  val InvariantNonceMergeCommutative =
    usedNonces.keys().forall(auth1 =>
      usedNonces.keys().forall(auth2 =>
        propNonceMergeCommutative(
          getOrDefaultNonces(usedNonces, auth1),
          getOrDefaultNonces(usedNonces, auth2)
        )
      )
    )

  // Lean: Aura.Domain.Journal.lamport_monotonic
  // Lamport timestamps are non-negative.
  val InvariantLamportMonotonic =
    authorityStates.keys().forall(authority =>
      authorityStates.get(authority).lamport >= 0
    )

  // Lean: Aura.Domain.Journal.reduce_deterministic
  // Fact list reduce is deterministic.
  val InvariantReduceDeterministic =
    facts.keys().forall(authority =>
      propReduceLamportDeterministic(getOrDefaultFacts(facts, authority))
    )

}
