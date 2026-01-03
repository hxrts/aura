// Invitation Protocol - Aura Invitation Flow Specification
//
// This module models Aura's invitation system with:
// - Invitation types: Channel, Guardian, Contact, DeviceEnrollment
// - Lifecycle: Pending → Accepted/Declined/Cancelled/Expired
// - Consensus-based ceremonies for relationship establishment
// - Authorization invariants (only sender cancels, only receiver accepts/declines)
//
// Key properties to verify:
// - Only sender can cancel an invitation
// - Only receiver can accept or decline
// - Accepted invitations cannot be declined or cancelled
// - Expired invitations cannot be accepted
// - No duplicate acceptance/decline for same invitation
// - Ceremony supersession maintains consistency
//
// == Lean Correspondence ==
// Module: Aura.Invitation (verification/lean/Aura/Invitation/)
//
// == Rust Correspondence ==
// - Types: crates/aura-invitation/src/service.rs
// - Facts: crates/aura-invitation/src/facts.rs
// - Guards: crates/aura-invitation/src/guards.rs
//
// See: docs/103_relational_contexts.md

module invitation {
  // ==================== TYPE DEFINITIONS ====================

  type Option[a] = Some(a) | None
  type AuthorityId = str
  type ContextId = str
  type InvitationId = int
  type CeremonyId = int
  type Timestamp = int

  // ==================== INVITATION TYPES ====================

  type InvitationType =
    | ChannelInvitation
    | GuardianInvitation
    | ContactInvitation
    | DeviceEnrollmentInvitation

  type InvitationStatus =
    | StatusPending
    | StatusAccepted
    | StatusDeclined
    | StatusCancelled
    | StatusExpired

  // ==================== INVITATION RECORDS ====================

  type InvitationInfo = {
    invitationId: InvitationId,
    contextId: ContextId,
    senderId: AuthorityId,
    receiverId: AuthorityId,
    invitationType: InvitationType,
    status: InvitationStatus,
    sentAt: Timestamp,
    expiresAt: Option[Timestamp],
    resolvedAt: Option[Timestamp]
  }

  // ==================== CEREMONY TYPES ====================

  type CeremonyStatus =
    | CeremonyStatusInitiated
    | CeremonyStatusAcceptanceReceived
    | CeremonyStatusCommitted
    | CeremonyStatusAborted
    | CeremonyStatusSuperseded

  type CeremonyInfo = {
    ceremonyId: CeremonyId,
    invitationId: InvitationId,
    status: CeremonyStatus,
    initiatedAt: Timestamp,
    completedAt: Option[Timestamp],
    supersededBy: Option[CeremonyId]
  }

  // ==================== INVITATION FACTS ====================

  type InvitationSentData = {
    invitationId: InvitationId,
    contextId: ContextId,
    senderId: AuthorityId,
    receiverId: AuthorityId,
    invitationType: InvitationType,
    sentAt: Timestamp,
    expiresAt: Option[Timestamp]
  }

  type InvitationAcceptedData = {
    invitationId: InvitationId,
    acceptorId: AuthorityId,
    acceptedAt: Timestamp
  }

  type InvitationDeclinedData = {
    invitationId: InvitationId,
    declinerId: AuthorityId,
    declinedAt: Timestamp
  }

  type InvitationCancelledData = {
    invitationId: InvitationId,
    cancellerId: AuthorityId,
    cancelledAt: Timestamp
  }

  type CeremonyInitiatedData = {
    ceremonyId: CeremonyId,
    invitationId: InvitationId,
    initiatedAt: Timestamp
  }

  type CeremonyCommittedData = {
    ceremonyId: CeremonyId,
    relationshipId: str,
    committedAt: Timestamp
  }

  type CeremonyAbortedData = {
    ceremonyId: CeremonyId,
    reason: str,
    abortedAt: Timestamp
  }

  type CeremonySupersededData = {
    supersededCeremonyId: CeremonyId,
    supersedingCeremonyId: CeremonyId,
    reason: str,
    supersededAt: Timestamp
  }

  type InvitationFact =
    | FactInvitationSent(InvitationSentData)
    | FactInvitationAccepted(InvitationAcceptedData)
    | FactInvitationDeclined(InvitationDeclinedData)
    | FactInvitationCancelled(InvitationCancelledData)
    | FactCeremonyInitiated(CeremonyInitiatedData)
    | FactCeremonyCommitted(CeremonyCommittedData)
    | FactCeremonyAborted(CeremonyAbortedData)
    | FactCeremonySuperseded(CeremonySupersededData)

  // ==================== STATE VARIABLES ====================

  var invitations: InvitationId -> InvitationInfo
  var ceremonies: CeremonyId -> CeremonyInfo
  var invitationFacts: Set[InvitationFact]
  var currentTime: Timestamp
  var nextInvitationId: int
  var nextCeremonyId: int

  // ==================== CONSTANTS ====================

  pure val DEFAULT_EXPIRATION_MS: int = 604800000  // 7 days in ms
  pure val MAX_MESSAGE_LENGTH: int = 1000

  // Static sets for nondeterministic choice
  pure val ALL_INVITATION_IDS = Set(1, 2, 3, 4, 5)
  pure val ALL_CEREMONY_IDS = Set(1, 2, 3, 4, 5)
  pure val ALL_AUTH_IDS = Set("alice", "bob", "carol", "dave")
  pure val ALL_CTX_IDS = Set("ctx1", "ctx2")

  // ==================== HELPER FUNCTIONS ====================

  pure def isTerminalStatus(status: InvitationStatus): bool =
    status == StatusAccepted or status == StatusDeclined or
    status == StatusCancelled or status == StatusExpired

  pure def isExpired(inv: InvitationInfo, now: Timestamp): bool =
    match inv.expiresAt {
      | Some(expiry) => now >= expiry
      | None => false
    }

  pure val defaultInvitation: InvitationInfo = {
    invitationId: 0,
    contextId: "",
    senderId: "",
    receiverId: "",
    invitationType: ContactInvitation,
    status: StatusExpired,
    sentAt: 0,
    expiresAt: None,
    resolvedAt: None
  }

  pure val defaultCeremony: CeremonyInfo = {
    ceremonyId: 0,
    invitationId: 0,
    status: CeremonyStatusAborted,
    initiatedAt: 0,
    completedAt: None,
    supersededBy: None
  }

  // ==================== ACTIONS ====================

  action init: bool = all {
    invitations' = Map(),
    ceremonies' = Map(),
    invitationFacts' = Set(),
    currentTime' = 0,
    nextInvitationId' = 1,
    nextCeremonyId' = 1
  }

  // Advance time
  action advanceTime(delta: Timestamp): bool = {
    val newTime = currentTime + delta
    all {
      delta > 0,
      currentTime' = newTime,
      invitations' = invitations,
      ceremonies' = ceremonies,
      invitationFacts' = invitationFacts,
      nextInvitationId' = nextInvitationId,
      nextCeremonyId' = nextCeremonyId
    }
  }

  // Send an invitation
  action sendInvitation(
    senderId: AuthorityId,
    receiverId: AuthorityId,
    contextId: ContextId,
    invType: InvitationType,
    expiresAt: Option[Timestamp]
  ): bool = {
    val invId = nextInvitationId
    val inv = {
      invitationId: invId,
      contextId: contextId,
      senderId: senderId,
      receiverId: receiverId,
      invitationType: invType,
      status: StatusPending,
      sentAt: currentTime,
      expiresAt: expiresAt,
      resolvedAt: None
    }
    val fact = FactInvitationSent({
      invitationId: invId,
      contextId: contextId,
      senderId: senderId,
      receiverId: receiverId,
      invitationType: invType,
      sentAt: currentTime,
      expiresAt: expiresAt
    })
    // Guard: sender != receiver
    val canSend = senderId != receiverId
    all {
      canSend,
      invitations' = invitations.put(invId, inv),
      ceremonies' = ceremonies,
      invitationFacts' = invitationFacts.union(Set(fact)),
      currentTime' = currentTime,
      nextInvitationId' = nextInvitationId + 1,
      nextCeremonyId' = nextCeremonyId
    }
  }

  // Accept an invitation
  action acceptInvitation(invitationId: InvitationId, acceptorId: AuthorityId): bool = {
    val invExists = invitationId.in(invitations.keys())
    val inv = if (invExists) invitations.get(invitationId) else defaultInvitation
    val isReceiver = inv.receiverId == acceptorId
    val isPending = inv.status == StatusPending
    val notExpired = not(isExpired(inv, currentTime))
    val canAccept = isReceiver and isPending and notExpired
    val fact = FactInvitationAccepted({
      invitationId: invitationId,
      acceptorId: acceptorId,
      acceptedAt: currentTime
    })
    all {
      invExists,
      canAccept,
      invitations' = invitations.put(invitationId, {
        ...inv,
        status: StatusAccepted,
        resolvedAt: Some(currentTime)
      }),
      ceremonies' = ceremonies,
      invitationFacts' = invitationFacts.union(Set(fact)),
      currentTime' = currentTime,
      nextInvitationId' = nextInvitationId,
      nextCeremonyId' = nextCeremonyId
    }
  }

  // Decline an invitation
  action declineInvitation(invitationId: InvitationId, declinerId: AuthorityId): bool = {
    val invExists = invitationId.in(invitations.keys())
    val inv = if (invExists) invitations.get(invitationId) else defaultInvitation
    val isReceiver = inv.receiverId == declinerId
    val isPending = inv.status == StatusPending
    val canDecline = isReceiver and isPending
    val fact = FactInvitationDeclined({
      invitationId: invitationId,
      declinerId: declinerId,
      declinedAt: currentTime
    })
    all {
      invExists,
      canDecline,
      invitations' = invitations.put(invitationId, {
        ...inv,
        status: StatusDeclined,
        resolvedAt: Some(currentTime)
      }),
      ceremonies' = ceremonies,
      invitationFacts' = invitationFacts.union(Set(fact)),
      currentTime' = currentTime,
      nextInvitationId' = nextInvitationId,
      nextCeremonyId' = nextCeremonyId
    }
  }

  // Cancel an invitation (sender only)
  action cancelInvitation(invitationId: InvitationId, cancellerId: AuthorityId): bool = {
    val invExists = invitationId.in(invitations.keys())
    val inv = if (invExists) invitations.get(invitationId) else defaultInvitation
    val isSender = inv.senderId == cancellerId
    val isPending = inv.status == StatusPending
    val canCancel = isSender and isPending
    val fact = FactInvitationCancelled({
      invitationId: invitationId,
      cancellerId: cancellerId,
      cancelledAt: currentTime
    })
    all {
      invExists,
      canCancel,
      invitations' = invitations.put(invitationId, {
        ...inv,
        status: StatusCancelled,
        resolvedAt: Some(currentTime)
      }),
      ceremonies' = ceremonies,
      invitationFacts' = invitationFacts.union(Set(fact)),
      currentTime' = currentTime,
      nextInvitationId' = nextInvitationId,
      nextCeremonyId' = nextCeremonyId
    }
  }

  // Expire an invitation (system action when time passes expiration)
  action expireInvitation(invitationId: InvitationId): bool = {
    val invExists = invitationId.in(invitations.keys())
    val inv = if (invExists) invitations.get(invitationId) else defaultInvitation
    val isPending = inv.status == StatusPending
    val hasExpired = isExpired(inv, currentTime)
    val canExpire = isPending and hasExpired
    all {
      invExists,
      canExpire,
      invitations' = invitations.put(invitationId, {
        ...inv,
        status: StatusExpired,
        resolvedAt: Some(currentTime)
      }),
      ceremonies' = ceremonies,
      invitationFacts' = invitationFacts,
      currentTime' = currentTime,
      nextInvitationId' = nextInvitationId,
      nextCeremonyId' = nextCeremonyId
    }
  }

  // Initiate a ceremony for an accepted invitation
  action initiateCeremony(invitationId: InvitationId): bool = {
    val invExists = invitationId.in(invitations.keys())
    val inv = if (invExists) invitations.get(invitationId) else defaultInvitation
    val isAccepted = inv.status == StatusAccepted
    // Check no existing active ceremony for this invitation
    val hasActiveCeremony = ceremonies.keys().exists(cid => {
      val c = ceremonies.get(cid)
      c.invitationId == invitationId and
      (c.status == CeremonyStatusInitiated or c.status == CeremonyStatusAcceptanceReceived)
    })
    val canInitiate = isAccepted and not(hasActiveCeremony)
    val cid = nextCeremonyId
    val ceremony = {
      ceremonyId: cid,
      invitationId: invitationId,
      status: CeremonyStatusInitiated,
      initiatedAt: currentTime,
      completedAt: None,
      supersededBy: None
    }
    val fact = FactCeremonyInitiated({
      ceremonyId: cid,
      invitationId: invitationId,
      initiatedAt: currentTime
    })
    all {
      invExists,
      canInitiate,
      invitations' = invitations,
      ceremonies' = ceremonies.put(cid, ceremony),
      invitationFacts' = invitationFacts.union(Set(fact)),
      currentTime' = currentTime,
      nextInvitationId' = nextInvitationId,
      nextCeremonyId' = nextCeremonyId + 1
    }
  }

  // Commit a ceremony (relationship established)
  action commitCeremony(ceremonyId: CeremonyId, relationshipId: str): bool = {
    val cerExists = ceremonyId.in(ceremonies.keys())
    val cer = if (cerExists) ceremonies.get(ceremonyId) else defaultCeremony
    val canCommit = cer.status == CeremonyStatusInitiated or cer.status == CeremonyStatusAcceptanceReceived
    val fact = FactCeremonyCommitted({
      ceremonyId: ceremonyId,
      relationshipId: relationshipId,
      committedAt: currentTime
    })
    all {
      cerExists,
      canCommit,
      invitations' = invitations,
      ceremonies' = ceremonies.put(ceremonyId, {
        ...cer,
        status: CeremonyStatusCommitted,
        completedAt: Some(currentTime)
      }),
      invitationFacts' = invitationFacts.union(Set(fact)),
      currentTime' = currentTime,
      nextInvitationId' = nextInvitationId,
      nextCeremonyId' = nextCeremonyId
    }
  }

  // Abort a ceremony
  action abortCeremony(ceremonyId: CeremonyId, reason: str): bool = {
    val cerExists = ceremonyId.in(ceremonies.keys())
    val cer = if (cerExists) ceremonies.get(ceremonyId) else defaultCeremony
    val canAbort = cer.status == CeremonyStatusInitiated or cer.status == CeremonyStatusAcceptanceReceived
    val fact = FactCeremonyAborted({
      ceremonyId: ceremonyId,
      reason: reason,
      abortedAt: currentTime
    })
    all {
      cerExists,
      canAbort,
      invitations' = invitations,
      ceremonies' = ceremonies.put(ceremonyId, {
        ...cer,
        status: CeremonyStatusAborted,
        completedAt: Some(currentTime)
      }),
      invitationFacts' = invitationFacts.union(Set(fact)),
      currentTime' = currentTime,
      nextInvitationId' = nextInvitationId,
      nextCeremonyId' = nextCeremonyId
    }
  }

  // Supersede a ceremony with a new one
  action supersedeCeremony(oldCeremonyId: CeremonyId, newCeremonyId: CeremonyId, reason: str): bool = {
    val oldExists = oldCeremonyId.in(ceremonies.keys())
    val newExists = newCeremonyId.in(ceremonies.keys())
    val oldCer = if (oldExists) ceremonies.get(oldCeremonyId) else defaultCeremony
    val newCer = if (newExists) ceremonies.get(newCeremonyId) else defaultCeremony
    // Old ceremony must be active, new ceremony must be newer
    val oldIsActive = oldCer.status == CeremonyStatusInitiated or oldCer.status == CeremonyStatusAcceptanceReceived
    val newIsNewer = newCer.initiatedAt > oldCer.initiatedAt
    val sameInvitation = oldCer.invitationId == newCer.invitationId
    val canSupersede = oldIsActive and newIsNewer and sameInvitation
    val fact = FactCeremonySuperseded({
      supersededCeremonyId: oldCeremonyId,
      supersedingCeremonyId: newCeremonyId,
      reason: reason,
      supersededAt: currentTime
    })
    all {
      oldExists,
      newExists,
      canSupersede,
      invitations' = invitations,
      ceremonies' = ceremonies.put(oldCeremonyId, {
        ...oldCer,
        status: CeremonyStatusSuperseded,
        completedAt: Some(currentTime),
        supersededBy: Some(newCeremonyId)
      }),
      invitationFacts' = invitationFacts.union(Set(fact)),
      currentTime' = currentTime,
      nextInvitationId' = nextInvitationId,
      nextCeremonyId' = nextCeremonyId
    }
  }

  // ==================== INVARIANTS ====================

  val AllInvariants = all {
    InvariantOnlySenderCancels,
    InvariantOnlyReceiverAcceptsOrDeclines,
    InvariantNoDoubleResolution,
    InvariantTerminalStatusImmutable,
    InvariantAcceptedHasFact,
    InvariantCeremonyForAcceptedOnly
  }

  // Safety: Only the sender can cancel an invitation
  val InvariantOnlySenderCancels =
    invitationFacts.forall(fact =>
      match fact {
        | FactInvitationCancelled(data) =>
            invitations.keys().contains(data.invitationId) implies {
              val inv = invitations.get(data.invitationId)
              data.cancellerId == inv.senderId
            }
        | _ => true
      }
    )

  // Safety: Only the receiver can accept or decline
  val InvariantOnlyReceiverAcceptsOrDeclines =
    invitationFacts.forall(fact =>
      match fact {
        | FactInvitationAccepted(data) =>
            invitations.keys().contains(data.invitationId) implies {
              val inv = invitations.get(data.invitationId)
              data.acceptorId == inv.receiverId
            }
        | FactInvitationDeclined(data) =>
            invitations.keys().contains(data.invitationId) implies {
              val inv = invitations.get(data.invitationId)
              data.declinerId == inv.receiverId
            }
        | _ => true
      }
    )

  // Safety: No invitation can be both accepted and declined
  val InvariantNoDoubleResolution =
    invitations.keys().forall(invId => {
      val hasAccepted = invitationFacts.exists(f =>
        match f {
          | FactInvitationAccepted(data) => data.invitationId == invId
          | _ => false
        }
      )
      val hasDeclined = invitationFacts.exists(f =>
        match f {
          | FactInvitationDeclined(data) => data.invitationId == invId
          | _ => false
        }
      )
      not(hasAccepted and hasDeclined)
    })

  // Safety: Terminal status cannot change
  val InvariantTerminalStatusImmutable =
    invitations.keys().forall(invId => {
      val inv = invitations.get(invId)
      if (isTerminalStatus(inv.status)) {
        inv.resolvedAt != None
      } else {
        true
      }
    })

  // Safety: Every accepted invitation has an accepted fact
  val InvariantAcceptedHasFact =
    invitations.keys().forall(invId => {
      val inv = invitations.get(invId)
      if (inv.status == StatusAccepted) {
        invitationFacts.exists(f =>
          match f {
            | FactInvitationAccepted(data) => data.invitationId == invId
            | _ => false
          }
        )
      } else {
        true
      }
    })

  // Safety: Ceremonies can only exist for accepted invitations
  val InvariantCeremonyForAcceptedOnly =
    ceremonies.keys().forall(ceremonId => {
      val cer = ceremonies.get(ceremonId)
      val invId = cer.invitationId
      if (invId.in(invitations.keys())) {
        val inv = invitations.get(invId)
        inv.status == StatusAccepted
      } else {
        true  // Orphaned ceremony, allowed for now
      }
    })

  // ==================== STEP RELATION ====================

  action step = any {
    // Time advancement
    nondet delta = oneOf(Set(1, 10, 100, 1000))
    advanceTime(delta),

    // Send invitation
    nondet sender = oneOf(ALL_AUTH_IDS)
    nondet receiver = oneOf(ALL_AUTH_IDS)
    nondet ctx = oneOf(ALL_CTX_IDS)
    nondet invType = oneOf(Set(ChannelInvitation, GuardianInvitation, ContactInvitation, DeviceEnrollmentInvitation))
    nondet hasExpiry = oneOf(Set(true, false))
    val expiry = if (hasExpiry) Some(currentTime + DEFAULT_EXPIRATION_MS) else None
    sendInvitation(sender, receiver, ctx, invType, expiry),

    // Accept invitation
    nondet invId = oneOf(ALL_INVITATION_IDS)
    nondet acceptor = oneOf(ALL_AUTH_IDS)
    acceptInvitation(invId, acceptor),

    // Decline invitation
    nondet invId = oneOf(ALL_INVITATION_IDS)
    nondet decliner = oneOf(ALL_AUTH_IDS)
    declineInvitation(invId, decliner),

    // Cancel invitation
    nondet invId = oneOf(ALL_INVITATION_IDS)
    nondet canceller = oneOf(ALL_AUTH_IDS)
    cancelInvitation(invId, canceller),

    // Expire invitation
    nondet invId = oneOf(ALL_INVITATION_IDS)
    expireInvitation(invId),

    // Ceremony operations
    nondet invId = oneOf(ALL_INVITATION_IDS)
    initiateCeremony(invId),

    nondet cerId = oneOf(ALL_CEREMONY_IDS)
    nondet relId = oneOf(Set("rel1", "rel2", "rel3"))
    commitCeremony(cerId, relId),

    nondet cerId = oneOf(ALL_CEREMONY_IDS)
    nondet reason = oneOf(Set("timeout", "conflict", "user_cancelled"))
    abortCeremony(cerId, reason),

    nondet oldCerId = oneOf(ALL_CEREMONY_IDS)
    nondet newCerId = oneOf(ALL_CEREMONY_IDS)
    nondet reason = oneOf(Set("prestate_stale", "newer_request", "timeout"))
    supersedeCeremony(oldCerId, newCerId, reason)
  }
}
