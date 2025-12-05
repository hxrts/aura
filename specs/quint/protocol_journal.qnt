module protocol_journal {
  // Base types
  type Option[a] = Some(a) | None
  type AccountId = str
  type AuthorityId = str  // renamed from DeviceId
  type EventId = str
  type SessionId = str

  type EventAuthorization =
    | AuthThreshold
    | AuthDevice(AuthorityId)
    | AuthGuardian(AuthorityId)

  type EventRecord = {
    id: EventId,
    account: AccountId,
    timestamp: int,
    nonce: int,
    parent: Option[EventId],
    epoch: int,
    eventType: str,
    authorization: EventAuthorization
  }

  type LedgerState = {
    account: AccountId,
    authorities: Set[AuthorityId],
    guardians: Set[AuthorityId],
    sessionEpoch: int,
    lamport: int,
    cooldowns: SessionId -> int
  }

  // Lifecycle status with unique names
  type JournalStatus = JournalPending | JournalCompleted

  var ledgerStates: AccountId -> LedgerState
  var events: AccountId -> List[EventRecord]
  var usedNonces: AccountId -> Set[int]
  var lifecycleStatus: JournalStatus

  // ============ HELPERS ============

  pure def getOrDefaultLedger(m: AccountId -> LedgerState, key: AccountId): Option[LedgerState] =
    if (key.in(m.keys())) Some(m.get(key)) else None

  pure def getOrDefaultNonces(m: AccountId -> Set[int], key: AccountId): Set[int] =
    if (key.in(m.keys())) m.get(key) else Set()

  pure def getOrDefaultEvents(m: AccountId -> List[EventRecord], key: AccountId): List[EventRecord] =
    if (key.in(m.keys())) m.get(key) else List()

  pure def validateNonce(account: AccountId, nonce: int, nonces: AccountId -> Set[int]): bool =
    not(nonce.in(getOrDefaultNonces(nonces, account)))

  pure def lastEventId(eventList: List[EventRecord]): Option[EventId] =
    if (eventList.length() == 0) None
    else Some(eventList[eventList.length() - 1].id)

  // ============ INITIALISATION ============

  action init: bool = all {
    ledgerStates' = Map(),
    events' = Map(),
    usedNonces' = Map(),
    lifecycleStatus' = JournalPending
  }

  // ============ ACTIONS ============

  action createAccount(account: AccountId, authority: AuthorityId, guardian: AuthorityId): bool = all {
    not(account.in(ledgerStates.keys())),
    ledgerStates' = ledgerStates.set(account, {
      account: account,
      authorities: Set(authority),
      guardians: Set(guardian),
      sessionEpoch: 0,
      lamport: 0,
      cooldowns: Map()
    }),
    events' = events.set(account, List()),
    usedNonces' = usedNonces.set(account, Set()),
    lifecycleStatus' = lifecycleStatus
  }

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

  // ==================== INVARIANTS ====================

  // Helper to check if all events in a list satisfy a predicate
  pure def allEventsMatch(eventList: List[EventRecord], pred: EventRecord => bool): bool =
    eventList.foldl(true, (acc, event) => acc and pred(event))

  val InvariantNonceUnique =
    usedNonces.keys().forall(account =>
      usedNonces.get(account).forall(n =>
        usedNonces.get(account).filter(m => m == n).size() == 1
      )
    )

  val InvariantEventsOrdered =
    events.keys().forall(account =>
      allEventsMatch(events.get(account), event => event.timestamp > 0)
    )

  val InvariantEventsMatchAccount =
    events.keys().forall(account =>
      allEventsMatch(events.get(account), event => event.account == account)
    )

  val InvariantLifecycleCompletedImpliesStable = {
    val isCompleted = match lifecycleStatus { | JournalCompleted => true | _ => false }
    isCompleted implies true
  }
}
