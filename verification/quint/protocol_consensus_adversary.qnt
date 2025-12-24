// Protocol Consensus Adversary - Byzantine Behavior Models
//
// This module extends protocol_consensus.qnt with Byzantine adversary actions.
// Models malicious witness behaviors for security verification.
//
// Byzantine behaviors modeled:
// - Equivocation: Signing conflicting values for same consensus instance
// - Selective dropping: Not responding to some consensus requests
// - Premature nonce reveal: Revealing nonce secrets before signing
// - Timing attacks: Delayed share submission
// - Nonce reuse: Attempting to reuse consumed nonces
//
// Security assumption: At most f < k Byzantine witnesses (f < threshold).
//
// == Lean Correspondence ==
// Module: Aura.Consensus.Equivocation (verification/lean/Aura/Consensus/Equivocation.lean)
// - Equivocation detection: Aura.Consensus.Equivocation.hasEquivocated
// - Detection soundness: Aura.Consensus.Equivocation.detection_soundness
// - Detection completeness: Aura.Consensus.Equivocation.detection_completeness
// - Exclusion correctness: Aura.Consensus.Equivocation.exclusion_correctness
// - Honest protection: Aura.Consensus.Equivocation.honest_never_detected
// Axioms: Aura.Assumptions (byzantine_threshold)
//
// == Rust Correspondence ==
// - Types: crates/aura-protocol/src/consensus/types.rs::ConflictFact
// - Detection: crates/aura-protocol/src/consensus/types.rs::detect_conflict
// - Witness logic: crates/aura-protocol/src/consensus/witness.rs
//
// See: docs/004_distributed_systems_contract.md (§6.2 Byzantine Witness Model)

module protocol_consensus_adversary {
    import protocol_consensus as core from "protocol_consensus"

    // Local Option type for match patterns (Quint doesn't allow qualified match patterns)
    type Option[a] = Some(a) | None

    // ════════════════════════════════════════════════════════════════════════
    // ADVERSARY STATE
    // ════════════════════════════════════════════════════════════════════════

    // Set of Byzantine (adversarial) witnesses - at most f < threshold
    var byzantineWitnesses: Set[core::AuthorityId]

    // Track compromised nonces (where secrets were revealed prematurely)
    var compromisedNonces: Set[core::AuthorityId]

    // Track delayed actions for timing attacks
    type DelayedAction = {
        cid: core::ConsensusId,
        witness: core::AuthorityId,
        rid: core::ResultId,
        share: core::ShareData,
        delayRemaining: int
    }
    var delayedActions: Set[DelayedAction]

    // ════════════════════════════════════════════════════════════════════════
    // EXPOSE: SEMANTIC INTERFACE

    // These predicates define Byzantine behavior MEANING.
    // - Rust conformance tests verify these properties
    // - Changing these requires updating Rust implementation
    // - Lean proofs reference these definitions
    //
    // Exposed predicates:
    //   - isByzantine: whether a witness is under adversary control
    //   - canEquivocate: whether a Byzantine witness can submit conflicting shares
    //   - byzantineThresholdOk: f < k bound holds
    //   - WellFormedAdversaryState: combined adversary invariant
    // ════════════════════════════════════════════════════════════════════════

    /// Check if a witness is Byzantine (under adversary control).
    /// Lean: Aura.Assumptions.byzantine_threshold models adversary capability
    /// Rust: consensus::adversary::is_byzantine
    pure def isByzantine(witness: core::AuthorityId, byzantines: Set[core::AuthorityId]): bool =
        byzantines.contains(witness)

    /// Check if a Byzantine witness can equivocate for a consensus instance.
    /// Lean: Aura.Consensus.Equivocation.hasEquivocated precondition
    pure def canEquivocate(
        witness: core::AuthorityId,
        inst: core::ConsensusInstance,
        byzantines: Set[core::AuthorityId]
    ): bool = all {
        isByzantine(witness, byzantines),
        inst.witnesses.contains(witness),
        inst.phase == core::FastPathActive or inst.phase == core::FallbackActive,
        not(core::hasProposal(inst.proposals, witness))
    }

    /// Check Byzantine threshold: f < k (strictly less than threshold).
    /// Lean: Aura.Assumptions.byzantine_threshold
    /// This is the fundamental security assumption for threshold protocols.
    pure def byzantineThresholdOk(byzantines: Set[core::AuthorityId], threshold: int): bool =
        byzantines.size() < threshold

    /// Combined adversary state well-formedness.
    /// Ensures adversary model maintains safety assumptions.
    pure def WellFormedAdversaryState(
        insts: core::ConsensusId -> core::ConsensusInstance,
        byzantines: Set[core::AuthorityId],
        compromised: Set[core::AuthorityId]
    ): bool = all {
        // Byzantine threshold maintained for all instances
        insts.keys().forall(cid => {
            val inst = insts.get(cid)
            countByzantine(inst.witnesses, byzantines) < inst.threshold
        }),
        // Compromised nonces subset of Byzantine witnesses
        compromised.subseteq(byzantines)
    }

    // ════════════════════════════════════════════════════════════════════════
    // INTERNAL HELPERS

    // Implementation utilities. Do not reference in Rust conformance tests.
    // ════════════════════════════════════════════════════════════════════════

    // Count Byzantine witnesses in a set
    pure def countByzantine(witnesses: Set[core::AuthorityId], byzantines: Set[core::AuthorityId]): int =
        witnesses.intersect(byzantines).size()

    // ==================== BYZANTINE ACTIONS ====================

    // Initialize adversary state alongside consensus state.
    action initAdversary: bool = all {
        core::init,
        byzantineWitnesses' = Set(),
        compromisedNonces' = Set(),
        delayedActions' = Set()
    }

    // Designate a witness as Byzantine (adversary control).
    // Constrained: total Byzantine < threshold for any active consensus.
    action designateByzantine(witness: core::AuthorityId): bool = all {
        witness.in(core::globalWitnesses),
        not(byzantineWitnesses.contains(witness)),
        // Ensure Byzantine count stays below threshold for all instances
        core::instances.keys().forall(cid => {
            val inst = core::instances.get(cid)
            val newByzCount = countByzantine(inst.witnesses, byzantineWitnesses.union(Set(witness)))
            newByzCount < inst.threshold
        }),
        byzantineWitnesses' = byzantineWitnesses.union(Set(witness)),
        compromisedNonces' = compromisedNonces,
        delayedActions' = delayedActions,
        // Preserve consensus state
        core::instances' = core::instances,
        core::committedFacts' = core::committedFacts,
        core::globalWitnesses' = core::globalWitnesses,
        core::currentEpoch' = core::currentEpoch,
        core::witnessNonces' = core::witnessNonces
    }

    // Byzantine equivocation: Submit conflicting share for same cid.
    // A Byzantine witness signs different resultIds for the same consensus.
    action byzantineEquivocate(
        cid: core::ConsensusId,
        witness: core::AuthorityId,
        rid1: core::ResultId,
        rid2: core::ResultId,
        share1: core::ShareData,
        share2: core::ShareData
    ): bool = {
        val instOpt = core::getOrDefaultInstance(core::instances, cid)
        val hasInstance = match instOpt { | Some(_) => true | None => false }
        val inst = match instOpt {
            | Some(i) => i
            | None => {
                cid: "", operation: "", prestateHash: "", threshold: 0,
                witnesses: Set(), initiator: "", phase: core::ConsensusPending,
                proposals: Set(), commitFact: None, fallbackTimerActive: false,
                equivocators: Set()
            }
        }
        all {
            hasInstance,
            isByzantine(witness, byzantineWitnesses),
            inst.witnesses.contains(witness),
            inst.phase == core::FastPathActive or inst.phase == core::FallbackActive,
            rid1 != rid2,  // Must be conflicting
            not(core::hasProposal(inst.proposals, witness)),
            // Submit first share (will be detected as equivocation when second arrives)
            val proposal1 = { witness: witness, resultId: rid1, prestateHash: inst.prestateHash, share: share1 }
            val proposal2 = { witness: witness, resultId: rid2, prestateHash: inst.prestateHash, share: share2 }
            // Both proposals added, witness marked as equivocator
            core::instances' = core::instances.put(cid, {
                ...inst,
                proposals: inst.proposals.union(Set(proposal1, proposal2)),
                equivocators: inst.equivocators.union(Set(witness))
            }),
            core::committedFacts' = core::committedFacts,
            core::globalWitnesses' = core::globalWitnesses,
            core::currentEpoch' = core::currentEpoch,
            core::witnessNonces' = core::witnessNonces,
            byzantineWitnesses' = byzantineWitnesses,
            compromisedNonces' = compromisedNonces,
            delayedActions' = delayedActions
        }
    }

    // Byzantine selective drop: Byzantine witness ignores consensus request.
    // Models network-level censorship or deliberate non-participation.
    // This is a "non-action" - the witness simply doesn't submit a share.
    // We model it explicitly to track the behavior.
    action byzantineSelectiveDrop(cid: core::ConsensusId, witness: core::AuthorityId): bool = {
        val instOpt = core::getOrDefaultInstance(core::instances, cid)
        val hasInstance = match instOpt { | Some(_) => true | None => false }
        val inst = match instOpt {
            | Some(i) => i
            | None => {
                cid: "", operation: "", prestateHash: "", threshold: 0,
                witnesses: Set(), initiator: "", phase: core::ConsensusPending,
                proposals: Set(), commitFact: None, fallbackTimerActive: false,
                equivocators: Set()
            }
        }
        all {
            hasInstance,
            isByzantine(witness, byzantineWitnesses),
            inst.witnesses.contains(witness),
            inst.phase == core::FastPathActive or inst.phase == core::FallbackActive,
            not(core::hasProposal(inst.proposals, witness)),
            // State unchanged - witness just doesn't participate
            core::instances' = core::instances,
            core::committedFacts' = core::committedFacts,
            core::globalWitnesses' = core::globalWitnesses,
            core::currentEpoch' = core::currentEpoch,
            core::witnessNonces' = core::witnessNonces,
            byzantineWitnesses' = byzantineWitnesses,
            compromisedNonces' = compromisedNonces,
            delayedActions' = delayedActions
        }
    }

    // Byzantine premature nonce reveal: Compromise nonce before signing.
    // If a nonce is revealed prematurely, it cannot be safely used.
    action byzantinePrematureNonceReveal(witness: core::AuthorityId): bool = all {
        isByzantine(witness, byzantineWitnesses),
        witness.in(core::witnessNonces.keys()),
        val nonceOpt = core::witnessNonces.get(witness)
        match nonceOpt {
            | Some(n) => not(n.used)  // Only reveal unused nonces
            | None => false
        },
        // Mark nonce as compromised and used (cannot be used safely now)
        val oldNonce = core::witnessNonces.get(witness)
        val updatedNonces = match oldNonce {
            | Some(n) => core::witnessNonces.put(witness, Some({ ...n, used: true }))
            | None => core::witnessNonces
        }
        core::witnessNonces' = updatedNonces,
        compromisedNonces' = compromisedNonces.union(Set(witness)),
        core::instances' = core::instances,
        core::committedFacts' = core::committedFacts,
        core::globalWitnesses' = core::globalWitnesses,
        core::currentEpoch' = core::currentEpoch,
        byzantineWitnesses' = byzantineWitnesses,
        delayedActions' = delayedActions
    }

    // Byzantine timing attack: Delay share submission.
    // Schedule a share to be submitted after some delay.
    action byzantineDelayShare(
        cid: core::ConsensusId,
        witness: core::AuthorityId,
        rid: core::ResultId,
        share: core::ShareData,
        delay: int
    ): bool = {
        val instOpt = core::getOrDefaultInstance(core::instances, cid)
        val hasInstance = match instOpt { | Some(_) => true | None => false }
        val inst = match instOpt {
            | Some(i) => i
            | None => {
                cid: "", operation: "", prestateHash: "", threshold: 0,
                witnesses: Set(), initiator: "", phase: core::ConsensusPending,
                proposals: Set(), commitFact: None, fallbackTimerActive: false,
                equivocators: Set()
            }
        }
        all {
            hasInstance,
            isByzantine(witness, byzantineWitnesses),
            inst.witnesses.contains(witness),
            inst.phase == core::FastPathActive or inst.phase == core::FallbackActive,
            not(core::hasProposal(inst.proposals, witness)),
            delay > 0,
            val delayedAction = {
                cid: cid,
                witness: witness,
                rid: rid,
                share: share,
                delayRemaining: delay
            }
            delayedActions' = delayedActions.union(Set(delayedAction)),
            core::instances' = core::instances,
            core::committedFacts' = core::committedFacts,
            core::globalWitnesses' = core::globalWitnesses,
            core::currentEpoch' = core::currentEpoch,
            core::witnessNonces' = core::witnessNonces,
            byzantineWitnesses' = byzantineWitnesses,
            compromisedNonces' = compromisedNonces
        }
    }

    // Process delayed actions: Decrement delay counters, execute when ready.
    action tickDelayedActions: bool = {
        // Partition into ready (delay=1) and still waiting
        val readyActions = delayedActions.filter(a => a.delayRemaining == 1)
        val waitingActions = delayedActions.filter(a => a.delayRemaining > 1)
        val decrementedWaiting = waitingActions.map(a => { ...a, delayRemaining: a.delayRemaining - 1 })
        // Ready actions are removed; caller should execute submitWitnessShare for each
        all {
            delayedActions' = decrementedWaiting,
            core::instances' = core::instances,
            core::committedFacts' = core::committedFacts,
            core::globalWitnesses' = core::globalWitnesses,
            core::currentEpoch' = core::currentEpoch,
            core::witnessNonces' = core::witnessNonces,
            byzantineWitnesses' = byzantineWitnesses,
            compromisedNonces' = compromisedNonces
        }
    }

    // Byzantine nonce reuse attempt: Try to use a consumed nonce.
    // This should be blocked by the protocol - models the attack, not success.
    action byzantineNonceReuseAttempt(
        cid: core::ConsensusId,
        witness: core::AuthorityId,
        rid: core::ResultId,
        share: core::ShareData
    ): bool = {
        val instOpt = core::getOrDefaultInstance(core::instances, cid)
        val hasInstance = match instOpt { | Some(_) => true | None => false }
        val inst = match instOpt {
            | Some(i) => i
            | None => {
                cid: "", operation: "", prestateHash: "", threshold: 0,
                witnesses: Set(), initiator: "", phase: core::ConsensusPending,
                proposals: Set(), commitFact: None, fallbackTimerActive: false,
                equivocators: Set()
            }
        }
        val isFastPath = inst.phase == core::FastPathActive
        val nonceOpt = if (witness.in(core::witnessNonces.keys())) core::witnessNonces.get(witness) else None
        val nonceIsUsed = match nonceOpt {
            | Some(n) => n.used
            | None => false
        }
        all {
            hasInstance,
            isByzantine(witness, byzantineWitnesses),
            inst.witnesses.contains(witness),
            isFastPath,
            nonceIsUsed,  // Attempting to reuse a used nonce
            // This should FAIL - nonce reuse is blocked
            // The action is enabled but produces no state change (attack fails)
            false  // Action always disabled - nonce reuse is prevented
        }
    }

    // ==================== ADVERSARY INVARIANTS ====================
    // Safety properties for Byzantine fault tolerance.
    // Lean correspondence: Aura.Consensus.Equivocation, Aura.Assumptions

    // Safety: Byzantine count never exceeds f < k bound.
    // Lean: Aura.Assumptions.byzantine_threshold (threshold > maxByzantine)
    val InvariantByzantineThreshold =
        core::instances.keys().forall(cid => {
            val inst = core::instances.get(cid)
            countByzantine(inst.witnesses, byzantineWitnesses) < inst.threshold
        })

    // Safety: Equivocators are always detected and excluded.
    // Lean: Aura.Consensus.Equivocation.detection_soundness, detection_completeness
    val InvariantEquivocationDetected =
        core::instances.keys().forall(cid => {
            val inst = core::instances.get(cid)
            // Any witness with conflicting proposals is in equivocators set
            val conflictingWitnesses = inst.proposals.filter(p1 =>
                inst.proposals.exists(p2 => p1.witness == p2.witness and p1.resultId != p2.resultId)
            ).map(p => p.witness)
            conflictingWitnesses.subseteq(inst.equivocators)
        })

    // Safety: Compromised nonces cannot be used for fast path.
    val InvariantCompromisedNoncesExcluded =
        core::instances.keys().forall(cid => {
            val inst = core::instances.get(cid)
            inst.phase == core::FastPathActive implies
                inst.witnesses.intersect(compromisedNonces).size() == 0
        })

    // Security: Despite Byzantine behavior, honest majority can still commit.
    // If k - f honest witnesses participate, consensus can complete.
    // Lean: Aura.Consensus.Validity.honest_participation
    val InvariantHonestMajorityCanCommit =
        core::instances.keys().forall(cid => {
            val inst = core::instances.get(cid)
            val honestWitnesses = inst.witnesses.exclude(byzantineWitnesses)
            val honestProposals = inst.proposals.filter(p => honestWitnesses.contains(p.witness))
            // If honest witnesses have threshold proposals, should be able to commit
            honestProposals.size() >= inst.threshold implies
                (inst.phase == core::ConsensusCommitted or inst.phase == core::FastPathActive or inst.phase == core::FallbackActive)
        })

    // ==================== ADVERSARIAL STEP RELATION ====================

    action stepAdversary = any {
        // Normal consensus actions
        nondet cid = oneOf(Set("cns1", "cns2", "cns3"))
        nondet initiator = oneOf(Set("w1", "w2"))
        nondet op = oneOf(Set("update_policy", "add_device", "remove_device"))
        nondet pHash = oneOf(Set("pre_abc", "pre_xyz"))
        nondet witnesses = oneOf(Set(Set("w1", "w2", "w3"), Set("w2", "w3", "w4")))
        nondet threshold = oneOf(Set(2, 3))
        nondet witness = oneOf(Set("w1", "w2", "w3", "w4"))
        nondet rid = oneOf(Set("rid_op1", "rid_op2"))
        nondet rid2 = oneOf(Set("rid_op1", "rid_op2"))
        // ShareData now a structured record with nonce binding and data binding
        nondet shareValue = oneOf(Set("share_a", "share_b", "share_c"))
        nondet shareValue2 = oneOf(Set("share_a", "share_b", "share_c"))
        nondet nonceBinding = oneOf(Set("nonce_commit_1", "nonce_commit_2", "nonce_commit_3"))
        val dataBinding = core::computeDataBinding(cid, rid, pHash)
        val dataBinding2 = core::computeDataBinding(cid, rid2, pHash)
        val share: core::ShareData = { shareValue: shareValue, nonceBinding: nonceBinding, dataBinding: dataBinding }
        val share2: core::ShareData = { shareValue: shareValue2, nonceBinding: nonceBinding, dataBinding: dataBinding2 }
        nondet commitment = oneOf(Set("nonce_commit_1", "nonce_commit_2", "nonce_commit_3"))
        nondet delay = oneOf(Set(1, 2, 3))
        any {
            // Byzantine actions
            designateByzantine(witness),
            byzantineEquivocate(cid, witness, rid, rid2, share, share2),
            byzantineSelectiveDrop(cid, witness),
            byzantinePrematureNonceReveal(witness),
            byzantineDelayShare(cid, witness, rid, share, delay),
            tickDelayedActions,
        }
    }
}
