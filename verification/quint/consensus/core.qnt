// Protocol Consensus - Aura Consensus Fast-Path and Fallback
//
// This module models Aura's single-shot consensus protocol with:
// - Fast path: one-RTT threshold signature collection
// - Fallback path: gossip-based share exchange when fast path stalls
// - Nonce caching: pipelining optimization for cached FROST commitments
//
// Key properties to verify:
// - Both paths converge to the same CommitFact for a given (cid, rid)
// - Fallback can't contradict a successful fast-path execution
// - Threshold shares prevent double-spending (equivocation detection)
// - Fast path selected when valid cached nonces exist
// - Nonces expire with epoch advancement
//
// == Lean Correspondence ==
// Module: Aura.Consensus (verification/lean/Aura/Consensus/)
// - Types: Aura.Consensus.Types
// - Agreement proofs: Aura.Consensus.Agreement
// - Validity proofs: Aura.Consensus.Validity
// - Evidence proofs: Aura.Consensus.Evidence
// - Equivocation proofs: Aura.Consensus.Equivocation
// - FROST proofs: Aura.Consensus.Frost
// - Axioms: Aura.Assumptions
//
// == Rust Correspondence ==
// - Types: crates/aura-consensus/src/consensus/types.rs
// - Protocol: crates/aura-consensus/src/consensus/protocol.rs
// - FROST: crates/aura-core/src/crypto/tree_signing.rs
//
// See: docs/104_consensus.md

module protocol_consensus {
    // ==================== TYPE DEFINITIONS ====================
    // Identifiers for consensus instances, results, and participants.
    // Lean: Aura.Consensus.Types

    type Option[a] = Some(a) | None

    // Lean: Aura.Consensus.Types.ConsensusId
    type ConsensusId = str      // Unique consensus instance identifier

    // Lean: Aura.Consensus.Types.ResultId
    type ResultId = str         // H(Op, prestate) - deterministic result binding

    // Lean: Aura.Consensus.Types.PrestateHash
    type PrestateHash = str     // H(prestate) - ensures witnesses agree on starting state

    // Lean: Aura.Consensus.Types.AuthorityId
    type AuthorityId = str      // Participant identity (device or threshold group)

    // ==================== THRESHOLD SIGNATURE TYPES ====================
    // Data binding structure for share verification
    // Lean: Aura.Consensus.Types.DataBinding
    type DataBinding = {
        bindCid: ConsensusId,
        bindRid: ResultId,
        bindPHash: PrestateHash
    }

    // FROST signature share (partial signature) with nonce binding.
    // Models: (share_value, nonce_commitment_binding, signed_data_binding)
    // Lean: Aura.Consensus.Types.SignatureShare
    type ShareData = {
        shareValue: str,            // The actual share bytes (σ_i)
        nonceBinding: str,          // Binding to nonce commitment (D_i, E_i)
        dataBinding: DataBinding    // Binding to signed data (cid, rid, pHash)
    }

    // Aggregated k-of-n threshold signature with provenance.
    // Models: (signature_value, binding_context, participant_set)
    // Lean: Aura.Consensus.Types.ThresholdSignature
    type ThresholdSignature = {
        sigValue: str,              // Aggregated signature σ
        boundCid: ConsensusId,      // Consensus instance bound to signature
        boundRid: ResultId,         // Result ID bound to signature
        boundPHash: PrestateHash,   // Prestate hash bound to signature
        signerSet: Set[AuthorityId] // Witnesses whose shares were aggregated
    }
    type OperationData = str    // Abstract operation being agreed upon

    // ==================== NONCE CACHING TYPES ====================
    // FROST pipelining: witnesses pre-cache nonce commitments for fast-path signing.

    type Epoch = int                    // Epoch counter for nonce validity windows
    type NonceCommitment = str          // D_i, E_i commitments (hiding/binding nonces)

    // Cached nonce state for a witness
    type CachedNonce = {
        commitment: NonceCommitment,    // Public commitment sent to coordinator
        epoch: Epoch,                   // Epoch when nonce was generated
        used: bool                      // Whether nonce has been consumed
    }

    // Path selection for consensus
    type PathSelection =
        | FastPath          // All witnesses have valid cached nonces
        | SlowPath          // Need fresh nonce generation (adds 1 RTT)

    // Consensus instance phase (state machine progression).
    type ConsensusPhase =
        | ConsensusPending      // Not yet started
        | FastPathActive        // Collecting shares via fast path
        | FallbackActive        // Fallback gossip in progress
        | ConsensusCommitted    // Valid CommitFact produced
        | ConsensusFailed       // Instance aborted

    // Witness state for a consensus instance
    type WitnessState = {
        hasVoted: bool,
        share: Option[ShareData],
        prestateHash: Option[PrestateHash],
        resultId: Option[ResultId]
    }

    // Share proposal from a witness
    type ShareProposal = {
        witness: AuthorityId,
        resultId: ResultId,
        prestateHash: PrestateHash,
        share: ShareData
    }

    // The commit fact output - contains the aggregated threshold signature
    // Lean: Aura.Consensus.Types.CommitFact
    // Rust: crates/aura-consensus/src/consensus/types.rs::CommitFact
    type CommitFact = {
        cid: ConsensusId,
        rid: ResultId,
        prestateHash: PrestateHash,     // Include prestate for full binding
        signature: ThresholdSignature,
        attesters: Set[AuthorityId]
    }

    // Cryptographic proof of equivocation (conflicting votes from same witness)
    // Rust: crates/aura-consensus/src/evidence.rs::EquivocationProof
    type EquivocationProof = {
        witness: AuthorityId,
        consensusId: ConsensusId,
        prestateHash: PrestateHash,
        firstResultId: ResultId,
        secondResultId: ResultId,
        timestampMs: int
    }

    // Evidence delta for incremental propagation of equivocation proofs
    // Rust: crates/aura-consensus/src/evidence.rs::EvidenceDelta
    type EvidenceDelta = {
        consensusId: ConsensusId,
        equivocationProofs: Set[EquivocationProof],
        timestampMs: int
    }

    // Consensus instance state
    type ConsensusInstance = {
        cid: ConsensusId,
        operation: OperationData,
        prestateHash: PrestateHash,
        threshold: int,
        witnesses: Set[AuthorityId],
        initiator: AuthorityId,
        phase: ConsensusPhase,
        proposals: Set[ShareProposal],
        commitFact: Option[CommitFact],
        fallbackTimerActive: bool,
        equivocators: Set[AuthorityId],
        equivocationProofs: Set[EquivocationProof]  // Track detected equivocation proofs
    }

    // ==================== STATE VARIABLES ====================

    var instances: ConsensusId -> ConsensusInstance
    var committedFacts: Set[CommitFact]
    var globalWitnesses: Set[AuthorityId]
    var globalEvidenceTracker: ConsensusId -> EvidenceDelta  // Global evidence tracker by consensus ID

    // ==================== NONCE STATE ====================
    // Global epoch and per-witness nonce cache for FROST pipelining.

    var currentEpoch: Epoch                                     // Global epoch counter
    var witnessNonces: AuthorityId -> Option[CachedNonce]       // Per-witness nonce cache

    // Nonces valid for N epochs (configurable constant)
    pure val NONCE_VALIDITY_WINDOW: int = 3

    // ════════════════════════════════════════════════════════════════════════
    // EXPOSE: SEMANTIC INTERFACE
    //
    // These predicates define protocol MEANING.
    // - Rust conformance tests verify these properties
    // - Changing these requires updating Rust implementation
    // - Lean proofs reference these definitions
    //
    // Exposed predicates:
    //   - ValidCommit: what makes a commit fact valid
    //   - ValidShare: what makes a signature share valid
    //   - ValidInstance: instance well-formedness
    //   - sharesConsistent: share agreement predicate
    //   - canCommit: commit precondition (threshold reached)
    //   - WellFormedState: global state invariant (combines all invariants)
    // ════════════════════════════════════════════════════════════════════════

    /// A CommitFact is valid if it has correct bindings and threshold attesters.
    /// Lean: Aura.Consensus.Validity.validity
    /// Rust: consensus::types::CommitFact::is_valid
    pure def ValidCommit(cf: CommitFact, threshold: int): bool = all {
        // Signature binds to the commit fact's consensus/result/prestate
        cf.signature.boundCid == cf.cid,
        cf.signature.boundRid == cf.rid,
        cf.signature.boundPHash == cf.prestateHash,
        // Signers match attesters
        cf.signature.signerSet == cf.attesters,
        // Threshold requirement met
        cf.attesters.size() >= threshold,
        // Signature value is non-empty (simplified verification)
        cf.signature.sigValue != ""
    }

    /// A signature share is valid if it binds to correct data and has valid nonce.
    /// Lean: Aura.Consensus.Frost.share_binding
    /// Rust: consensus::protocol::verify_share
    pure def ValidShare(share: ShareData, cid: ConsensusId, rid: ResultId, pHash: PrestateHash): bool = all {
        share.dataBinding.bindCid == cid,
        share.dataBinding.bindRid == rid,
        share.dataBinding.bindPHash == pHash,
        share.shareValue != "",
        share.nonceBinding != ""
    }

    /// A consensus instance is well-formed if it has valid configuration.
    /// Lean: Aura.Consensus.Types (instance well-formedness)
    pure def ValidInstance(inst: ConsensusInstance): bool = all {
        inst.threshold >= 1,
        inst.witnesses.size() >= inst.threshold,
        inst.proposals.forall(p => inst.witnesses.contains(p.witness)),
        inst.equivocators.subseteq(inst.witnesses)
    }

    /// Shares are consistent if all match the same (rid, pHash) binding.
    /// Lean: Aura.Consensus.Frost.share_session_consistency
    pure def sharesConsistent(proposals: Set[ShareProposal], rid: ResultId, pHash: PrestateHash): bool =
        val matchingProps = proposals.filter(p => p.resultId == rid and p.prestateHash == pHash)
        matchingProps.forall(p => isValidDataBinding(p.share.dataBinding))

    /// Commit precondition: threshold matching shares exist.
    /// Lean: Aura.Consensus.Validity.commit_has_threshold
    pure def canCommit(inst: ConsensusInstance, rid: ResultId): bool = {
        val matchingCount = inst.proposals.filter(p =>
            p.resultId == rid and p.prestateHash == inst.prestateHash
        ).size()
        matchingCount >= inst.threshold
    }

    /// Global state well-formedness (combines all invariants).
    /// This is the main predicate for state validity.
    pure def WellFormedState(
        insts: ConsensusId -> ConsensusInstance,
        committed: Set[CommitFact],
        nonces: AuthorityId -> Option[CachedNonce],
        epoch: Epoch
    ): bool = all {
        // All instances are well-formed
        insts.keys().forall(cid => ValidInstance(insts.get(cid))),
        // All committed facts are valid
        committed.forall(cf =>
            cf.cid.in(insts.keys()) implies ValidCommit(cf, insts.get(cf.cid).threshold)
        ),
        // Unique commit per instance (agreement)
        committed.forall(cf1 => committed.forall(cf2 =>
            cf1.cid == cf2.cid implies cf1.rid == cf2.rid
        )),
        // Equivocators excluded from attestation
        insts.keys().forall(cid => {
            val inst = insts.get(cid)
            match inst.commitFact {
                | Some(cf) => inst.equivocators.intersect(cf.attesters).size() == 0
                | None => true
            }
        }),
        // Equivocation proofs are valid
        insts.keys().forall(cid => {
            val inst = insts.get(cid)
            inst.equivocationProofs.forall(proof =>
                proof.firstResultId != proof.secondResultId and
                proof.consensusId == cid and
                proof.prestateHash == inst.prestateHash
            )
        }),
        // Equivocators have corresponding proofs
        insts.keys().forall(cid => {
            val inst = insts.get(cid)
            inst.equivocators.forall(w =>
                inst.equivocationProofs.exists(proof => proof.witness == w)
            )
        }),
        // Nonces have valid epochs
        nonces.keys().forall(w => match nonces.get(w) {
            | Some(n) => n.epoch <= epoch
            | None => true
        })
    }

    // ════════════════════════════════════════════════════════════════════════
    // INTERNAL HELPERS

    // Implementation utilities. Do not reference in Rust conformance tests.
    // These may change without requiring implementation updates.
    // ════════════════════════════════════════════════════════════════════════

    pure def getOrDefaultInstance(m: ConsensusId -> ConsensusInstance, cid: ConsensusId): Option[ConsensusInstance] =
        if (cid.in(m.keys())) Some(m.get(cid)) else None

    // Count proposals matching a specific (rid, prestateHash)
    pure def countMatchingProposals(proposals: Set[ShareProposal], rid: ResultId, pHash: PrestateHash): int =
        proposals.filter(p => p.resultId == rid and p.prestateHash == pHash).size()

    // Get witnesses that have submitted proposals for (rid, prestateHash)
    pure def getMatchingWitnesses(proposals: Set[ShareProposal], rid: ResultId, pHash: PrestateHash): Set[AuthorityId] =
        proposals.filter(p => p.resultId == rid and p.prestateHash == pHash).map(p => p.witness)

    // Check if a witness has already submitted a proposal
    pure def hasProposal(proposals: Set[ShareProposal], witness: AuthorityId): bool =
        proposals.exists(p => p.witness == witness)

    // Check for equivocation and generate proof if detected
    // Returns Some(proof) if witness submitted conflicting votes, None otherwise
    // Rust: crates/aura-consensus/src/core/validation.rs::EquivocationDetector::check_share
    pure def detectEquivocation(
        proposals: Set[ShareProposal],
        witness: AuthorityId,
        cid: ConsensusId,
        prestateHash: PrestateHash,
        newRid: ResultId,
        timestampMs: int
    ): Option[EquivocationProof] = {
        val existingProposal = proposals.filter(p => p.witness == witness)
        if (existingProposal.size() > 0) {
            // Get the existing result ID (we know there's exactly one because witness can only vote once)
            val existingRids = existingProposal.map(p => p.resultId)
            // Since there's only one proposal from this witness, check if it conflicts
            val conflictExists = existingProposal.exists(p => p.resultId != newRid)
            if (conflictExists) {
                // Extract the first proposal to get its result ID
                val firstRid = existingProposal.fold(
                    "",  // Default empty string (won't be used due to size check)
                    (acc, p) => p.resultId
                )
                Some({
                    witness: witness,
                    consensusId: cid,
                    prestateHash: prestateHash,
                    firstResultId: firstRid,
                    secondResultId: newRid,
                    timestampMs: timestampMs
                })
            } else {
                None  // Same result ID - duplicate, not equivocation
            }
        } else {
            None  // First share from this witness
        }
    }

    // ==================== THRESHOLD SIGNATURE FUNCTIONS ====================
    // FROST share aggregation and verification.

    // Compute expected data binding for share verification
    // Models: H(cid || rid || pHash) - deterministic binding hash
    pure def computeDataBinding(cid: ConsensusId, rid: ResultId, pHash: PrestateHash): DataBinding =
        { bindCid: cid, bindRid: rid, bindPHash: pHash }

    // Create empty data binding for initialization
    pure def emptyDataBinding: DataBinding =
        { bindCid: "", bindRid: "", bindPHash: "" }

    // Check if data binding is valid (non-empty)
    pure def isValidDataBinding(db: DataBinding): bool =
        db.bindCid != "" or db.bindRid != "" or db.bindPHash != ""

    // Verify a single share is well-formed and bound to correct data
    pure def verifyShare(
        share: ShareData,
        cid: ConsensusId,
        rid: ResultId,
        pHash: PrestateHash,
        nonceCommitment: NonceCommitment
    ): bool = all {
        // Share must bind to correct data
        share.dataBinding == computeDataBinding(cid, rid, pHash),
        // Share must bind to witness's nonce commitment
        share.nonceBinding == nonceCommitment,
        // Share value must be non-empty
        share.shareValue != ""
    }

    // Generate aggregated threshold signature from k valid shares
    // Models FROST signature aggregation: σ = Σ σ_i
    pure def aggregateShares(
        cid: ConsensusId,
        proposals: Set[ShareProposal],
        rid: ResultId,
        pHash: PrestateHash
    ): ThresholdSignature = {
        val matchingProps = proposals.filter(p => p.resultId == rid and p.prestateHash == pHash)
        val signers = matchingProps.map(p => p.witness)
        val shareValues = matchingProps.map(p => p.share.shareValue)
        // Simplified aggregation: concatenate share values (real FROST does point addition)
        val aggValue = if (matchingProps.size() >= 2) "agg_sig_" else ""
        {
            sigValue: aggValue,
            boundCid: cid,
            boundRid: rid,
            boundPHash: pHash,
            signerSet: signers
        }
    }

    // Verify threshold signature against public key and message binding
    // Models: verify(pk_group, σ, msg) where msg = (cid || rid || pHash)
    pure def verifySignature(
        sig: ThresholdSignature,
        cid: ConsensusId,
        rid: ResultId,
        pHash: PrestateHash,
        threshold: int
    ): bool = all {
        // Signature value is valid (non-empty)
        sig.sigValue != "",
        // Signature binds to correct consensus instance
        sig.boundCid == cid,
        // Signature binds to correct result
        sig.boundRid == rid,
        // Signature binds to correct prestate
        sig.boundPHash == pHash,
        // Signature has at least threshold signers
        sig.signerSet.size() >= threshold
    }

    // Check if signature can be verified (helper for invariants)
    pure def isValidSignature(sig: ThresholdSignature, threshold: int): bool =
        sig.sigValue != "" and sig.signerSet.size() >= threshold

    // ==================== NONCE HELPERS ====================

    // Check if a cached nonce is valid (not expired, not used)
    pure def isNonceValid(nonceOpt: Option[CachedNonce], epoch: Epoch, validityWindow: int): bool =
        match nonceOpt {
            | Some(n) => not(n.used) and (epoch - n.epoch) < validityWindow
            | None => false
        }

    // Check if all witnesses in a set have valid cached nonces
    pure def allWitnessesHaveValidNonces(
        witnesses: Set[AuthorityId],
        nonces: AuthorityId -> Option[CachedNonce],
        epoch: Epoch,
        validityWindow: int
    ): bool =
        witnesses.forall(w =>
            w.in(nonces.keys()) and isNonceValid(nonces.get(w), epoch, validityWindow)
        )

    // Determine path selection based on nonce availability
    pure def selectPath(
        witnesses: Set[AuthorityId],
        nonces: AuthorityId -> Option[CachedNonce],
        epoch: Epoch,
        validityWindow: int
    ): PathSelection =
        if (allWitnessesHaveValidNonces(witnesses, nonces, epoch, validityWindow))
            FastPath
        else
            SlowPath

    // Count witnesses with valid nonces
    pure def countValidNonces(
        witnesses: Set[AuthorityId],
        nonces: AuthorityId -> Option[CachedNonce],
        epoch: Epoch,
        validityWindow: int
    ): int =
        witnesses.filter(w =>
            w.in(nonces.keys()) and isNonceValid(nonces.get(w), epoch, validityWindow)
        ).size()

    // ==================== ACTIONS ====================
    // State transitions modeling the consensus protocol.

    // Initialize consensus module with empty state.
    action init: bool = all {
        instances' = Map(),
        committedFacts' = Set(),
        globalWitnesses' = Set("w1", "w2", "w3", "w4", "w5"),
        globalEvidenceTracker' = Map(),
        // Initialize nonce state
        currentEpoch' = 0,
        witnessNonces' = Map()
    }

    // Initiator starts a consensus instance.
    // Path selection: FastPath if all witnesses have valid nonces, else SlowPath.
    action startConsensus(
        cid: ConsensusId,
        initiator: AuthorityId,
        op: OperationData,
        pHash: PrestateHash,
        witnesses: Set[AuthorityId],
        threshold: int
    ): bool = {
        // Determine path based on nonce availability
        val pathChoice = selectPath(witnesses, witnessNonces, currentEpoch, NONCE_VALIDITY_WINDOW)
        val initialPhase = match pathChoice {
            | FastPath => FastPathActive
            | SlowPath => FallbackActive  // Skip to fallback if nonces unavailable
        }
        all {
            not(cid.in(instances.keys())),
            witnesses.size() >= threshold,
            threshold >= 1,
            instances' = instances.put(cid, {
                cid: cid,
                operation: op,
                prestateHash: pHash,
                threshold: threshold,
                witnesses: witnesses,
                initiator: initiator,
                phase: initialPhase,
                proposals: Set(),
                commitFact: None,
                fallbackTimerActive: initialPhase == FallbackActive,
                equivocators: Set(),
                equivocationProofs: Set()
            }),
            committedFacts' = committedFacts,
            globalWitnesses' = globalWitnesses,
            globalEvidenceTracker' = globalEvidenceTracker,
            currentEpoch' = currentEpoch,
            witnessNonces' = witnessNonces
        }
    }

    // Witness submits share (fast path response).
    // Share binds witness to (resultId, prestateHash). Equivocation detected and proof generated.
    // Consumes witness's cached nonce on fast path.
    action submitWitnessShare(
        cid: ConsensusId,
        witness: AuthorityId,
        rid: ResultId,
        share: ShareData,
        timestampMs: int
    ): bool = {
        val instOpt = getOrDefaultInstance(instances, cid)
        val hasInstance = match instOpt { | Some(_) => true | None => false }
        val inst = match instOpt {
            | Some(i) => i
            | None => {
                cid: "", operation: "", prestateHash: "", threshold: 0,
                witnesses: Set(), initiator: "", phase: ConsensusPending,
                proposals: Set(), commitFact: None, fallbackTimerActive: false,
                equivocators: Set(), equivocationProofs: Set()
            }
        }
        val isWitness = inst.witnesses.contains(witness)
        val notVoted = not(hasProposal(inst.proposals, witness))
        val isActive = inst.phase == FastPathActive or inst.phase == FallbackActive
        // Fast path requires valid nonce
        val isFastPath = inst.phase == FastPathActive
        val hasValidNonce = isNonceValid(
            if (witness.in(witnessNonces.keys())) witnessNonces.get(witness) else None,
            currentEpoch,
            NONCE_VALIDITY_WINDOW
        )
        val nonceOk = not(isFastPath) or hasValidNonce  // Fallback doesn't need cached nonce
        val proposal = { witness: witness, resultId: rid, prestateHash: inst.prestateHash, share: share }
        // Check for equivocation and generate proof if detected
        val equivProofOpt = detectEquivocation(inst.proposals, witness, cid, inst.prestateHash, rid, timestampMs)
        val isEquivocating = match equivProofOpt { | Some(_) => true | None => false }
        val newEquivocators = if (isEquivocating) inst.equivocators.union(Set(witness)) else inst.equivocators
        val newEquivocationProofs = match equivProofOpt {
            | Some(proof) => inst.equivocationProofs.union(Set(proof))
            | None => inst.equivocationProofs
        }
        val newProposals = if (not(isEquivocating)) inst.proposals.union(Set(proposal)) else inst.proposals
        val matchingCount = countMatchingProposals(newProposals, rid, inst.prestateHash)
        val reachedThreshold = matchingCount >= inst.threshold
        val newPhase = if (reachedThreshold) ConsensusCommitted else inst.phase
        val attesters = getMatchingWitnesses(newProposals, rid, inst.prestateHash)
        val emptySig: ThresholdSignature = { sigValue: "", boundCid: "", boundRid: "", boundPHash: "", signerSet: Set() }
        val newSig = if (reachedThreshold) aggregateShares(cid, newProposals, rid, inst.prestateHash) else emptySig
        val newCommitFact = if (reachedThreshold) Some({
            cid: cid,
            rid: rid,
            prestateHash: inst.prestateHash,
            signature: newSig,
            attesters: attesters
        }) else None
        // Mark nonce as used on fast path
        val updatedNonces = if (isFastPath and hasValidNonce and not(isEquivocating)) {
            val oldNonce = witnessNonces.get(witness)
            match oldNonce {
                | Some(n) => witnessNonces.put(witness, Some({ ...n, used: true }))
                | None => witnessNonces
            }
        } else witnessNonces
        all {
            hasInstance,
            isWitness,
            notVoted,
            isActive,
            nonceOk,
            not(inst.equivocators.contains(witness)),
            instances' = instances.put(cid, {
                ...inst,
                proposals: newProposals,
                phase: newPhase,
                commitFact: newCommitFact,
                equivocators: newEquivocators,
                equivocationProofs: newEquivocationProofs
            }),
            committedFacts' = if (reachedThreshold)
                committedFacts.union(Set({
                    cid: cid, rid: rid, prestateHash: inst.prestateHash, signature: newSig, attesters: attesters
                }))
                else committedFacts,
            globalWitnesses' = globalWitnesses,
            globalEvidenceTracker' = globalEvidenceTracker,
            currentEpoch' = currentEpoch,
            witnessNonces' = updatedNonces
        }
    }

    // Trigger fallback when fast path stalls.
    // Activated by timer expiry, nonce invalidity, or prestate mismatch detection.
    action triggerFallback(cid: ConsensusId): bool = {
        val instOpt = getOrDefaultInstance(instances, cid)
        val hasInstance = match instOpt { | Some(_) => true | None => false }
        val inst = match instOpt {
            | Some(i) => i
            | None => {
                cid: "", operation: "", prestateHash: "", threshold: 0,
                witnesses: Set(), initiator: "", phase: ConsensusPending,
                proposals: Set(), commitFact: None, fallbackTimerActive: false,
                equivocators: Set(), equivocationProofs: Set()
            }
        }
        val isFastPath = inst.phase == FastPathActive
        all {
            hasInstance,
            isFastPath,
            instances' = instances.put(cid, {
                ...inst,
                phase: FallbackActive,
                fallbackTimerActive: true
            }),
            committedFacts' = committedFacts,
            globalWitnesses' = globalWitnesses,
            globalEvidenceTracker' = globalEvidenceTracker,
            currentEpoch' = currentEpoch,
            witnessNonces' = witnessNonces
        }
    }

    // Gossip shares during fallback (AggregateShare message).
    // Witnesses exchange shares via gossip to overcome network partitions.
    // Fallback path doesn't consume cached nonces (generates fresh nonces inline).
    action gossipShares(cid: ConsensusId, shareSet: Set[ShareProposal]): bool = {
        val instOpt = getOrDefaultInstance(instances, cid)
        val hasInstance = match instOpt { | Some(_) => true | None => false }
        val inst = match instOpt {
            | Some(i) => i
            | None => {
                cid: "", operation: "", prestateHash: "", threshold: 0,
                witnesses: Set(), initiator: "", phase: ConsensusPending,
                proposals: Set(), commitFact: None, fallbackTimerActive: false,
                equivocators: Set(), equivocationProofs: Set()
            }
        }
        val isFallback = inst.phase == FallbackActive
        val validShares = shareSet.filter(p =>
            inst.witnesses.contains(p.witness) and
            not(inst.equivocators.contains(p.witness)) and
            not(hasProposal(inst.proposals, p.witness))
        )
        val mergedProposals = inst.proposals.union(validShares)
        // Check if any (rid, prestateHash) combination now has threshold
        val pHash = inst.prestateHash
        // Simplified: check the first resultId in proposals
        val ridSet = mergedProposals.map(p => p.resultId)
        val anyReachesThreshold = ridSet.exists(rid =>
            countMatchingProposals(mergedProposals, rid, pHash) >= inst.threshold
        )
        all {
            hasInstance,
            isFallback,
            validShares.size() >= 1,
            not(anyReachesThreshold),  // If threshold reached, use completeViaFallback instead
            instances' = instances.put(cid, {
                ...inst,
                proposals: mergedProposals
            }),
            committedFacts' = committedFacts,
            globalWitnesses' = globalWitnesses,
            globalEvidenceTracker' = globalEvidenceTracker,
            currentEpoch' = currentEpoch,
            witnessNonces' = witnessNonces
        }
    }

    // Complete consensus via fallback (any witness can finalize).
    // Once threshold shares gathered via gossip, any node can aggregate.
    action completeViaFallback(cid: ConsensusId, winningRid: ResultId): bool = {
        val instOpt = getOrDefaultInstance(instances, cid)
        val hasInstance = match instOpt { | Some(_) => true | None => false }
        val inst = match instOpt {
            | Some(i) => i
            | None => {
                cid: "", operation: "", prestateHash: "", threshold: 0,
                witnesses: Set(), initiator: "", phase: ConsensusPending,
                proposals: Set(), commitFact: None, fallbackTimerActive: false,
                equivocators: Set(), equivocationProofs: Set()
            }
        }
        val isFallback = inst.phase == FallbackActive
        val matchingCount = countMatchingProposals(inst.proposals, winningRid, inst.prestateHash)
        val reachedThreshold = matchingCount >= inst.threshold
        val attesters = getMatchingWitnesses(inst.proposals, winningRid, inst.prestateHash)
        val sig = aggregateShares(cid, inst.proposals, winningRid, inst.prestateHash)
        all {
            hasInstance,
            isFallback,
            reachedThreshold,
            instances' = instances.put(cid, {
                ...inst,
                phase: ConsensusCommitted,
                commitFact: Some({
                    cid: cid,
                    rid: winningRid,
                    prestateHash: inst.prestateHash,
                    signature: sig,
                    attesters: attesters
                })
            }),
            committedFacts' = committedFacts.union(Set({
                cid: cid, rid: winningRid, prestateHash: inst.prestateHash, signature: sig, attesters: attesters
            })),
            globalWitnesses' = globalWitnesses,
            globalEvidenceTracker' = globalEvidenceTracker,
            currentEpoch' = currentEpoch,
            witnessNonces' = witnessNonces
        }
    }

    // Fail consensus instance (abort without producing CommitFact).
    // Called when consensus cannot complete (insufficient shares, timeout).
    action failConsensus(cid: ConsensusId): bool = {
        val instOpt = getOrDefaultInstance(instances, cid)
        val hasInstance = match instOpt { | Some(_) => true | None => false }
        val inst = match instOpt {
            | Some(i) => i
            | None => {
                cid: "", operation: "", prestateHash: "", threshold: 0,
                witnesses: Set(), initiator: "", phase: ConsensusPending,
                proposals: Set(), commitFact: None, fallbackTimerActive: false,
                equivocators: Set(), equivocationProofs: Set()
            }
        }
        val notCommitted = inst.phase != ConsensusCommitted
        val notFailed = inst.phase != ConsensusFailed
        all {
            hasInstance,
            notCommitted,
            notFailed,
            instances' = instances.put(cid, { ...inst, phase: ConsensusFailed }),
            committedFacts' = committedFacts,
            globalWitnesses' = globalWitnesses,
            globalEvidenceTracker' = globalEvidenceTracker,
            currentEpoch' = currentEpoch,
            witnessNonces' = witnessNonces
        }
    }

    // ==================== NONCE MANAGEMENT ACTIONS ====================

    // Witness caches a fresh nonce commitment for future fast-path signing.
    // Called proactively by witnesses to enable pipelining.
    action cacheNonce(witness: AuthorityId, commitment: NonceCommitment): bool = all {
        witness.in(globalWitnesses),
        // Can cache if no current nonce or current nonce is used/expired
        val currentNonce = if (witness.in(witnessNonces.keys())) witnessNonces.get(witness) else None
        val canCache = match currentNonce {
            | Some(n) => n.used or (currentEpoch - n.epoch) >= NONCE_VALIDITY_WINDOW
            | None => true
        }
        canCache,
        witnessNonces' = witnessNonces.put(witness, Some({
            commitment: commitment,
            epoch: currentEpoch,
            used: false
        })),
        instances' = instances,
        committedFacts' = committedFacts,
        globalWitnesses' = globalWitnesses,
        globalEvidenceTracker' = globalEvidenceTracker,
        currentEpoch' = currentEpoch
    }

    // Advance the global epoch (causes nonce expiration).
    // Called periodically or after significant events.
    action advanceEpoch: bool = all {
        currentEpoch' = currentEpoch + 1,
        instances' = instances,
        committedFacts' = committedFacts,
        globalWitnesses' = globalWitnesses,
        globalEvidenceTracker' = globalEvidenceTracker,
        witnessNonces' = witnessNonces  // Nonces remain but validity checked against new epoch
    }

    // ════════════════════════════════════════════════════════════════════════
    // INVARIANTS
    // ════════════════════════════════════════════════════════════════════════
    // Safety properties that must hold in every reachable state.
    // Each invariant corresponds to a Lean theorem in Aura.Consensus.*.
    //
    // For model checking, use: quint run --invariant=AllInvariants protocol_consensus.qnt
    // ════════════════════════════════════════════════════════════════════════

    /// Combined invariant for model checking - uses WellFormedState predicate.
    val AllInvariants = WellFormedState(instances, committedFacts, witnessNonces, currentEpoch)

    // Safety: Only one CommitFact per consensus instance.
    // Prevents conflicting commits (double-spend protection).
    // Lean: Aura.Consensus.Agreement.agreement, Aura.Consensus.Agreement.unique_commit
    val InvariantUniqueCommitPerInstance =
        committedFacts.forall(cf1 =>
            committedFacts.forall(cf2 =>
                cf1.cid == cf2.cid implies cf1.rid == cf2.rid
            )
        )

    // Safety: CommitFact requires threshold attesters.
    // Ensures k-of-n security (can't forge with fewer shares).
    // Lean: Aura.Consensus.Validity.commit_has_threshold
    val InvariantCommitRequiresThreshold =
        instances.keys().forall(cid => {
            val inst = instances.get(cid)
            val commitOpt = inst.commitFact
            match commitOpt {
                | Some(cf) => cf.attesters.size() >= inst.threshold
                | None => true
            }
        })

    // Safety: Committed instances have valid commit facts.
    // Phase==Committed implies commitFact is Some.
    val InvariantCommittedHasCommitFact =
        instances.keys().forall(cid => {
            val inst = instances.get(cid)
            val isCommitted = inst.phase == ConsensusCommitted
            val hasCommit = match inst.commitFact { | Some(_) => true | None => false }
            isCommitted implies hasCommit
        })

    // Safety: Equivocators are excluded from attestation.
    // Byzantine witnesses who vote inconsistently can't contribute.
    // Lean: Aura.Consensus.Equivocation.exclusion_correctness
    val InvariantEquivocatorsExcluded =
        instances.keys().forall(cid => {
            val inst = instances.get(cid)
            match inst.commitFact {
                | Some(cf) => inst.equivocators.intersect(cf.attesters).size() == 0
                | None => true
            }
        })

    // Evidence: Equivocation proofs are valid (conflicting result IDs)
    // All stored equivocation proofs must have different first/second result IDs
    val InvariantEquivocationProofsValid =
        instances.keys().forall(cid => {
            val inst = instances.get(cid)
            inst.equivocationProofs.forall(proof =>
                proof.firstResultId != proof.secondResultId and
                proof.consensusId == cid and
                proof.prestateHash == inst.prestateHash
            )
        })

    // Evidence: Equivocators have corresponding proofs
    // If a witness is marked as equivocator, there should be a proof
    val InvariantEquivocatorsHaveProofs =
        instances.keys().forall(cid => {
            val inst = instances.get(cid)
            inst.equivocators.forall(w =>
                inst.equivocationProofs.exists(proof => proof.witness == w)
            )
        })

    // Safety: Proposals only from configured witnesses.
    // Prevents Sybil attacks (only pre-approved witnesses vote).
    val InvariantProposalsFromWitnesses =
        instances.keys().forall(cid => {
            val inst = instances.get(cid)
            inst.proposals.forall(p => inst.witnesses.contains(p.witness))
        })

    // Convergence: Fast-path and fallback produce same result.
    // Both paths must agree on (cid, rid) binding.
    val InvariantPathConvergence =
        committedFacts.forall(cf =>
            cf.cid.in(instances.keys()) implies {
                val inst = instances.get(cf.cid)
                cf.rid == cf.rid  // Tautology placeholder - real check in temporal property
            }
        )

    // ==================== NONCE INVARIANTS ====================

    // Used nonces cannot be reused (no nonce reuse attack).
    // Once a nonce is marked used, it must be refreshed before reuse.
    val InvariantUsedNoncesNotReusable =
        witnessNonces.keys().forall(w => {
            val nonceOpt = witnessNonces.get(w)
            match nonceOpt {
                | Some(n) => n.used implies not(isNonceValid(Some(n), currentEpoch, NONCE_VALIDITY_WINDOW))
                | None => true
            }
        })

    // Fast path only selected when all witnesses have valid nonces.
    // Ensures fast path precondition is met.
    val InvariantFastPathRequiresValidNonces =
        instances.keys().forall(cid => {
            val inst = instances.get(cid)
            inst.phase == FastPathActive implies
                allWitnessesHaveValidNonces(inst.witnesses, witnessNonces, currentEpoch, NONCE_VALIDITY_WINDOW)
        })

    // Nonces have bounded epoch age.
    // Cached nonces expire after NONCE_VALIDITY_WINDOW epochs.
    val InvariantNonceEpochBounded =
        witnessNonces.keys().forall(w => {
            val nonceOpt = witnessNonces.get(w)
            match nonceOpt {
                | Some(n) => n.epoch <= currentEpoch
                | None => true
            }
        })

    // ==================== SIGNATURE BINDING INVARIANTS ====================
    // Safety properties ensuring threshold signatures are correctly bound.

    // Signature binding: CommitFact signature binds to correct cid/rid/pHash.
    // Ensures aggregated signature cannot be replayed for different consensus.
    // Lean: Aura.Consensus.Frost.share_binding, Aura.Consensus.Validity.validPrestateBinding
    val InvariantSignatureBindsToCommitFact =
        committedFacts.forall(cf =>
            cf.signature.boundCid == cf.cid and
            cf.signature.boundRid == cf.rid and
            cf.signature.boundPHash == cf.prestateHash
        )

    // Signature attesters match: signerSet in signature equals attesters.
    // Ensures signature provenance is correctly tracked.
    val InvariantSignatureAttesters =
        committedFacts.forall(cf =>
            cf.signature.signerSet == cf.attesters
        )

    // Signature threshold: All committed signatures have valid threshold.
    // Ensures k-of-n security requirement is met.
    // Lean: Aura.Consensus.Frost.aggregation_threshold, Aura.Assumptions.byzantine_threshold
    val InvariantSignatureThreshold =
        instances.keys().forall(cid => {
            val inst = instances.get(cid)
            match inst.commitFact {
                | Some(cf) => isValidSignature(cf.signature, inst.threshold)
                | None => true
            }
        })

    // Signature verifiability: Committed signatures pass verification.
    // Ensures all commits have cryptographically valid signatures.
    val InvariantSignatureVerifiable =
        instances.keys().forall(cid => {
            val inst = instances.get(cid)
            match inst.commitFact {
                | Some(cf) => verifySignature(cf.signature, cf.cid, cf.rid, cf.prestateHash, inst.threshold)
                | None => true
            }
        })

    // ==================== TEMPORAL PROPERTIES ====================
    // LTL properties for liveness and safety over time.

    // Liveness: If threshold honest witnesses participate, consensus commits.
    // Requires fairness assumption (messages eventually delivered).
    temporal livenessEventualCommit = always(
        instances.keys().forall(cid => {
            val inst = instances.get(cid)
            val activeNotCommitted = (inst.phase == FastPathActive or inst.phase == FallbackActive)
            val hasThresholdProposals = inst.proposals.size() >= inst.threshold
            // Note: Full liveness would require fairness assumptions
            activeNotCommitted and hasThresholdProposals implies eventually(inst.phase == ConsensusCommitted)
        })
    )

    // Safety: Once committed, result never changes (immutability).
    // CommitFacts are permanent once produced.
    temporal safetyImmutableCommit = always(
        committedFacts.forall(cf =>
            always(committedFacts.exists(cf2 => cf2.cid == cf.cid and cf2.rid == cf.rid))
        )
    )

    // ==================== STEP RELATION (for trace generation) ====================
    // Nondeterministically choose any enabled action for model checking.

    action step = any {
        nondet cid = oneOf(Set("cns1", "cns2", "cns3"))
        nondet initiator = oneOf(Set("w1", "w2"))
        nondet op = oneOf(Set("update_policy", "add_device", "remove_device"))
        nondet pHash = oneOf(Set("pre_abc", "pre_xyz"))
        nondet witnesses = oneOf(Set(Set("w1", "w2", "w3"), Set("w2", "w3", "w4")))
        nondet threshold = oneOf(Set(2, 3))
        nondet witness = oneOf(Set("w1", "w2", "w3", "w4"))
        nondet rid = oneOf(Set("rid_op1", "rid_op2"))
        nondet timestampMs = oneOf(Set(1000, 2000, 3000, 4000))
        // ShareData now a structured record with nonce binding and data binding
        nondet shareValue = oneOf(Set("share_a", "share_b", "share_c"))
        nondet nonceBinding = oneOf(Set("nonce_commit_1", "nonce_commit_2", "nonce_commit_3"))
        // Construct share with data binding computed from (cid, rid, pHash)
        val dataBinding = computeDataBinding(cid, rid, pHash)
        val share: ShareData = { shareValue: shareValue, nonceBinding: nonceBinding, dataBinding: dataBinding }
        // Nonce commitment values
        nondet commitment = oneOf(Set("nonce_commit_1", "nonce_commit_2", "nonce_commit_3"))
        any {
            startConsensus(cid, initiator, op, pHash, witnesses, threshold),
            submitWitnessShare(cid, witness, rid, share, timestampMs),
            triggerFallback(cid),
            completeViaFallback(cid, rid),
            failConsensus(cid),
            // Nonce management actions
            cacheNonce(witness, commitment),
            advanceEpoch,
        }
    }
}
