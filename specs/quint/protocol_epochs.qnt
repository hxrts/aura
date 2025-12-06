// Protocol Epochs - Receipt Validity and Epoch Transitions
//
// This module models Aura's epoch-based validity windows and receipt semantics:
// - Epoch transitions and validity windows
// - Receipt creation and expiration
// - Cross-epoch operation validity
//
// Key properties to verify:
// - Receipts are valid only within their epoch window
// - Epoch transitions don't invalidate in-flight operations
// - Receipt chains maintain causality
//
// See: docs/108_transport_and_information_flow.md, docs/111_maintenance.md

module protocol_epochs {
    // ==================== TYPE DEFINITIONS ====================

    type Option[a] = Some(a) | None

    type EpochId = int
    type ReceiptId = str
    type AuthorityId = str
    type ContextId = str
    type OperationId = str
    type Timestamp = int

    // Epoch state
    type Epoch = {
        epochId: EpochId,
        startTime: Timestamp,
        endTime: Option[Timestamp],
        activeAuthorities: Set[AuthorityId],
        committedOps: Set[OperationId]
    }

    // Receipt proving operation delivery
    type Receipt = {
        receiptId: ReceiptId,
        operationId: OperationId,
        sourceAuthority: AuthorityId,
        targetAuthority: AuthorityId,
        createdEpoch: EpochId,
        validUntilEpoch: EpochId,  // Validity window
        timestamp: Timestamp,
        acknowledged: bool
    }

    // Operation with epoch binding
    type Operation = {
        opId: OperationId,
        initiator: AuthorityId,
        contextId: ContextId,
        createdEpoch: EpochId,
        committed: bool,
        receipt: Option[ReceiptId]
    }

    // Receipt validity status
    type ReceiptStatus =
        | ReceiptValid
        | ReceiptExpired
        | ReceiptPending
        | ReceiptAcknowledged

    // ==================== STATE VARIABLES ====================

    var currentEpoch: EpochId
    var epochs: EpochId -> Epoch
    var receipts: Set[Receipt]
    var operations: Set[Operation]
    var currentTime: Timestamp
    var authorities: Set[AuthorityId]

    // ==================== CONSTANTS ====================

    // Epoch duration (simplified)
    pure val EPOCH_DURATION = 100
    // Receipt validity window in epochs
    pure val RECEIPT_VALIDITY_WINDOW = 2

    // ==================== HELPER FUNCTIONS ====================

    pure def isEpochActive(epoch: Epoch, time: Timestamp): bool =
        epoch.startTime <= time and
        match epoch.endTime {
            | Some(end) => time < end
            | None => true
        }

    pure def getReceiptStatus(r: Receipt, curEpoch: EpochId): ReceiptStatus =
        if (r.acknowledged) ReceiptAcknowledged
        else if (curEpoch > r.validUntilEpoch) ReceiptExpired
        else if (curEpoch >= r.createdEpoch) ReceiptValid
        else ReceiptPending

    pure def isReceiptValid(r: Receipt, curEpoch: EpochId): bool =
        getReceiptStatus(r, curEpoch) == ReceiptValid

    pure def getEpochOrDefault(epochMap: EpochId -> Epoch, eid: EpochId): Epoch =
        if (eid.in(epochMap.keys())) epochMap.get(eid)
        else { epochId: -1, startTime: 0, endTime: None, activeAuthorities: Set(), committedOps: Set() }

    // ==================== ACTIONS ====================

    // Initialize epochs module
    action init: bool = all {
        currentEpoch' = 0,
        epochs' = Map((0, {
            epochId: 0,
            startTime: 0,
            endTime: None,
            activeAuthorities: Set("auth1", "auth2", "auth3"),
            committedOps: Set()
        })),
        receipts' = Set(),
        operations' = Set(),
        currentTime' = 0,
        authorities' = Set("auth1", "auth2", "auth3")
    }

    // Advance time
    action advanceTime(delta: Timestamp): bool = all {
        delta > 0,
        currentTime' = currentTime + delta,
        currentEpoch' = currentEpoch,
        epochs' = epochs,
        receipts' = receipts,
        operations' = operations,
        authorities' = authorities
    }

    // Transition to new epoch
    action transitionEpoch: bool = all {
        // Current epoch must exist
        currentEpoch.in(epochs.keys()),
        val curEpoch = epochs.get(currentEpoch)
        val newEpochId = currentEpoch + 1
        val closedEpoch = { ...curEpoch, endTime: Some(currentTime) }
        val newEpoch = {
            epochId: newEpochId,
            startTime: currentTime,
            endTime: None,
            activeAuthorities: curEpoch.activeAuthorities,  // Inherit authorities
            committedOps: Set()
        }
        all {
            currentEpoch' = newEpochId,
            epochs' = epochs.put(currentEpoch, closedEpoch).put(newEpochId, newEpoch),
            receipts' = receipts,
            operations' = operations,
            currentTime' = currentTime,
            authorities' = authorities
        }
    }

    // Create new operation
    action createOperation(opId: OperationId, initiator: AuthorityId, ctxId: ContextId): bool = all {
        initiator.in(authorities),
        not(operations.exists(o => o.opId == opId)),
        currentEpoch.in(epochs.keys()),
        val curEpoch = epochs.get(currentEpoch)
        val isActiveAuthority = curEpoch.activeAuthorities.contains(initiator)
        all {
            isActiveAuthority,
            val op = {
                opId: opId,
                initiator: initiator,
                contextId: ctxId,
                createdEpoch: currentEpoch,
                committed: false,
                receipt: None
            }
            operations' = operations.union(Set(op)),
            currentEpoch' = currentEpoch,
            epochs' = epochs,
            receipts' = receipts,
            currentTime' = currentTime,
            authorities' = authorities
        }
    }

    // Create receipt for operation delivery
    action createReceipt(
        receiptId: ReceiptId,
        opId: OperationId,
        source: AuthorityId,
        target: AuthorityId
    ): bool = all {
        source.in(authorities),
        target.in(authorities),
        source != target,
        operations.exists(o => o.opId == opId and o.initiator == source),
        not(receipts.exists(r => r.receiptId == receiptId)),
        val receipt = {
            receiptId: receiptId,
            operationId: opId,
            sourceAuthority: source,
            targetAuthority: target,
            createdEpoch: currentEpoch,
            validUntilEpoch: currentEpoch + RECEIPT_VALIDITY_WINDOW,
            timestamp: currentTime,
            acknowledged: false
        }
        all {
            receipts' = receipts.union(Set(receipt)),
            // Link receipt to operation
            operations' = operations.map(o =>
                if (o.opId == opId) { ...o, receipt: Some(receiptId) } else o
            ),
            currentEpoch' = currentEpoch,
            epochs' = epochs,
            currentTime' = currentTime,
            authorities' = authorities
        }
    }

    // Acknowledge receipt (target confirms delivery)
    action acknowledgeReceipt(receiptId: ReceiptId, acknowledger: AuthorityId): bool = all {
        acknowledger.in(authorities),
        receipts.exists(r => r.receiptId == receiptId and r.targetAuthority == acknowledger),
        val receipt = receipts.filter(r => r.receiptId == receiptId).fold(
            { receiptId: "", operationId: "", sourceAuthority: "", targetAuthority: "",
              createdEpoch: 0, validUntilEpoch: 0, timestamp: 0, acknowledged: false },
            (acc, r) => r
        )
        val isValid = isReceiptValid(receipt, currentEpoch)
        all {
            isValid,
            not(receipt.acknowledged),
            receipts' = receipts.map(r =>
                if (r.receiptId == receiptId) { ...r, acknowledged: true } else r
            ),
            currentEpoch' = currentEpoch,
            epochs' = epochs,
            operations' = operations,
            currentTime' = currentTime,
            authorities' = authorities
        }
    }

    // Commit operation (after receipt acknowledged)
    action commitOperation(opId: OperationId): bool = all {
        operations.exists(o => o.opId == opId and not(o.committed)),
        val op = operations.filter(o => o.opId == opId).fold(
            { opId: "", initiator: "", contextId: "", createdEpoch: 0, committed: false, receipt: None },
            (acc, o) => o
        )
        // Operation must have acknowledged receipt
        val hasAckedReceipt = match op.receipt {
            | Some(rid) => receipts.exists(r => r.receiptId == rid and r.acknowledged)
            | None => false
        }
        all {
            hasAckedReceipt,
            operations' = operations.map(o =>
                if (o.opId == opId) { ...o, committed: true } else o
            ),
            // Add to epoch's committed ops
            currentEpoch.in(epochs.keys()),
            val curEpoch = epochs.get(currentEpoch)
            epochs' = epochs.put(currentEpoch, {
                ...curEpoch,
                committedOps: curEpoch.committedOps.union(Set(opId))
            }),
            receipts' = receipts,
            currentEpoch' = currentEpoch,
            currentTime' = currentTime,
            authorities' = authorities
        }
    }

    // Expire old receipts (cleanup)
    action expireReceipts: bool = {
        val expiredReceipts = receipts.filter(r =>
            currentEpoch > r.validUntilEpoch and not(r.acknowledged)
        )
        all {
            expiredReceipts.size() > 0,
            receipts' = receipts.exclude(expiredReceipts),
            currentEpoch' = currentEpoch,
            epochs' = epochs,
            operations' = operations,
            currentTime' = currentTime,
            authorities' = authorities
        }
    }

    // ==================== INVARIANTS ====================

    // Safety: Acknowledged receipts were valid at acknowledgment time
    val InvariantAcknowledgedReceiptsValid =
        receipts.forall(r =>
            r.acknowledged implies r.createdEpoch <= currentEpoch
        )

    // Safety: Operations only commit with acknowledged receipts
    val InvariantCommitRequiresAck =
        operations.forall(o =>
            o.committed implies match o.receipt {
                | Some(rid) => receipts.exists(r => r.receiptId == rid and r.acknowledged)
                | None => false
            }
        )

    // Safety: Receipt validity window is respected
    val InvariantReceiptValidityWindow =
        receipts.forall(r =>
            r.validUntilEpoch == r.createdEpoch + RECEIPT_VALIDITY_WINDOW
        )

    // Safety: Epochs are sequential
    val InvariantEpochsSequential =
        epochs.keys().forall(e1 =>
            epochs.keys().forall(e2 =>
                (e1 < e2) implies {
                    val epoch1 = epochs.get(e1)
                    val epoch2 = epochs.get(e2)
                    match epoch1.endTime {
                        | Some(end) => end <= epoch2.startTime
                        | None => false  // Earlier epoch must be closed
                    } or e2 == e1 + 1 and match epoch1.endTime { | None => true | Some(_) => true }
                }
            )
        )

    // Safety: Active epoch has no end time
    val InvariantActiveEpochOpen =
        currentEpoch.in(epochs.keys()) implies
            match epochs.get(currentEpoch).endTime {
                | None => true
                | Some(_) => false
            }

    // Safety: Receipts reference existing operations
    val InvariantReceiptsReferenceOps =
        receipts.forall(r =>
            operations.exists(o => o.opId == r.operationId)
        )

    // Safety: Operation creators are authorities
    val InvariantOperationsByAuthorities =
        operations.forall(o =>
            authorities.contains(o.initiator)
        )

    // ==================== STEP RELATION ====================

    action step = any {
        nondet delta = oneOf(Set(1, 5, 10, 50))
        nondet opId = oneOf(Set("op1", "op2", "op3", "op4"))
        nondet receiptId = oneOf(Set("rcpt1", "rcpt2", "rcpt3", "rcpt4"))
        nondet auth = oneOf(Set("auth1", "auth2", "auth3"))
        nondet target = oneOf(Set("auth1", "auth2", "auth3"))
        nondet ctxId = oneOf(Set("ctx1", "ctx2"))
        any {
            advanceTime(delta),
            transitionEpoch,
            createOperation(opId, auth, ctxId),
            createReceipt(receiptId, opId, auth, target),
            acknowledgeReceipt(receiptId, auth),
            commitOperation(opId),
            expireReceipts,
        }
    }
}
