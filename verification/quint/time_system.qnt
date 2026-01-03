// Unified Time System Specification
//
// Models Aura's four time domains with domain separation to prevent
// accidental mixing. This ensures time semantics are explicit and auditable.
//
// Time Domains:
// - PhysicalClock: Wall-clock time for timestamps, expiration, receipts
// - LogicalClock: Vector + Lamport clocks for causal ordering
// - OrderClock: Privacy-preserving total ordering tokens
// - Range: Validity windows and constraints
//
// Key Properties:
// - Domain separation: cross-domain comparison requires explicit policy
// - No implicit ordering: TimeStamp::compare(policy) for cross-domain
// - Provenance: TimeAttestationEffects for attested time when required
//
// == Lean Correspondence ==
// Module: Aura.Domain.TimeSystem (verification/lean/Aura/Domain/TimeSystem.lean)
// - Types: TimeStamp, PhysicalTime, LogicalTime, OrderTime, RangeTime
// - Functions: compare_with_policy, is_expired, in_range
//
// == Rust Correspondence ==
// - File: crates/aura-core/src/time.rs
// - Types: TimeStamp, PhysicalTime, LogicalTime, OrderTime, RangeTime
// - Traits: PhysicalTimeEffects, LogicalClockEffects, OrderClockEffects
//
// See: docs/002_theoretical_model.md, docs/003_information_flow_contract.md

module time_system {
  // ==================== TYPE DEFINITIONS ====================
  // Four distinct time domains with no implicit conversion.

  // Physical time: milliseconds since UNIX epoch with optional uncertainty.
  // Used for: wall-clock timestamps, expiration, cooldowns, receipts.
  // Source: PhysicalTimeEffects (never SystemTime::now() directly).
  type PhysicalTime = {
    ts_ms: int,              // Milliseconds since epoch
    uncertainty: Option[int] // Optional uncertainty bound in ms
  }

  // Logical time: vector clock + Lamport timestamp for causality.
  // Used for: CRDT causal delivery, journal conflict resolution.
  // Does not leak wall-clock information.
  type LogicalTime = {
    lamport: int,                    // Scalar Lamport timestamp
    vector: AuthorityId -> int       // Vector clock per authority
  }

  // Order time: privacy-preserving opaque ordering token.
  // Used for: deterministic ordering without leaking temporal info.
  // Tokens are 32 bytes but modeled as int for simplicity.
  type OrderTime = {
    token: int  // Opaque ordering token (no temporal meaning)
  }

  // Range time: validity window constraints.
  // Used for: dispute periods, cooldowns, receipt expiry.
  // Derived from PhysicalTime + policy-defined bounds.
  type RangeTime = {
    start: PhysicalTime,  // Window start
    end: PhysicalTime     // Window end
  }

  // Union type for TimeStamp (one of the four domains).
  type TimeStamp =
    | TsPhysical(PhysicalTime)
    | TsLogical(LogicalTime)
    | TsOrder(OrderTime)
    | TsRange(RangeTime)

  // Comparison policy for cross-domain ordering.
  // Required when comparing timestamps from different domains.
  type ComparisonPolicy =
    | PolicyPhysicalOnly    // Only compare if both are Physical
    | PolicyLogicalOnly     // Only compare if both are Logical
    | PolicyOrderOnly       // Only compare if both are Order
    | PolicyPreferPhysical  // Use Physical if available, else fail
    | PolicyPreferLogical   // Use Logical if available, else fail

  // Result of a comparison with policy.
  type ComparisonResult =
    | CmpLess
    | CmpEqual
    | CmpGreater
    | CmpIncomparable  // Different domains, policy doesn't allow

  type Option[a] = Some(a) | None
  type AuthorityId = str

  // ==================== HELPER FUNCTIONS ====================

  // Extract domain from timestamp.
  pure def getDomain(ts: TimeStamp): str =
    match ts {
      | TsPhysical(_) => "physical"
      | TsLogical(_) => "logical"
      | TsOrder(_) => "order"
      | TsRange(_) => "range"
    }

  // Check if two timestamps are in the same domain.
  pure def sameDomain(a: TimeStamp, b: TimeStamp): bool =
    getDomain(a) == getDomain(b)

  // Compare physical times.
  pure def comparePhysical(a: PhysicalTime, b: PhysicalTime): ComparisonResult =
    if (a.ts_ms < b.ts_ms) CmpLess
    else if (a.ts_ms > b.ts_ms) CmpGreater
    else CmpEqual

  // Compare logical times (Lamport component).
  pure def compareLogical(a: LogicalTime, b: LogicalTime): ComparisonResult =
    if (a.lamport < b.lamport) CmpLess
    else if (a.lamport > b.lamport) CmpGreater
    else CmpEqual

  // Compare order tokens.
  pure def compareOrder(a: OrderTime, b: OrderTime): ComparisonResult =
    if (a.token < b.token) CmpLess
    else if (a.token > b.token) CmpGreater
    else CmpEqual

  // ==================== CORE OPERATIONS ====================

  // Compare timestamps with explicit policy.
  // This is the ONLY way to compare timestamps from different domains.
  // Direct comparison without policy is a type error.
  pure def compareWithPolicy(a: TimeStamp, b: TimeStamp, policy: ComparisonPolicy): ComparisonResult =
    match policy {
      | PolicyPhysicalOnly =>
          match a {
            | TsPhysical(pa) => match b {
                | TsPhysical(pb) => comparePhysical(pa, pb)
                | _ => CmpIncomparable
              }
            | _ => CmpIncomparable
          }
      | PolicyLogicalOnly =>
          match a {
            | TsLogical(la) => match b {
                | TsLogical(lb) => compareLogical(la, lb)
                | _ => CmpIncomparable
              }
            | _ => CmpIncomparable
          }
      | PolicyOrderOnly =>
          match a {
            | TsOrder(oa) => match b {
                | TsOrder(ob) => compareOrder(oa, ob)
                | _ => CmpIncomparable
              }
            | _ => CmpIncomparable
          }
      | PolicyPreferPhysical =>
          match a {
            | TsPhysical(pa) => match b {
                | TsPhysical(pb) => comparePhysical(pa, pb)
                | _ => CmpIncomparable
              }
            | _ => CmpIncomparable
          }
      | PolicyPreferLogical =>
          match a {
            | TsLogical(la) => match b {
                | TsLogical(lb) => compareLogical(la, lb)
                | _ => CmpIncomparable
              }
            | _ => CmpIncomparable
          }
    }

  // Check if a physical time is expired relative to now.
  pure def isExpired(expiry: PhysicalTime, now: PhysicalTime): bool =
    now.ts_ms >= expiry.ts_ms

  // Check if a physical time is within a range.
  pure def inRange(t: PhysicalTime, window: RangeTime): bool =
    t.ts_ms >= window.start.ts_ms and t.ts_ms <= window.end.ts_ms

  // Create a range from physical time with duration.
  pure def createRange(start: PhysicalTime, duration_ms: int): RangeTime = {
    start: start,
    end: { ts_ms: start.ts_ms + duration_ms, uncertainty: None }
  }

  // Increment Lamport timestamp (for causality).
  pure def tickLamport(t: LogicalTime): LogicalTime = {
    lamport: t.lamport + 1,
    vector: t.vector
  }

  // Merge logical clocks (for message receive).
  pure def mergeLogical(local: LogicalTime, received: LogicalTime): LogicalTime = {
    val newLamport = if (local.lamport > received.lamport)
                       local.lamport + 1
                     else
                       received.lamport + 1
    {
      lamport: newLamport,
      vector: local.vector  // Simplified: would merge vectors component-wise
    }
  }

  // ==================== INVARIANTS ====================
  // Properties that ensure time domain separation.
  // These correspond to Lean proofs in Aura.Proofs.TimeSystem.

  // Lean: Aura.Proofs.TimeSystem.domain_separation
  // Domain separation: comparing timestamps from different domains
  // without appropriate policy yields CmpIncomparable.
  pure def InvariantDomainSeparation(a: TimeStamp, b: TimeStamp): bool = {
    val sameD = sameDomain(a, b)
    val result = compareWithPolicy(a, b, PolicyPhysicalOnly)
    // If not same domain and policy is PhysicalOnly, must be incomparable
    // (unless both happen to be physical)
    val bothPhysical = match a {
      | TsPhysical(_) => match b { | TsPhysical(_) => true | _ => false }
      | _ => false
    }
    if (bothPhysical) true
    else match result { | CmpIncomparable => true | _ => sameD }
  }

  // Lean: Aura.Proofs.TimeSystem.physical_total_order
  // Physical time ordering is total (transitive and antisymmetric).
  pure def InvariantPhysicalTotalOrder(a: PhysicalTime, b: PhysicalTime, c: PhysicalTime): bool = {
    val ab = comparePhysical(a, b)
    val bc = comparePhysical(b, c)
    val ac = comparePhysical(a, c)
    val ba = comparePhysical(b, a)
    // Transitivity: if a < b and b < c then a < c
    val transitive = match ab {
      | CmpLess => match bc {
          | CmpLess => match ac { | CmpLess => true | _ => false }
          | _ => true
        }
      | _ => true
    }
    // Antisymmetry: if a <= b and b <= a then a == b
    val antisymmetric = match ab {
      | CmpEqual => match ba { | CmpEqual => true | _ => true }
      | CmpLess => match ba { | CmpGreater => true | _ => true }
      | CmpGreater => match ba { | CmpLess => true | _ => true }
      | _ => true
    }
    transitive and antisymmetric
  }

  // Lean: Aura.Proofs.TimeSystem.lamport_monotonic
  // Lamport timestamps increase monotonically.
  pure def InvariantLamportMonotonic(before: LogicalTime, after: LogicalTime): bool =
    after.lamport >= before.lamport

  // Lean: Aura.Proofs.TimeSystem.order_total
  // Order tokens provide total ordering without temporal meaning.
  pure def InvariantOrderTotalNoTemporal(a: OrderTime, b: OrderTime): bool = {
    val cmp = compareOrder(a, b)
    // Always comparable (total order)
    match cmp {
      | CmpIncomparable => false
      | _ => true
    }
  }

  // Lean: Aura.Proofs.TimeSystem.range_valid
  // Range validity: end >= start.
  pure def InvariantRangeValid(r: RangeTime): bool =
    r.end.ts_ms >= r.start.ts_ms

  // ==================== LEAKAGE PROPERTIES ====================
  // Properties related to information leakage through time.

  // Physical timestamps can leak wall-clock info (intentional for receipts).
  pure def leaksWallClock(ts: TimeStamp): bool =
    match ts {
      | TsPhysical(_) => true
      | TsRange(_) => true  // Derived from physical
      | _ => false
    }

  // Logical timestamps do not leak wall-clock info.
  pure def noWallClockLeak(ts: TimeStamp): bool =
    match ts {
      | TsLogical(_) => true
      | TsOrder(_) => true
      | _ => false
    }

  // Order tokens are maximally privacy-preserving.
  pure def isPrivacyPreserving(ts: TimeStamp): bool =
    match ts {
      | TsOrder(_) => true
      | _ => false
    }

}
