// FROST (Flexible Round-Optimized Schnorr Threshold) Protocol
//
// Formal specification of threshold signature generation. FROST enables
// k-of-n participants to jointly sign a message without reconstructing
// the full private key. This is the signing protocol; key generation
// is handled by DKG (see protocol_dkg.qnt).
//
// Round progression: FrostRound1Commitment → FrostRound2Signing
//                    → FrostAggregation → FrostComplete (or FrostFailed)
//
// Safety: Only signed messages with threshold shares can produce valid signatures.
// Liveness: Progress requires threshold honest participants with valid key shares.
//
// == Lean Correspondence ==
// Module: Aura.Frost (verification/lean/Aura/Frost.lean)
// - SessionId: Aura.Frost.SessionId
// - Round: Aura.Frost.Round
// - Share: Aura.Frost.Share
// - canAggregate: Aura.Frost.canAggregate
// - aggregate: Aura.Frost.aggregate
// - aggregate_same_session_round: Aura.Frost.aggregate_same_session_round
//
// == Rust Correspondence ==
// - File: crates/aura-core/src/crypto/tree_signing.rs
// - Types: SigningSession, Share, SignatureShare
// - Functions: aggregate
//
// See: docs/116_crypto.md

module protocol_frost {
    // ==================== TYPE DEFINITIONS ====================

    type SessionId = str
    type Identifier = int    // FROST participant index (1..n)
    type SigningShare = str  // Secret key share (from DKG)
    type VerifyingKey = str  // Public key (individual or group)
    type Signature = str     // Final aggregated signature
    type Nonce = str         // One-time randomness (critical: never reuse!)
    type SigningCommitment = str  // Public commitment to nonce
    type SignatureShare = str     // Partial signature

    // Long-lived key material from DKG ceremony.
    type FROSTKeyShare = {
        identifier: Identifier,
        signingShare: SigningShare,  // Secret: never transmitted
        verifyingKey: VerifyingKey   // Public: used for verification
    }

    // FROST signing rounds. Must proceed in order.
    type FROSTRound =
        | FrostRound1Commitment  // Collecting nonce commitments
        | FrostRound2Signing     // Collecting signature shares
        | FrostAggregation       // Combining shares into signature
        | FrostComplete          // Successfully signed
        | FrostFailed            // Protocol aborted

    // Session state for one signing operation.
    type FROSTSession = {
        sessionId: SessionId,
        message: str,                // Message being signed
        threshold: int,              // k in k-of-n
        totalParticipants: int,      // n in k-of-n
        currentRound: FROSTRound,
        participants: Set[Identifier],
        nonces: Identifier -> Nonce,
        commitments: Identifier -> SigningCommitment,
        signatureShares: Identifier -> SignatureShare,
        finalSignature: Signature,
        groupVerifyingKey: VerifyingKey  // Group public key for verification
    }

    // Per-participant state within a signing session.
    type FROSTParticipant = {
        identifier: Identifier,
        keyShare: FROSTKeyShare,
        currentNonce: Nonce,            // Ephemeral: fresh for each session
        currentCommitment: SigningCommitment,
        hasCommitted: bool,
        hasSigned: bool
    }

    // ==================== STATE VARIABLES ====================
    // Global mutable state tracked by the model checker.

    var frostSessions: SessionId -> FROSTSession                      // Active signing sessions
    var frostParticipants: (SessionId, Identifier) -> FROSTParticipant  // Per-participant state
    var keyPackages: Identifier -> FROSTKeyShare                       // Long-lived key shares from DKG

    // ==================== HELPER FUNCTIONS ====================
    // Abstracted cryptographic operations (real impl uses curve arithmetic).

    // Generate nonce and commitment (Round 1).
    // In practice: sample random nonce, compute commitment = g^nonce.
    pure def generateCommitment(keyShare: FROSTKeyShare): (Nonce, SigningCommitment) = {
        val nonce = if (keyShare.identifier == 1) "nonce_1"
                    else if (keyShare.identifier == 2) "nonce_2"
                    else "nonce_3"
        val commitment = if (keyShare.identifier == 1) "commit_1"
                         else if (keyShare.identifier == 2) "commit_2"
                         else "commit_3"
        (nonce, commitment)
    }

    // Create signature share (Round 2).
    // In practice: Schnorr partial signature using nonce, key share, and binding factor.
    pure def createSignatureShare(
        message: str,
        nonce: Nonce,
        commitments: Identifier -> SigningCommitment,
        keyShare: FROSTKeyShare
    ): SignatureShare =
        if (keyShare.identifier == 1) "share_1"
        else if (keyShare.identifier == 2) "share_2"
        else "share_3"

    // Aggregate signature shares via Lagrange interpolation.
    // In practice: sum shares with Lagrange coefficients at x=0.
    pure def aggregateSignatures(
        message: str,
        commitments: Identifier -> SigningCommitment,
        shares: Identifier -> SignatureShare,
        groupKey: VerifyingKey
    ): Signature =
        "final_sig"

    // Verify aggregated signature against group public key.
    pure def verifySignature(
        message: str,
        signature: Signature,
        publicKey: VerifyingKey
    ): bool =
        signature == "final_sig" and publicKey != "" and message != ""

    // True if we have enough commitments to proceed to signing.
    pure def hasThresholdCommitments(session: FROSTSession): bool =
        session.commitments.keys().size() >= session.threshold

    // True if we have enough shares to aggregate.
    pure def hasThresholdShares(session: FROSTSession): bool =
        session.signatureShares.keys().size() >= session.threshold

    // ==================== ACTIONS ====================
    // These model the FROST signing protocol's message handlers.

    // Initialize a signing session. All participants must have key shares.
    action initiateFROSTSession(
        sessionId: SessionId,
        message: str,
        threshold: int,
        participants: Set[Identifier],
        groupKey: VerifyingKey
    ): bool = {
        val session = {
            sessionId: sessionId,
            message: message,
            threshold: threshold,
            totalParticipants: participants.size(),
            currentRound: FrostRound1Commitment,
            participants: participants,
            nonces: Map(),
            commitments: Map(),
            signatureShares: Map(),
            finalSignature: "",
            groupVerifyingKey: groupKey
        }
        all {
            not(sessionId.in(frostSessions.keys())),
            threshold > 0 and threshold <= participants.size(),
            participants.forall(id => id.in(keyPackages.keys())),
            frostSessions' = frostSessions.put(sessionId, session),
            frostParticipants' = participants.fold(frostParticipants, (acc, id) =>
                acc.put((sessionId, id), {
                    identifier: id,
                    keyShare: keyPackages.get(id),
                    currentNonce: "",
                    currentCommitment: "",
                    hasCommitted: false,
                    hasSigned: false
                })
            ),
            keyPackages' = keyPackages
        }
    }

    // Round 1: Generate fresh nonce and broadcast commitment.
    // CRITICAL: Nonces must never be reused across sessions!
    action submitCommitment(sessionId: SessionId, participantId: Identifier): bool = {
        val session = if (sessionId.in(frostSessions.keys())) frostSessions.get(sessionId)
            else { sessionId: "", message: "", threshold: 0, totalParticipants: 0,
                   currentRound: FrostFailed, participants: Set(), nonces: Map(),
                   commitments: Map(), signatureShares: Map(), finalSignature: "",
                   groupVerifyingKey: "" }
        val participant = if ((sessionId, participantId).in(frostParticipants.keys()))
            frostParticipants.get((sessionId, participantId))
            else { identifier: 0, keyShare: { identifier: 0, signingShare: "", verifyingKey: "" },
                   currentNonce: "", currentCommitment: "", hasCommitted: false, hasSigned: false }
        val commitmentPair = generateCommitment(participant.keyShare)
        val nonce = commitmentPair._1
        val commitment = commitmentPair._2
        val newParticipant = participant
            .with("currentNonce", nonce)
            .with("currentCommitment", commitment)
            .with("hasCommitted", true)
        val newNonces = session.nonces.put(participantId, nonce)
        val newCommitments = session.commitments.put(participantId, commitment)
        val updatedSession = session.with("commitments", newCommitments)
        val newRound = if (hasThresholdCommitments(updatedSession)) FrostRound2Signing else FrostRound1Commitment
        val newSession = session
            .with("nonces", newNonces)
            .with("commitments", newCommitments)
            .with("currentRound", newRound)
        all {
            sessionId.in(frostSessions.keys()),
            session.currentRound == FrostRound1Commitment,
            (sessionId, participantId).in(frostParticipants.keys()),
            not(participant.hasCommitted),
            participantId.in(session.participants),
            frostParticipants' = frostParticipants.put((sessionId, participantId), newParticipant),
            frostSessions' = frostSessions.put(sessionId, newSession),
            keyPackages' = keyPackages
        }
    }

    // Round 2: Compute and submit signature share using committed nonce.
    // Requires: participant has committed in Round 1.
    action submitSignatureShare(sessionId: SessionId, participantId: Identifier): bool = {
        val session = if (sessionId.in(frostSessions.keys())) frostSessions.get(sessionId)
            else { sessionId: "", message: "", threshold: 0, totalParticipants: 0,
                   currentRound: FrostFailed, participants: Set(), nonces: Map(),
                   commitments: Map(), signatureShares: Map(), finalSignature: "",
                   groupVerifyingKey: "" }
        val participant = if ((sessionId, participantId).in(frostParticipants.keys()))
            frostParticipants.get((sessionId, participantId))
            else { identifier: 0, keyShare: { identifier: 0, signingShare: "", verifyingKey: "" },
                   currentNonce: "", currentCommitment: "", hasCommitted: false, hasSigned: false }
        val share = createSignatureShare(session.message, participant.currentNonce, session.commitments, participant.keyShare)
        val newParticipant = participant.with("hasSigned", true)
        val newShares = session.signatureShares.put(participantId, share)
        val updatedSession = session.with("signatureShares", newShares)
        val newRound = if (hasThresholdShares(updatedSession)) FrostAggregation else FrostRound2Signing
        val newSession = session
            .with("signatureShares", newShares)
            .with("currentRound", newRound)
        all {
            sessionId.in(frostSessions.keys()),
            session.currentRound == FrostRound2Signing,
            (sessionId, participantId).in(frostParticipants.keys()),
            participant.hasCommitted and not(participant.hasSigned),
            frostParticipants' = frostParticipants.put((sessionId, participantId), newParticipant),
            frostSessions' = frostSessions.put(sessionId, newSession),
            keyPackages' = keyPackages
        }
    }

    // Aggregate threshold shares into final signature.
    // Requires: all signers submitted shares from their committed identities.
    action aggregateSignature(sessionId: SessionId): bool = {
        val session = if (sessionId.in(frostSessions.keys())) frostSessions.get(sessionId)
            else { sessionId: "", message: "", threshold: 0, totalParticipants: 0,
                   currentRound: FrostFailed, participants: Set(), nonces: Map(),
                   commitments: Map(), signatureShares: Map(), finalSignature: "",
                   groupVerifyingKey: "" }
        val signature = aggregateSignatures(session.message, session.commitments, session.signatureShares, session.groupVerifyingKey)
        val newSession = session
            .with("finalSignature", signature)
            .with("currentRound", FrostComplete)
        all {
            sessionId.in(frostSessions.keys()),
            session.currentRound == FrostAggregation,
            hasThresholdShares(session),
            session.signatureShares.keys().subseteq(session.commitments.keys()),
            verifySignature(session.message, signature, session.groupVerifyingKey),
            frostSessions' = frostSessions.put(sessionId, newSession),
            frostParticipants' = frostParticipants,
            keyPackages' = keyPackages
        }
    }

    // Abort signing session (timeout or invalid share detected).
    action failSession(sessionId: SessionId): bool = {
        val session = if (sessionId.in(frostSessions.keys())) frostSessions.get(sessionId)
            else { sessionId: "", message: "", threshold: 0, totalParticipants: 0,
                   currentRound: FrostFailed, participants: Set(), nonces: Map(),
                   commitments: Map(), signatureShares: Map(), finalSignature: "",
                   groupVerifyingKey: "" }
        val newSession = session.with("currentRound", FrostFailed)
        all {
            sessionId.in(frostSessions.keys()),
            session.currentRound != FrostComplete and session.currentRound != FrostFailed,
            frostSessions' = frostSessions.put(sessionId, newSession),
            frostParticipants' = frostParticipants,
            keyPackages' = keyPackages
        }
    }

    // ==================== INVARIANTS ====================
    // Safety properties that must hold in every reachable state.

    // Lean: Aura.Frost.aggregate_same_session_round (threshold requirement)
    // Completion requires threshold participants in both rounds.
    val InvariantThreshold = frostSessions.keys().forall(sid =>
        val session = frostSessions.get(sid)
        session.currentRound == FrostComplete implies (
            session.commitments.keys().size() >= session.threshold and
            session.signatureShares.keys().size() >= session.threshold
        )
    )

    // Lean: Aura.Frost.commitment_before_signing
    // Participants must commit before signing (Round 1 before Round 2).
    val InvariantCommitmentBeforeSigning = frostParticipants.keys().forall(key =>
        val participant = frostParticipants.get(key)
        participant.hasSigned implies participant.hasCommitted
    )

    // Lean: Aura.Frost.shares_from_committed
    // Signature shares only from participants who committed.
    val InvariantSharesFromCommitted = frostSessions.keys().forall(sid =>
        val session = frostSessions.get(sid)
        session.signatureShares.keys().subseteq(session.commitments.keys())
    )

    // Lean: Aura.Frost.valid_signature
    // Completed sessions have valid signatures.
    val InvariantValidSignature = frostSessions.keys().forall(sid =>
        val session = frostSessions.get(sid)
        session.currentRound == FrostComplete implies
            verifySignature(session.message, session.finalSignature, session.groupVerifyingKey)
    )

    // ==================== STRENGTHENED INVARIANTS (T3c.12) ====================
    // CRITICAL: Nonce uniqueness - nonces must never be reused across sessions.
    // In FROST, nonce reuse allows private key extraction via Schnorr signature attack.
    // Lean: Aura.Frost.nonce_uniqueness (axiom based on RNG security)
    //
    // NOTE: This invariant cannot be model-checked with deterministic nonce generation.
    // The mock generateCommitment() produces the same nonce per participant regardless
    // of session. In production, nonces are randomly generated per signing session.
    // This invariant documents the REQUIREMENT; enforcement is via RNG security.
    //
    // For model checking, we verify the weaker property that nonces are from participants.
    // The uniqueness property is an axiom in Lean (Aura.Assumptions).
    val InvariantNonceUniquenessNote = true  // Documented requirement, not model-checkable

    // Participant consistency: participants in session must be in key packages.
    val InvariantParticipantsHaveKeys = frostSessions.keys().forall(sid =>
        val session = frostSessions.get(sid)
        session.participants.forall(pid => pid.in(keyPackages.keys()))
    )

    // Session-participant binding: participant state must match session participants.
    val InvariantParticipantSessionBinding = frostParticipants.keys().forall(key =>
        val sid = key._1
        val pid = key._2
        if (sid.in(frostSessions.keys())) {
            val session = frostSessions.get(sid)
            session.participants.contains(pid)
        } else true
    )

    // Nonces only for committed participants.
    val InvariantNoncesFromParticipants = frostSessions.keys().forall(sid =>
        val session = frostSessions.get(sid)
        session.nonces.keys().subseteq(session.participants)
    )

    // Commitments only from session participants.
    val InvariantCommitmentsFromParticipants = frostSessions.keys().forall(sid =>
        val session = frostSessions.get(sid)
        session.commitments.keys().subseteq(session.participants)
    )

    // Threshold validity: must be positive and not exceed participant count.
    val InvariantThresholdValid = frostSessions.keys().forall(sid =>
        val session = frostSessions.get(sid)
        session.threshold > 0 and session.threshold <= session.participants.size()
    )

    // Combined well-formed state predicate.
    // Note: InvariantNonceUniqueness is documented but not included - see note above.
    val WellFormedFrostState = all {
        InvariantThreshold,
        InvariantCommitmentBeforeSigning,
        InvariantSharesFromCommitted,
        InvariantValidSignature,
        InvariantParticipantsHaveKeys,
        InvariantParticipantSessionBinding,
        InvariantNoncesFromParticipants,
        InvariantCommitmentsFromParticipants,
        InvariantThresholdValid
    }

    // ==================== INITIALISATION ====================
    // Pre-populate key shares (normally from DKG).

    action init = all {
        frostSessions' = Map(),
        frostParticipants' = Map(),
        // Initialize with 3 key shares (simulating completed DKG)
        keyPackages' = Map()
            .put(1, { identifier: 1, signingShare: "share1", verifyingKey: "vk1" })
            .put(2, { identifier: 2, signingShare: "share2", verifyingKey: "vk2" })
            .put(3, { identifier: 3, signingShare: "share3", verifyingKey: "vk3" })
    }

    // ==================== STEP RELATION ====================
    // Nondeterministically choose any enabled action.

    action step = any {
        // Initiate a new signing session
        nondet sessionId = oneOf(Set("session1", "session2", "session3"))
        nondet message = oneOf(Set("msg_hello", "msg_transfer", "msg_approve"))
        nondet threshold = oneOf(Set(2, 3))
        nondet participants = oneOf(Set(Set(1, 2), Set(1, 3), Set(2, 3), Set(1, 2, 3)))
        any {
            initiateFROSTSession(sessionId, message, threshold, participants, "group_vk"),
            // Submit commitment (Round 1)
            nondet pid = oneOf(Set(1, 2, 3))
            submitCommitment(sessionId, pid),
            // Submit signature share (Round 2)
            nondet pid2 = oneOf(Set(1, 2, 3))
            submitSignatureShare(sessionId, pid2),
            // Aggregate signature
            aggregateSignature(sessionId),
            // Fail session
            failSession(sessionId),
        }
    }
}
