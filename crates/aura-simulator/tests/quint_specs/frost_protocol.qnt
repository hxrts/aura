// FROST (Flexible Round-Optimized Schnorr Threshold) Protocol
// Formal specification of Aura's FROST threshold signature implementation

module FROSTProtocol {
    // ========== FROST Types ==========

    type SessionId = str
    type Identifier = int  // FROST participant identifier
    type SigningShare = str
    type VerifyingKey = str
    type Signature = str
    type Nonce = str
    type SigningCommitment = str
    type SignatureShare = str

    type FROSTKeyShare = {
        identifier: Identifier,
        signingShare: SigningShare,
        verifyingKey: VerifyingKey
    }

    type FROSTRound =
        | FrostRound1Commitment
        | FrostRound2Signing
        | FrostAggregation
        | FrostComplete
        | FrostFailed

    type FROSTSession = {
        sessionId: SessionId,
        message: str,
        threshold: int,
        totalParticipants: int,
        currentRound: FROSTRound,
        participants: Set[Identifier],
        nonces: Identifier -> Nonce,
        commitments: Identifier -> SigningCommitment,
        signatureShares: Identifier -> SignatureShare,
        finalSignature: Signature,
        groupVerifyingKey: VerifyingKey
    }

    type FROSTParticipant = {
        identifier: Identifier,
        keyShare: FROSTKeyShare,
        currentNonce: Nonce,
        currentCommitment: SigningCommitment,
        hasCommitted: bool,
        hasSigned: bool
    }

    // ========== State Variables ==========

    var frostSessions: SessionId -> FROSTSession
    var frostParticipants: (SessionId, Identifier) -> FROSTParticipant
    var keyPackages: Identifier -> FROSTKeyShare  // Long-lived key shares

    // ========== Helper Functions ==========

    // Generate nonce and commitment (Round 1)
    // Uses identifier as a seed for deterministic generation
    pure def generateCommitment(keyShare: FROSTKeyShare): (Nonce, SigningCommitment) = {
        val nonce = if (keyShare.identifier == 1) "nonce_1"
                    else if (keyShare.identifier == 2) "nonce_2"
                    else "nonce_3"
        val commitment = if (keyShare.identifier == 1) "commit_1"
                         else if (keyShare.identifier == 2) "commit_2"
                         else "commit_3"
        (nonce, commitment)
    }

    // Create signature share (Round 2)
    pure def createSignatureShare(
        message: str,
        nonce: Nonce,
        commitments: Identifier -> SigningCommitment,
        keyShare: FROSTKeyShare
    ): SignatureShare =
        if (keyShare.identifier == 1) "share_1"
        else if (keyShare.identifier == 2) "share_2"
        else "share_3"

    // Aggregate signature shares
    pure def aggregateSignatures(
        message: str,
        commitments: Identifier -> SigningCommitment,
        shares: Identifier -> SignatureShare,
        groupKey: VerifyingKey
    ): Signature =
        "final_sig"

    // Verify signature
    pure def verifySignature(
        message: str,
        signature: Signature,
        publicKey: VerifyingKey
    ): bool =
        signature == "final_sig" and publicKey != "" and message != ""

    // Check if we have threshold commitments
    pure def hasThresholdCommitments(session: FROSTSession): bool =
        session.commitments.keys().size() >= session.threshold

    // Check if we have threshold shares
    pure def hasThresholdShares(session: FROSTSession): bool =
        session.signatureShares.keys().size() >= session.threshold

    // ========== Actions ==========

    // Initialize FROST signing session
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

    // Round 1: Generate and submit commitment
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

    // Round 2: Create and submit signature share
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

    // Aggregate shares into final signature
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

    // Fail signing session
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

    // ========== Invariants ==========

    // Threshold requirement: can't complete without threshold participants
    val thresholdInvariant = frostSessions.keys().forall(sid =>
        val session = frostSessions.get(sid)
        session.currentRound == FrostComplete implies (
            session.commitments.keys().size() >= session.threshold and
            session.signatureShares.keys().size() >= session.threshold
        )
    )

    // Commitment before signing
    val commitmentBeforeSigning = frostParticipants.keys().forall(key =>
        val participant = frostParticipants.get(key)
        participant.hasSigned implies participant.hasCommitted
    )

    // Shares only from committed participants
    val sharesFromCommitted = frostSessions.keys().forall(sid =>
        val session = frostSessions.get(sid)
        session.signatureShares.keys().subseteq(session.commitments.keys())
    )

    // Valid signature when complete
    val validSignatureInvariant = frostSessions.keys().forall(sid =>
        val session = frostSessions.get(sid)
        session.currentRound == FrostComplete implies
            verifySignature(session.message, session.finalSignature, session.groupVerifyingKey)
    )

    // ========== Initial State ==========

    action init = all {
        frostSessions' = Map(),
        frostParticipants' = Map(),
        keyPackages' = Map()
            .put(1, { identifier: 1, signingShare: "share1", verifyingKey: "vk1" })
            .put(2, { identifier: 2, signingShare: "share2", verifyingKey: "vk2" })
            .put(3, { identifier: 3, signingShare: "share3", verifyingKey: "vk3" })
    }
}
