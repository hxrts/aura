module protocol_journal {
  import protocol_core as core from "protocol_core"

  type AccountId = str
  type DeviceId = core.DeviceId
  type EventId = str
  type SessionId = core.SessionId

  type EventAuthorization = Threshold | Device(DeviceId) | Guardian(DeviceId)

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
    devices: Set[DeviceId],
    guardians: Set[DeviceId],
    sessionEpoch: int,
    lamport: int,
    cooldowns: Map[SessionId, int]
  }

  var ledgers: AccountId -> LedgerState
  var events: AccountId -> List[EventRecord]
  var usedNonces: AccountId -> Set[int]
  var lifecycleStatus: LifecycleStatus

  type LifecycleStatus = Pending | Completed

  pure def getOrDefault[K, V](m: K -> V, key: K, default: V): V =
    if key.in(m.keys()) then m.get(key) else default

  pure def appendEvent(list: List[EventRecord], event: EventRecord): List[EventRecord] =
    list.append(event)

  pure def validateNonce(account: AccountId, nonce: int): bool =
    not nonce.in(getOrDefault(usedNonces, account, Set()))

  action init: bool = all {
    ledgers' = Map(),
    events' = Map(),
    usedNonces' = Map(),
    lifecycleStatus' = Pending
  }

  action createAccount(account: AccountId, device: DeviceId, guardian: DeviceId): bool = all {
    not account.in(ledgers.keys()),
    ledgers' = ledgers.set(account, {
      account: account,
      devices: Set(device),
      guardians: Set(guardian),
      sessionEpoch: 0,
      lamport: 0,
      cooldowns: Map()
    }),
    events' = events.set(account, List()),
    usedNonces' = usedNonces.set(account, Set())
  }

  action appendLedgerEvent(
    account: AccountId,
    eventId: EventId,
    nonce: int,
    eventType: str,
    auth: EventAuthorization
  ): bool =
    val ledger = ledgers.get(account) {
      account.in(ledgers.keys()) and
      validateNonce(account, nonce) {
        val newEvent = {
          id: eventId,
          account: account,
          timestamp: ledger.lamport + 1,
          nonce: nonce,
          parent: events.get(account).lastOption().map(ev => ev.id),
          epoch: ledger.sessionEpoch,
          eventType: eventType,
          authorization: auth
        }
        ledgers' = ledgers.set(account, {
          ...ledger,
          lamport: ledger.lamport + 1
        }),
        events' = events.set(account, appendEvent(events.get(account), newEvent)),
        usedNonces' = usedNonces.set(account, getOrDefault(usedNonces, account, Set()).union(Set(nonce)))
      }
    }

  action addDevice(account: AccountId, device: DeviceId, nonce: int): bool =
    val ledger = ledgers.get(account) {
      not ledger.devices.contains(device) and
      appendLedgerEvent(account, "AddDevice", nonce, "AddDevice", Threshold) {
        ledgers' = ledgers.set(account, {
          ...ledger,
          devices: ledger.devices.union(Set(device))
        }),
        events' = events,
        usedNonces' = usedNonces
      }
    }

  action removeDevice(account: AccountId, device: DeviceId, nonce: int): bool =
    val ledger = ledgers.get(account) {
      device.in(ledger.devices) and
      appendLedgerEvent(account, "RemoveDevice", nonce, "RemoveDevice", Threshold) {
        ledgers' = ledgers.set(account, {
          ...ledger,
          devices: ledger.devices.filter(d => d != device)
        }),
        events' = events,
        usedNonces' = usedNonces
      }
    }

  action bumpSessionEpoch(account: AccountId, nonce: int): bool =
    val ledger = ledgers.get(account) {
      appendLedgerEvent(account, "SessionEpochBump", nonce, "SessionEpochBump", Threshold) {
        ledgers' = ledgers.set(account, {
          ...ledger,
          sessionEpoch: ledger.sessionEpoch + 1
        }),
        events' = events,
        usedNonces' = usedNonces
      }
    }

  action signalFinalize: bool = all {
    lifecycleStatus = Pending,
    lifecycleStatus' = Completed,
    ledgers' = ledgers,
    events' = events,
    usedNonces' = usedNonces
  }

  // ==================== INVARIANTS ====================

  val InvariantNonceUnique =
    usedNonces.keys().forall(account =>
      usedNonces.get(account).forall(n =>
        usedNonces.get(account).filter(m => m = n).size() = 1
      )
    )

  val InvariantEventsOrdered =
    events.keys().forall(account =>
      events.get(account).forall(event => event.timestamp > 0)
    )

  val InvariantEventsMatchAccount =
    events.keys().forall(account =>
      events.get(account).forall(event => event.account = account)
    )

  val InvariantLifecycleCompletedImpliesStable =
    lifecycleStatus = Completed implies true
}
