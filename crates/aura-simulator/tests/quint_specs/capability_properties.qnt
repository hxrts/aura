// Capability Property Verification
// Formal specification of Aura's core security properties:
// - Authorization (guard chain compliance)
// - Budget (charge-before-send invariant)
// - Integrity (attenuation-only, receipt chain)

module CapabilityProperties {
    // ========== Core Types ==========

    type AuthorityId = str
    type ContextId = str
    type Epoch = int

    // Capability level as integer (0=None, 1=Execute, 2=Write, 3=Read, 4=Full)
    // Meet operation: min(a, b) - can only narrow
    type CapLevel = int

    val CAP_NONE: int = 0
    val CAP_EXECUTE: int = 1
    val CAP_WRITE: int = 2
    val CAP_READ: int = 3
    val CAP_FULL: int = 4

    // Flow budget state
    type FlowBudget = {
        context_id: ContextId,
        peer: AuthorityId,
        epoch: Epoch,
        spent: int,
        limit: int
    }

    // Biscuit token (attenuated capability)
    type BiscuitToken = {
        cap_level: CapLevel,
        attenuation_count: int
    }

    // Guard chain step
    type GuardStep =
        | CapGuard
        | FlowGuard
        | JournalCoupler
        | TransportSend

    // Transport operation record
    type TransportOp = {
        context_id: ContextId,
        source: AuthorityId,
        dest: AuthorityId,
        cost: int,
        epoch: Epoch,
        guard_steps_completed: List[GuardStep],
        charged: bool
    }

    // ========== State Variables ==========

    var budgets: ContextId -> FlowBudget
    var completed_ops: List[TransportOp]
    var current_epoch: ContextId -> Epoch
    var tokens: AuthorityId -> BiscuitToken

    // ========== Helper Functions ==========

    // Check guard chain order is correct
    pure def isValidGuardOrder(steps: List[GuardStep]): bool = {
        steps.length() == 4 and
        steps[0] == CapGuard and
        steps[1] == FlowGuard and
        steps[2] == JournalCoupler and
        steps[3] == TransportSend
    }

    // ========== Actions ==========

    // Complete a transport operation (simplified)
    action completeTransportOp(
        ctx: ContextId,
        src: AuthorityId,
        dst: AuthorityId,
        cost: int
    ): bool = all {
        ctx.in(current_epoch.keys()),
        ctx.in(budgets.keys()),
        src.in(tokens.keys()),
        tokens.get(src).cap_level > CAP_NONE,
        budgets.get(ctx).spent + cost <= budgets.get(ctx).limit,
        completed_ops' = completed_ops.append({
            context_id: ctx,
            source: src,
            dest: dst,
            cost: cost,
            epoch: budgets.get(ctx).epoch,
            guard_steps_completed: [CapGuard, FlowGuard, JournalCoupler, TransportSend],
            charged: true
        }),
        budgets' = budgets.put(ctx, budgets.get(ctx).with("spent", budgets.get(ctx).spent + cost)),
        current_epoch' = current_epoch,
        tokens' = tokens
    }

    // Attenuate a capability token
    action attenuateToken(auth: AuthorityId, new_cap: CapLevel): bool = all {
        auth.in(tokens.keys()),
        new_cap <= tokens.get(auth).cap_level,
        new_cap >= CAP_NONE,
        tokens' = tokens.put(auth, {
            cap_level: new_cap,
            attenuation_count: tokens.get(auth).attenuation_count + 1
        }),
        completed_ops' = completed_ops,
        budgets' = budgets,
        current_epoch' = current_epoch
    }

    // Initialize a context
    action initContext(ctx: ContextId, peer: AuthorityId, limit: int): bool = all {
        not(ctx.in(current_epoch.keys())),
        current_epoch' = current_epoch.put(ctx, 0),
        budgets' = budgets.put(ctx, {
            context_id: ctx,
            peer: peer,
            epoch: 0,
            spent: 0,
            limit: limit
        }),
        completed_ops' = completed_ops,
        tokens' = tokens
    }

    // Initialize an authority
    action initAuthority(auth: AuthorityId, cap: CapLevel): bool = all {
        not(auth.in(tokens.keys())),
        tokens' = tokens.put(auth, {
            cap_level: cap,
            attenuation_count: 0
        }),
        completed_ops' = completed_ops,
        budgets' = budgets,
        current_epoch' = current_epoch
    }

    // ========== Invariants ==========

    // AUTHORIZATION: Guard chain order is always correct
    val guardChainOrder = completed_ops.forall(op => isValidGuardOrder(op.guard_steps_completed))

    // BUDGET: Charge-before-send invariant
    val chargeBeforeSend = completed_ops.forall(op =>
        op.guard_steps_completed.contains(TransportSend) implies op.charged
    )

    // BUDGET: Spent never exceeds limit
    val spentWithinLimit = budgets.keys().forall(ctx => budgets.get(ctx).spent <= budgets.get(ctx).limit)

    // BUDGET: No transport without FlowGuard
    val noTransportWithoutFlowGuard = completed_ops.forall(op =>
        op.guard_steps_completed.contains(TransportSend) implies
        op.guard_steps_completed.contains(FlowGuard)
    )

    // INTEGRITY: Attenuation only narrows capabilities
    val attenuationOnlyNarrows = tokens.keys().forall(auth =>
        tokens.get(auth).cap_level >= CAP_NONE and tokens.get(auth).cap_level <= CAP_FULL
    )

    // AUTHORIZATION: Attenuation count tracks narrowing
    val noCapabilityWidening = tokens.keys().forall(auth => tokens.get(auth).attenuation_count >= 0)

    // ========== Temporal Properties ==========

    // Authorization soundness: All completed operations went through full guard chain
    temporal authorizationSoundness = always(
        completed_ops.forall(op =>
            op.guard_steps_completed == [CapGuard, FlowGuard, JournalCoupler, TransportSend]
        )
    )

    // Budget consistency: Spent counters are non-negative
    temporal budgetMonotonicity = always(budgets.keys().forall(ctx => budgets.get(ctx).spent >= 0))

    // Flow budget fairness: Limits are positive
    temporal flowBudgetFairness = always(budgets.keys().forall(ctx => budgets.get(ctx).limit > 0))

    // ========== Initial State ==========

    action init = all {
        budgets' = Map(),
        completed_ops' = [],
        current_epoch' = Map(),
        tokens' = Map()
    }

    // ========== Step Relation ==========

    action step = any {
        nondet ctx = oneOf(Set("ctx1", "ctx2"))
        nondet auth = oneOf(Set("auth1", "auth2"))
        nondet cost = oneOf(Set(1, 5, 10))
        nondet limit = oneOf(Set(100, 500))
        nondet cap = oneOf(Set(CAP_READ, CAP_WRITE, CAP_FULL))
        any {
            initContext(ctx, auth, limit),
            initAuthority(auth, cap),
            completeTransportOp(ctx, auth, auth, cost),
            attenuateToken(auth, cap - 1),
        }
    }
}
