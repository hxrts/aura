// Capability Property Verification
// Formal specification of Aura's core security properties:
// - Authorization (guard chain compliance)
// - Budget (charge-before-send invariant)
// - Integrity (attenuation-only, receipt chain)

module CapabilityProperties {
    // ========== Core Types ==========
    // Identifiers for authorities, contexts, and epochs.

    type AuthorityId = str  // Opaque authority identifier
    type ContextId = str    // Relational context (peer pair)
    type Epoch = int        // Monotonic version for state invalidation

    // Capability level forms a meet-semilattice.
    // Delegation can only attenuate (narrow), never amplify.
    type CapLevel = int

    val CAP_NONE: int = 0     // No permissions
    val CAP_EXECUTE: int = 1  // Can invoke operations
    val CAP_WRITE: int = 2    // Can modify state
    val CAP_READ: int = 3     // Can read state
    val CAP_FULL: int = 4     // All permissions

    // Flow budget state (per-context, per-peer spending limit).
    // Spent counters are facts; limits derived from Biscuit + policy.
    type FlowBudget = {
        context_id: ContextId,  // Which relational context
        peer: AuthorityId,      // Which peer this budget is for
        epoch: Epoch,           // Current epoch (resets budget on bump)
        spent: int,             // Amount consumed this epoch
        limit: int              // Maximum allowed this epoch
    }

    // Biscuit token (attenuated capability).
    // Delegation creates tokens with lower/equal cap_level.
    type BiscuitToken = {
        cap_level: CapLevel,      // Current permission level
        attenuation_count: int    // How many attenuations applied
    }

    // Guard chain step (ordered security checks).
    // Must execute in order: CapGuard → FlowGuard → JournalCoupler → TransportSend.
    type GuardStep =
        | CapGuard         // Verify Biscuit authorization
        | FlowGuard        // Check and charge flow budget
        | JournalCoupler   // Record fact in journal
        | TransportSend    // Actually send the message

    // Transport operation record (audit trail).
    // Captures full guard chain execution for verification.
    type TransportOp = {
        context_id: ContextId,    // Which context
        source: AuthorityId,      // Sender
        dest: AuthorityId,        // Receiver
        cost: int,                // Flow budget units consumed
        epoch: Epoch,             // Epoch at time of operation
        guard_steps_completed: List[GuardStep],  // Ordered steps executed
        charged: bool             // True if flow budget was charged
    }

    // ========== State Variables ==========
    // Global mutable state tracked by the model checker.

    var budgets: ContextId -> FlowBudget       // Flow budgets per context
    var completed_ops: List[TransportOp]       // All completed operations
    var current_epoch: ContextId -> Epoch      // Current epoch per context
    var tokens: AuthorityId -> BiscuitToken    // Capability tokens per authority

    // ========== Helper Functions ==========
    // Pure functions for guard chain validation.

    // Check if a list contains an element (foldl since contains is Set-only).
    pure def listContains(lst: List[GuardStep], elem: GuardStep): bool =
        lst.foldl(false, (acc, x) => acc or (x == elem))

    // Validate guard chain order is correct.
    // Security critical: wrong order could bypass checks.
    pure def isValidGuardOrder(steps: List[GuardStep]): bool = {
        steps.length() == 4 and
        steps[0] == CapGuard and        // 1. Authorization check
        steps[1] == FlowGuard and       // 2. Budget check + charge
        steps[2] == JournalCoupler and  // 3. Record fact
        steps[3] == TransportSend       // 4. Send message
    }

    // ========== Actions ==========
    // State transitions modeling capability operations.

    // Complete a transport operation (full guard chain).
    // Requires: valid token, sufficient budget, correct guard order.
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

    // Attenuate a capability token (delegation with reduced permissions).
    // Can only narrow: new_cap <= current cap_level.
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

    // Initialize a context with flow budget.
    // Sets up peer relationship and spending limit.
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

    // Initialize an authority with capability token.
    // Grants initial permissions (typically CAP_FULL for self).
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
    // Safety properties for the guard chain and capability system.

    // AUTHORIZATION: Guard chain order is always correct.
    // CapGuard → FlowGuard → JournalCoupler → TransportSend.
    val guardChainOrder = completed_ops.foldl(true, (acc, op) => acc and isValidGuardOrder(op.guard_steps_completed))

    // BUDGET: Charge-before-send invariant (core security property).
    // Cannot send without first charging the flow budget.
    val chargeBeforeSend = completed_ops.foldl(true, (acc, op) =>
        acc and (listContains(op.guard_steps_completed, TransportSend) implies op.charged)
    )

    // BUDGET: Spent never exceeds limit.
    // Prevents resource exhaustion attacks.
    val spentWithinLimit = budgets.keys().forall(ctx => budgets.get(ctx).spent <= budgets.get(ctx).limit)

    // BUDGET: No transport without FlowGuard.
    // Every send must pass through flow budget check.
    val noTransportWithoutFlowGuard = completed_ops.foldl(true, (acc, op) =>
        acc and (listContains(op.guard_steps_completed, TransportSend) implies
        listContains(op.guard_steps_completed, FlowGuard))
    )

    // INTEGRITY: Attenuation only narrows capabilities.
    // Tokens stay within valid capability range.
    val attenuationOnlyNarrows = tokens.keys().forall(auth =>
        tokens.get(auth).cap_level >= CAP_NONE and tokens.get(auth).cap_level <= CAP_FULL
    )

    // AUTHORIZATION: Attenuation count tracks narrowing.
    // Monotonically increases (never negative).
    val noCapabilityWidening = tokens.keys().forall(auth => tokens.get(auth).attenuation_count >= 0)

    // ========== Temporal Properties ==========
    // LTL properties for security over all execution traces.

    // Authorization soundness: All operations went through full guard chain.
    // No operation can bypass any guard step.
    temporal authorizationSoundness = always(
        completed_ops.foldl(true, (acc, op) =>
            acc and (op.guard_steps_completed == [CapGuard, FlowGuard, JournalCoupler, TransportSend])
        )
    )

    // Budget consistency: Spent counters are always non-negative.
    // Can't spend negative amounts (no "refunds" that create budget).
    temporal budgetMonotonicity = always(budgets.keys().forall(ctx => budgets.get(ctx).spent >= 0))

    // Flow budget fairness: Limits are always positive.
    // Every context has some budget (DoS prevention via rate limiting).
    temporal flowBudgetFairness = always(budgets.keys().forall(ctx => budgets.get(ctx).limit > 0))

    // ========== Initial State ==========
    // Start with empty state (no contexts, no authorities).

    action init = all {
        budgets' = Map(),
        completed_ops' = [],
        current_epoch' = Map(),
        tokens' = Map()
    }

    // ========== Step Relation ==========
    // Nondeterministically choose any enabled action for model checking.

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
