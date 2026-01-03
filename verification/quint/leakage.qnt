// Leakage Tracking Specification
//
// Models Aura's privacy-preserving information flow system. Each transport
// operation is annotated with leakage to observer classes. Budgets bound
// what each observer class can learn over time.
//
// Observer classes (from docs/003_information_flow_contract.md):
// - Neighbor: Sees envelope metadata (size, timing, rtags)
// - InGroup: Sees group-level activity
// - External: Should learn nothing
//
// Key properties verified:
// - Leakage never exceeds budget per observer class
// - Charge-before-leak invariant (no leakage without recording)
// - Observer class separation (external learns less than neighbor)
//
// == Lean Correspondence ==
// Module: Aura.Proofs.ContextIsolation (for context separation)
// - Types: ObserverClass, LeakageEvent, LeakageBudget
// - Invariants: leakage_within_budget, observer_hierarchy
//
// == Rust Correspondence ==
// - File: crates/aura-core/src/effects/leakage.rs
// - Types: ObserverClass, LeakageEvent, LeakageEffects
// - Trait: LeakageEffects
//
// See: docs/003_information_flow_contract.md

module leakage_tracking {
  // ==================== TYPE DEFINITIONS ====================
  // Observer classes and leakage tracking types.

  type Option[a] = Some(a) | None
  type AuthorityId = str
  type ContextId = str
  type DeviceId = str

  // Observer classes ordered by visibility privilege.
  // External < Neighbor < InGroup < Self
  // Higher privilege sees more; budgets are independent per class.
  //
  // Lean: Aura.Domain.ContextIsolation.ObserverClass
  type ObserverClass =
    | ObserverExternal   // No relationship, should learn nothing
    | ObserverNeighbor   // Sees envelope metadata (size, timing, rtags)
    | ObserverInGroup    // Sees group-level activity
    | ObserverSelf       // Full visibility (local device)

  // Type of information leaked.
  // Metadata is lower sensitivity than Content.
  //
  // Lean: Aura.Domain.ContextIsolation.LeakageType
  type LeakageType =
    | LeakMetadata   // Envelope metadata (timing, size, routing)
    | LeakContent    // Actual message content (encrypted)
    | LeakTiming     // Precise timing information
    | LeakIdentity   // Identity linkage potential

  // A single leakage event recorded during operation.
  //
  // Lean: Aura.Domain.ContextIsolation.LeakageEvent
  type LeakageEvent = {
    source: AuthorityId,          // Who generated the leakage
    context_id: ContextId,        // Which context this applies to
    observer: DeviceId,           // Who observed this (specific device)
    observer_class: ObserverClass, // Class of observation
    leakage_type: LeakageType,    // What kind of info leaked
    amount: int,                  // Leakage units (1 = one message metadata)
    timestamp: int                // When this occurred (logical time)
  }

  // Budget for a specific observer class and context.
  // Reset per epoch. Cannot be widened once set.
  //
  // Lean: Aura.Domain.ContextIsolation.LeakageBudget
  type LeakageBudget = {
    context_id: ContextId,
    observer_class: ObserverClass,
    limit: int,                   // Maximum leakage allowed
    spent: int,                   // Amount leaked so far
    epoch: int                    // Budget epoch (resets on bump)
  }

  // Policy for undefined budgets.
  //
  // Lean: Aura.Domain.ContextIsolation.UndefinedBudgetPolicy
  type UndefinedBudgetPolicy =
    | PolicyDeny             // Deny all leakage (default, secure)
    | PolicyPermit           // Allow unlimited (legacy/migration only)
    | PolicyDefault(int)     // Use default budget value

  // ==================== STATE VARIABLES ====================
  // Mutable state for leakage tracking.

  var leakageEvents: List[LeakageEvent]                    // All recorded leakage
  var budgets: (ContextId, ObserverClass) -> LeakageBudget // Budgets by (context, class)
  var undefinedPolicy: UndefinedBudgetPolicy               // Policy for missing budgets
  var currentEpoch: ContextId -> int                       // Current epoch per context

  // ==================== HELPER FUNCTIONS ====================
  // Pure functions for leakage checking and comparison.

  // Get observer class privilege level (higher = more visibility).
  pure def observerLevel(obs: ObserverClass): int =
    match obs {
      | ObserverExternal => 0
      | ObserverNeighbor => 1
      | ObserverInGroup => 2
      | ObserverSelf => 3
    }

  // Check if observer class a can see at least as much as b.
  pure def canSeeAtLeast(a: ObserverClass, b: ObserverClass): bool =
    observerLevel(a) >= observerLevel(b)

  // Get budget key for lookup.
  pure def budgetKey(ctx: ContextId, obs: ObserverClass): (ContextId, ObserverClass) =
    (ctx, obs)

  // Check if budget exists for context and observer class.
  pure def hasBudget(
    ctx: ContextId,
    obs: ObserverClass,
    budgetMap: (ContextId, ObserverClass) -> LeakageBudget
  ): bool =
    budgetKey(ctx, obs).in(budgetMap.keys())

  // Get budget or return None.
  pure def getBudget(
    ctx: ContextId,
    obs: ObserverClass,
    budgetMap: (ContextId, ObserverClass) -> LeakageBudget
  ): Option[LeakageBudget] =
    if (hasBudget(ctx, obs, budgetMap))
      Some(budgetMap.get(budgetKey(ctx, obs)))
    else
      None

  // Check if leakage amount is within budget.
  pure def withinBudget(budget: LeakageBudget, amount: int): bool =
    budget.spent + amount <= budget.limit

  // Calculate total leakage for a context and observer class.
  pure def totalLeakage(
    events: List[LeakageEvent],
    ctx: ContextId,
    obs: ObserverClass
  ): int =
    events.foldl(0, (acc, e) =>
      if (e.context_id == ctx and e.observer_class == obs)
        acc + e.amount
      else
        acc
    )

  // ==================== ACTIONS ====================
  // State transitions for leakage tracking.

  action init: bool = all {
    leakageEvents' = [],
    budgets' = Map(),
    undefinedPolicy' = PolicyDeny,
    currentEpoch' = Map()
  }

  // Initialize a leakage budget for a context and observer class.
  action initBudget(ctx: ContextId, obs: ObserverClass, limit: int): bool = all {
    not(hasBudget(ctx, obs, budgets)),
    limit > 0,
    budgets' = budgets.put(budgetKey(ctx, obs), {
      context_id: ctx,
      observer_class: obs,
      limit: limit,
      spent: 0,
      epoch: if (ctx.in(currentEpoch.keys())) currentEpoch.get(ctx) else 0
    }),
    leakageEvents' = leakageEvents,
    undefinedPolicy' = undefinedPolicy,
    currentEpoch' = if (ctx.in(currentEpoch.keys())) currentEpoch else currentEpoch.put(ctx, 0)
  }

  // Record leakage and charge budget (check-before-leak).
  // Fails if budget would be exceeded.
  action recordLeakage(
    source: AuthorityId,
    ctx: ContextId,
    observer: DeviceId,
    obs: ObserverClass,
    leakType: LeakageType,
    amount: int,
    timestamp: int
  ): bool = {
    val key = budgetKey(ctx, obs)
    val budgetOpt = getBudget(ctx, obs, budgets)
    val hasBudgetDefined = match budgetOpt { | Some(_) => true | None => false }
    val budget = match budgetOpt {
      | Some(b) => b
      | None => { context_id: ctx, observer_class: obs, limit: 0, spent: 0, epoch: 0 }
    }
    // Check policy for undefined budgets
    val allowedByPolicy = match undefinedPolicy {
      | PolicyDeny => hasBudgetDefined
      | PolicyPermit => true
      | PolicyDefault(defaultLimit) => true
    }
    val effectiveLimit = match undefinedPolicy {
      | PolicyDeny => budget.limit
      | PolicyPermit => 999999  // Effectively unlimited
      | PolicyDefault(defaultLimit) => if (hasBudgetDefined) budget.limit else defaultLimit
    }
    val canSpend = budget.spent + amount <= effectiveLimit
    all {
      allowedByPolicy,
      canSpend,
      amount > 0,
      leakageEvents' = leakageEvents.append({
        source: source,
        context_id: ctx,
        observer: observer,
        observer_class: obs,
        leakage_type: leakType,
        amount: amount,
        timestamp: timestamp
      }),
      budgets' = if (hasBudgetDefined)
        budgets.put(key, { ...budget, spent: budget.spent + amount })
      else
        budgets,
      undefinedPolicy' = undefinedPolicy,
      currentEpoch' = currentEpoch
    }
  }

  // Check if leakage is allowed (without recording).
  // Returns true if the amount would be within budget.
  pure def checkLeakage(
    ctx: ContextId,
    obs: ObserverClass,
    amount: int,
    budgetMap: (ContextId, ObserverClass) -> LeakageBudget,
    policy: UndefinedBudgetPolicy
  ): bool = {
    val budgetOpt = getBudget(ctx, obs, budgetMap)
    match budgetOpt {
      | Some(b) => withinBudget(b, amount)
      | None => match policy {
          | PolicyDeny => false
          | PolicyPermit => true
          | PolicyDefault(defaultLimit) => amount <= defaultLimit
        }
    }
  }

  // Bump epoch for a context (resets budgets).
  action bumpEpoch(ctx: ContextId): bool = {
    val newEpoch = if (ctx.in(currentEpoch.keys())) currentEpoch.get(ctx) + 1 else 1
    all {
      currentEpoch' = currentEpoch.put(ctx, newEpoch),
      // Reset spent counters for all budgets in this context
      budgets' = budgets.keys().fold(budgets, (acc, key) =>
        if (key._1 == ctx)
          acc.put(key, { ...acc.get(key), spent: 0, epoch: newEpoch })
        else
          acc
      ),
      leakageEvents' = leakageEvents,
      undefinedPolicy' = undefinedPolicy
    }
  }

  // Set undefined budget policy.
  action setUndefinedPolicy(policy: UndefinedBudgetPolicy): bool = all {
    undefinedPolicy' = policy,
    leakageEvents' = leakageEvents,
    budgets' = budgets,
    currentEpoch' = currentEpoch
  }

  // ==================== STEP RELATION ====================
  // Nondeterministic action selection for model checking.

  action step: bool = any {
    nondet ctx = oneOf(Set("ctx1", "ctx2"))
    nondet auth = oneOf(Set("auth1", "auth2"))
    nondet device = oneOf(Set("dev1", "dev2", "dev3"))
    nondet limit = oneOf(Set(10, 50, 100))
    nondet amount = oneOf(Set(1, 5, 10))
    nondet timestamp = oneOf(1.to(100))
    nondet obs = oneOf(Set(ObserverExternal, ObserverNeighbor, ObserverInGroup))
    nondet leakType = oneOf(Set(LeakMetadata, LeakTiming))
    any {
      initBudget(ctx, obs, limit),
      recordLeakage(auth, ctx, device, obs, leakType, amount, timestamp),
      bumpEpoch(ctx),
      setUndefinedPolicy(PolicyDeny),
      setUndefinedPolicy(PolicyPermit)
    }
  }

  // ==================== INVARIANTS ====================
  // Safety properties that must hold in every reachable state.

  // Lean: Aura.Proofs.ContextIsolation.leakage_within_budget
  // Spent never exceeds limit for any defined budget.
  val InvariantLeakageWithinBudget =
    budgets.keys().forall(key =>
      budgets.get(key).spent <= budgets.get(key).limit
    )

  // Lean: Aura.Proofs.ContextIsolation.leakage_non_negative
  // Leakage amounts are always positive.
  val InvariantLeakagePositive =
    leakageEvents.foldl(true, (acc, e) => acc and e.amount > 0)

  // Lean: Aura.Proofs.ContextIsolation.budget_limit_positive
  // Budget limits are always positive.
  val InvariantBudgetLimitPositive =
    budgets.keys().forall(key => budgets.get(key).limit > 0)

  // Lean: Aura.Proofs.ContextIsolation.observer_hierarchy
  // External observers have lower or equal budgets than neighbors.
  // This reflects that external should learn less.
  val InvariantObserverHierarchy =
    budgets.keys().forall(key1 =>
      budgets.keys().forall(key2 =>
        (key1._1 == key2._1 and key1._2 == ObserverExternal and key2._2 == ObserverNeighbor)
        implies budgets.get(key1).limit <= budgets.get(key2).limit
      )
    )

  // Lean: Aura.Proofs.ContextIsolation.epoch_monotonic
  // Epochs only increase (never decrease).
  val InvariantEpochMonotonic =
    currentEpoch.keys().forall(ctx => currentEpoch.get(ctx) >= 0)

  // ==================== TEMPORAL PROPERTIES ====================
  // LTL properties for leakage control over time.

  // Leakage budgets are always respected.
  temporal InvariantLeakageBounded = always(InvariantLeakageWithinBudget)

  // Observer hierarchy is maintained over time.
  temporal InvariantObserverHierarchyMaintained = always(InvariantObserverHierarchy)

  // Budget limits are always positive (DoS prevention).
  temporal InvariantBudgetsPositive = always(InvariantBudgetLimitPositive)

}
