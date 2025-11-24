# Quint + Choreo

## 0) What Quint is (and isn’t)

Quint is a typed specification language for describing **state machines** (transition systems) and **properties** (mostly invariants) you can check by simulation or model checking. When a property doesn’t hold, Quint produces a **counter-example trace** showing exactly how to reproduce the bug—no “false positives.” See [Why Quint](https://quint-lang.org/docs/why).

You typically:

1. model states and step transitions,
2. write invariants (and sometimes temporal properties),
3. run `quint run` (simulator) for fast feedback, or `quint verify` (Apalache) for formal checking.

## 1) Core mental model

A Quint spec is a **module** with:

* **constants** and **state variables** (typed),
* **actions** that update state using the prime operator (`x'` = value “in next state”),
* **runs** (structured sequences of actions) and **temporal formulas** (optional),
* **invariants** you check against all reachable states.

### Modes (semantic strata)

Quint makes the “levels” explicit via **modes** (stateless, state, nondeterminism, action, run). Write expressions in the right mode (e.g., `action` definitions in action mode; invariants are boolean/state-mode). This clarity removes common confusion found in unstratified notations.

## 2) Types & data

Quint is **typed**. You annotate variables/constants and may rely on inference for many other definitions. Core types: `bool`, `int`, `str`, sets, lists, tuples, records, maps (functions), sum types, arrows (`T1 -> T2` for functions; `(T1,…,Tn) => R` for operators), and polymorphic aliases via `type`. Example: `type Option[a] = Some(a) | None`.

Everyday data you’ll use a lot:

* **Sets**: `Set[T]` with `map`, `filter`, `forall`, `exists`, `union`, `intersect`, `subseteq`, etc.
* **Maps (functions)**: `Map(k -> v, …)`, `get`, `put`, `keys`, `mapBy`, `setOfMaps`.
* **Lists**: `List[T]`, `length`, `slice`, `foldl`, `head`, `tail`.

> Tip: prefer Sets when order doesn’t matter, Lists when it does.

## 3) Modules, imports, instances

* Define with `module M { … }`.
* Import everything: `import A.* from "A"`, or as a namespace: `import A as a from "A"`.
* **Module instances** let you parameterize modules (e.g., `import A(N = 3) as a3` then call `a3::init`).
* Namespaces may use `::` to qualify references.

## 4) State, actions, and next-state updates

Declare **state variables** with `var x: int`.
Write **actions** as boolean formulas about current/next state, e.g.:

```quint
var x: int

action init = x' = 1

action step = all {
  x < 5,
  x' = x + 1
}
```

Read as: `init` sets the next‐state value of `x` to 1. `step` requires `x < 5` and increments.

In the REPL, applying an action evaluates to `true` (took the step) or `false` (guard failed).

### Nondeterminism & control flow

Quint provides structured composition (`all { … }`, disjunctions, non-det choice) and “flow” operators to combine action fragments, plus `any`/`nondet` patterns used by the simulator/REPL examples.

## 5) Runs & temporal

You can compose steps into **runs** and use **temporal operators** like `always`, `eventually`, `next`, fairness hints, etc. In practice, **invariants** cover most work; temporal properties are more advanced and partially supported (simulator doesn’t check temporal). Use Apalache via `quint verify` for temporal when needed.

## 6) Properties that matter

Start with **safety invariants**—boolean expressions that must hold in **every** state (e.g., balances never negative). Liveness often needs temporal reasoning and fairness; if you just need “good things happen sometimes,” use **witnesses** in the simulator to count traces where a condition becomes true.

Examples:

* Invariant: `val no_negatives = ADDRESSES.forall(a => balances.get(a) >= 0)`
* Witness counting: `quint run ... --witnesses=alice_more_than_bob`
* Negated-invariant trick to **show** a witnessing trace: `--invariant="not(alice_more_than_bob)"`.

### Inductive invariants (advanced but powerful)

Use `--inductive-invariant` to prove an invariant **without** exploring all states: prove base + step + implication. `quint verify` orchestrates the Apalache calls that check these proof obligations.

## 7) Tooling (complete catalog)

This section consolidates all day-to-day tools and how to use them, aligned with the official docs.

### 7.1 Simulator — `quint run` ([docs](https://quint-lang.org/docs/simulator))

**Purpose.** Generate sample executions (traces) and check **invariants** on every state. Useful for fast feedback and finding **short counter-examples**.

**Basics**

* `quint run spec.qnt [--module M] --invariant inv`
* `--invariants inv1 inv2 ...` to check multiple values that evaluate to `bool`.
* `--max-steps N` bound on each execution length.
* `--seed S` to make runs reproducible.
* `--out-itf trace.itf.json` to save a trace in ITF (Interchange Trace Format).
* Witnesses: `--witnesses name1 name2` to **count** how many runs satisfy those boolean values at least once.

**Notes**

* The simulator **does not** check temporal formulas; use a model checker for that.
* Output is either: a violation trace (counter-example) or the longest sample explored for your settings.

### 7.2 Model Checking — `quint verify` (Apalache) ([Checking properties](https://quint-lang.org/docs/checking-properties), [Model checkers](https://quint-lang.org/docs/model-checkers))

**Purpose.** Bounded model checking over steps. Formally verifies that your properties hold up to a bound; if not, returns a counter-example.

**Basics**

* `quint verify spec.qnt [--module M] --invariant inv`
* Multi: `--invariants inv1 inv2 ...`
* Temporal: `--temporal prop1 prop2` (checked by the model checker, not by the simulator).
* Inductive: `--inductive-invariant inv` to prove base/step obligations.
* Bounds: `--max-steps N`.

**Requirements**

* JDK ≥ 17 installed (Apalache is fetched/invoked under the hood).

### 7.3 REPL — interactive exploration ([docs](https://quint-lang.org/docs/repl))

**Purpose.** Evaluate expressions, build/step actions, and run short executions interactively; great for sketching specs and sanity checks.

**Basics**

* Start: `quint`
* Load a module: `quint -r file.qnt::ModuleName` (then use names unqualified inside the REPL).
* Evaluate: `>>> Set(1,2).map(x => x + 1)`
* Define/step: you can define actions and apply them; the REPL returns whether the action was enabled and shows updated state when stepping.
* Explore traces produced by the simulator inside the REPL when loaded with a run.

### 7.4 Literate specifications — weave Markdown & Quint ([docs](https://quint-lang.org/docs/literate))

**Purpose.** Keep prose and code together; extract `.qnt` files from Markdown.

**Basics**

* Write Markdown with fenced code blocks labeled `quint` and a **target file** plus an **append operator**:

  ````
  ```quint myspec.qnt +=
  module Counter {
    var n: int
    // ...
  }
  ````

  ```
  ```
* Each `quint` code block declares: (1) language `quint`, (2) target file (e.g., `myspec.qnt`), and (3) the `+=` append directive. Multiple blocks to the same file are concatenated in order.

### 7.5 Model‑Based Testing (MBT) ([docs](https://quint-lang.org/docs/model-based-testing))

**Purpose.** Generate **many** concrete scenarios from one model and drive a system‑under‑test (SUT) via a thin test harness.

**Workflow**

* Encode actions as SUT calls (driver) and use the simulator to generate action sequences.
* Use witnesses/invariants to assert end‑to‑end properties across generated runs.
* Practical pattern: export ITF traces from `quint run` and replay them in your test harness.

### 7.6 Model checkers — the landscape

**Purpose.** Understand what engines exist and how Quint integrates.

* **Apalache** (primary): translates Quint to TLA+/SMT constraints and checks safety/temporal within a bound; returns counter‑examples on failure.
* Simulator vs. model checker: simulator is fast sampling; model checker gives formal guarantees (within bounds) and can prove inductive invariants.

### 7.7 CLI grab‑bag

* Parse/Typecheck: `quint parse spec.qnt`, `quint typecheck spec.qnt` (emit JSON IR and type/effect info).
* Tests: `quint test` (randomized tests over values/operators).
* Common flags: `--verbosity`, `--seed`, `--out`, `--out-itf`.

---

## 7A) Choreo tooling & workflow (see [Choreo tutorial](https://quint-lang.org/choreo/tutorial))

**Getting the library files**

* From the tutorial: download `choreo.qnt`, `template.qnt`, and the `spells/` files (e.g., via `curl`) into your project.

**Core development loop**

1. Define types (`Node`, `Message`, `StateFields`, and optional `CustomEffects`, `Event`, `Extensions`).
2. Implement transitions and **listeners** using the `cue` pattern.
3. Simulate with `quint run` and add invariants/witnesses.
4. Verify with `quint verify` where needed.

**The `cue` pattern**

* Compose listeners that **detect** relevant messages and **emit** transitions. Keeps routing/filtering centralized and logic composable.

**Custom effects & environment extensions**

* Declare new effect variants and specify how the environment interprets them (e.g., delivery, delays, faults). Useful for adversarial or lossy networks.

**`step_micro`**

* Switch stepping strategy to process **one message at a time**, exposing fine‑grained interleavings for deeper exploration/proofs.

---

## 8) Language staples (quick reference)

* **Comments**: `//` and `/* ... */`.
* **Lambdas**: `x => expr`.
* **Records**: `{ name: "TLA+", age: 33 }`, field update `{ ...r, name: "Quint" }`.
* **Sum types**: `type Message = Commit | Abort | Prepared(Node)` with pattern-matching via `match`.
* **Sets/Maps/Lists**: see operations in the cheatsheet; sets are the backbone of many models.
* **No recursive operators** (design choice). Work with folds/iteration over finite data and explicit runs instead.

For an authoritative list of **built-in operators** (arithmetic, logic, set/map/list ops, temporal, run/action helpers), consult the Language Manual and built-ins reference in [docs/lang](https://quint-lang.org/docs/lang).

---

## 9) Syntax reference from lessons — [Booleans](https://quint-lang.org/docs/lessons/booleans), [Integers](https://quint-lang.org/docs/lessons/integers), [Sets](https://quint-lang.org/docs/lessons/sets), [Coin](https://quint-lang.org/docs/lessons/coin)

The following is an exhaustive syntax/ops digest pulled from the official lessons you referenced. It’s organized by topic with the concrete spellings, method forms, and sugar.

### Booleans

**Literals**

* `true`, `false` (strict: no implicit conversions)

**Unary/binary ops (function + dot forms)**

* Negation: `not(x)` ⇔ `x.not()`
* Conjunction (and): `x and y` ⇔ `and(x, y, …)` ⇔ `x.and(y)` and block form `and { e1, e2, e3 }`
* Disjunction (or): `x or y` ⇔ `or(x, y, …)` ⇔ `x.or(y)` and block form `or { e1, e2, e3 }`
* Implication: `x implies y` ⇔ `x.implies(y)` (equivalent to `not(x) or y`)
* Equivalence: `x iff y` ⇔ `x.iff(y)` (boolean-only equality)
* Equality/inequality: `x == y`, `x != y` (args must have same type)

**Notes**

* Most operators have an n‑ary variant (e.g., `and(x,y,z)`) and an OOP/dot variant (`x.and(y)`).
* Avoid `false.not` (without `()`), which is parsed as record-field access.

### Integers

**Literals**

* Decimal literals, including negatives: `0`, `2`, `-3`, … (arbitrary‑precision/big integers)

**Arithmetic**

* Exponentiation: `i ^ j`
* Addition/subtraction: `i + j`, `i - j` (same precedence; left‑to‑right)
* Multiplication: `i * j`
* Division/remainder: `i / j`, `i % j` with the law `i == (i / j) * j + (i % j)` (for `j != 0`)
* Unary minus (negation): `-i`

**Comparison**

* `i < j`, `i <= j`, `i > j`, `i >= j`, `i == j`, `i != j`

**Ranges that produce sets (used often with nondet/tests)**

* `a.to(b)` produces the set `{a, a+1, …, b}` when `a <= b` (commonly seen as `0.to(N)`), then you can call `.oneOf()` (see **Coin** below).

### Sets (and tuples used within)

**Tuple syntax & projection**

* Construct: `(x, y, z)`
* Access: `t._1`, `t._2`, …

**Set literals & membership**

* Construct: `Set(e1, e2, …)` (duplicates removed)
* Membership (two equivalent forms):

  * Element‑in‑set: `e.in(S)`
  * Set contains element: `S.contains(e)`

**Quantifiers on sets**

* Existence: `S.exists(x => P(x))`
* Universality: `S.forall(x => P(x))`

**Core higher‑order ops**

* Map: `S.map(x => f(x))`
* Filter: `S.filter(x => P(x))`
* Size: `S.size()` → `int`
* Flatten (for set‑of‑sets): `S.flatten()` → union of direct elements
* (Common additional set ops used broadly in specs though not emphasized in the lesson): `S.union(T)`, `S.intersect(T)`, `S.diff(T)`, `S.subseteq(T)`

**Reachability & folds (patterns showcased in the lesson)**

* Progressive construction using maps/filters and checks with `exists/forall`.
* Folding patterns over sets (e.g., accumulating reachability or computing derived properties) via `fold` operators.
* Powersets: use the powerset operator/util to enumerate subsets (commonly written via `powerset(S)` in examples; pair with `filter`/`exists` to search).

### Coin / Protocol anatomy (syntax highlights)

**Module & types**

* Module: `module coin { ... }`
* Type aliases: `type Addr = str`, `type UInt = int` (and custom invariants/predicates like `isUInt`)

**State & constants**

* Constants: `const ADDR: Set[Addr]`
* State vars: `var balances: Map[Addr, UInt]`, `var minter: Addr`

**Initializer & actions**

* Prime updates in an action/initializer block:

  ```quint
  action init = all {
    // nondet choice (see below) then assignments
    minter' = sender,
    balances' = ADDR.mapBy(a => 0)
  }
  ```
* Guarded action with locals and `require` checks:

  ```quint
  action mint(sender: Addr, receiver: Addr, amount: UInt): bool = all {
    require(sender == minter),
    val newBal = balances.get(receiver) + amount,
    all {
      require(isUInt(newBal)),
      balances' = balances.set(receiver, newBal),
      minter' = minter
    }
  }
  ```

**Nondeterministic choice**

* From a set literal/range: `nondet x = 0.to(MAX_UINT).oneOf()`
* From an existing set: `nondet sender = oneOf(ADDR)` or `ADDR.oneOf()`

**Sequencing runs in REPL/tests**

* Compose steps: `init.then(mint(minter, "bob", 5)).then(send("bob", "eve", 3))`
* Conditional action in a run: `if (cond) action1 else action2`

**Map helpers (commonly used with state)**

* Build map by domain: `ADDR.mapBy(a => 0)`
* Read/write entries: `m.get(k)`, `m.set(k, v)` (aka `put` in some references)

**Testing hooks**

* `run` blocks with nondet inputs and fixed sequences; invariants expressed as values and passed to `quint run`/`verify`.

---

> With these lesson-derived primitives consolidated here, the guide now doubles as a syntax crib sheet you can use while modeling.

---

# Choreo: a "batteries-included" framework for distributed protocols

Choreo is a Quint library + methodology that hands you the best-known modeling techniques for message-passing protocols, so you focus on **protocol logic** instead of boilerplate. It adopts the **message soup** technique for efficiency, separates **local state** from **environment**, and treats **timeouts as internal events**.

## Important Note on Choreographic Programming in Quint

While the name "Choreo" suggests choreographic programming, Quint's effect system makes it difficult to implement true choreographic patterns where:
- A global viewpoint describes message flows between participants
- Local projections are automatically derived
- Session types enforce protocol correctness

The limitations include:
- Match expressions cannot mix pure and action branches
- Actions cannot be used inside higher-order functions like `exists`, `forall`, or `fold`
- Complex message routing patterns become cumbersome to express

For true choreographic programming with session types, consider:
1. Using dedicated choreographic languages (e.g., Scribble, Choral)
2. Using the Choreo library patterns shown below, which provide good abstractions for distributed protocols
3. Writing simpler message-passing specifications without choreographic patterns

## 1) How you use it

Add:

```quint
import choreo(processes = NODES) as choreo from "./choreo"
```

Then you define:

* **types**: `Node`, `Message`, `StateFields`, and any protocol enums (e.g., `Role`, `Stage`);
* **listeners** using cues that react to relevant messages;
* **transitions** that return `{ post_state: ..., effects: Set(...) }`.
  The framework supplies **built-in effects** like `choreo::Broadcast(msg)`.

### A listener via `cue`

```quint
pure def listen_proposal_in_propose(ctx: LocalContext): Set[ProposeMsg] = { ... }

pure def broadcast_prevote_for_proposal(ctx: LocalContext, p: ProposeMsg): Transition = {
  { post_state: { ...ctx.state, stage: PreVoteStage },
    effects: Set(choreo::Broadcast(PreVote(message))) }
}

pure def main_listener(ctx: LocalContext): Set[Transition] = Set(
  choreo::cue(ctx, listen_proposal_in_propose, broadcast_prevote_for_proposal)
)
```

Here the `cue` wires “what to listen for” to “how to react,” producing protocol transitions.

### Transition shape

```quint
{
  post_state: { ...s, stage: PreVoteStage },
  effects: Set(choreo::Broadcast(PreVote(message)))
}
```

Choreo handles delivery semantics for built-in effects; you can add **custom effects** with handlers too.

## 2) Tutorial blueprint: Two-Phase Commit ([Choreo tutorial](https://quint-lang.org/choreo/tutorial))

The tutorial walks you from a **template** to a full 2PC spec:

* define roles & stages:
  `type Role = Coordinator | Participant`
  `type Stage = Working | Prepared | Committed | Aborted`
* messages: `CoordinatorAbort | CoordinatorCommit | ParticipantPrepared(Node)`
* state fields: `{ role: Role, stage: Stage }`
* helper: `get_prepared_msgs`
* participant transitions: spontaneous prepare/abort, follow coordinator
* coordinator transitions: decide commit when all prepared; abort anytime
* wire with listeners; add properties/tests; simulate/verify.

## 3) cue-pattern deep-dive, custom effects, micro-steps ([Cue pattern](https://quint-lang.org/choreo/cue-pattern), [Custom effects & extensions](https://quint-lang.org/choreo/custom-effects-extensions), [`step_micro`](https://quint-lang.org/choreo/step-micro))

* **cue-pattern**: A composable way to declare listeners → reactions, keeping routing/filtering centralized and **protocol logic** clean.
* **Custom effects & environment extensions**: Define your own effect types and specify how the environment interprets them (e.g., network adversary models, delays, loss, Byzantine).
* **`step_micro`**: Switch the stepping strategy to process **one message at a time** (fine-grained interleavings) when you need per-message scheduling control for exploration or proofs.

## 4) Why Choreo tends to win

Compared to hand-rolled, Choreo:

* encodes “message soup” efficiently,
* enforces a clean separation between local logic and environment,
* makes timeouts just another internal event,
* comes with examples (Two-Phase Commit, Tendermint, Alpenglow, Monad BFT).

---

# Patterns & Practices

* **Model first, shrink the state space**: prefer small domains, bounded integers, constrained nondet choices; Apalache is bounded by `--max-steps`.
* **Invariants first**: start with safety; add witnesses for “sometimes” behavior; only reach for temporal once necessary.
* **Shortest counter-example**: simulator tries to give you the shortest failing trace—useful for debugging quickly.
* **Use the REPL to sketch**: define state/actions interactively, sanity-check expressions, and iterate.
* **Prefer sets/maps idioms**: they compose well with quantifiers (`forall`, `exists`) and folds.
* **No recursion**: rewrite as folds or bounded loops (via step composition).
* **Export ITF traces** for tooling or CI visualization.

---

# Minimal “starter” template (plain Quint)

```quint
module bank {
  // --- domain
  type Address = str
  const ADDRESSES: Set[Address]

  // --- state
  var balances: Map[Address, int]

  // --- init
  action init = all {
    balances' = ADDRESSES.setToMap(_ => 0)
  }

  // --- a transfer step (one possibility)
  action step = any {
    // nondeterministically pick src/dst and amount within bounds (sketch)
    exists(src in ADDRESSES, dst in ADDRESSES, amount in 0.to(10)) {
      all {
        src != dst,
        balances.get(src) >= amount,
        balances' = balances
          .put(src, balances.get(src) - amount)
          .put(dst, balances.get(dst) + amount)
      }
    }
  }

  // --- properties
  val total = ADDRESSES.map(a => balances.get(a)).sum()
  val total_nonnegative = total >= 0
  val no_negatives = ADDRESSES.forall(a => balances.get(a) >= 0)
}
```

Simulate and check:

```
quint run bank.qnt --invariants total_nonnegative no_negatives --max-steps=20
```

Then verify (bounded) with Apalache:

```
quint verify bank.qnt --invariants total_nonnegative no_negatives --max-steps=10
```

---

## 10) Comprehensive syntax catalog (cross‑referenced with the Language Manual)

> This section consolidates *all* surface syntax you’ll commonly use, aligned with the Language Manual at [quint-lang.org/docs/lang](https://quint-lang.org/docs/lang). If something here ever drifts, prefer the manual.

### 10.1 Literals, identifiers, comments

* **Identifiers**: `[a-zA-Z_][a-zA-Z0-9_]*`
* **String literals**: `"hello"`
* **Boolean literals**: `true`, `false`
* **Integer literals**: `0`, `-1`, `314159...` (arbitrary precision)
* **Comments**: `// line`, `/* block */`

### 10.2 Types

* **Base**: `bool`, `int`, `str`
* **Uninterpreted**: `TYPE_NAME` (caps)
* **Type vars**: `a`, `b`, ...
* **Constructors**: `Set[T]`, `List[T]`, tuples `(T1, T2, ...)`, records `{ f1: T1, f2: T2 }`, function `T1 -> T2`, operator `(A1, ..., An) => R`
* **Sum types (algebraic)**: `type T = L1(T1) | L2(T2) | ...`
  - Variants take positional parameters, not named fields
  - For named fields, use record types as parameters: `type Msg = Send({from: str, to: str})`
  - Match expressions: `match expr { | Variant(x) => ... | _ => ... }`
* **Type aliases**: `type Temperature = int`
* **Polymorphic aliases**: `type Option[a] = Some(a) | None`

### 10.3 Modules & top‑level definitions

* **Module**:

  ```quint
  module M { /* definitions */ }
  ```
* **Constants**: `const N: int`
* **Assumptions**: `assume Name = <expr>` (or anonymous: `assume _ = <expr>`) — stateless
* **State vars**: `var x: T` — state mode
* **Values/operators**:

  * Stateless values: `pure val X: T = <expr>`
  * Stateful values: `val Y = <expr>` (may depend on state)
  * Stateless ops: `pure def f(a: A): B = <expr>`
  * Stateful ops: `def g(a: A): B = <expr>`
* **Type aliases**: `type NAME = <type>` (or polymorphic)
* **Imports / namespaces**:

  * `import A.* from "A"`
  * `import A as a from "A"`
  * Qualify with `A::foo` or `a::foo`
* **Module instances** (parameterized imports): `import Counter(N = 5) as c5`

### 10.4 Modes (where an expression is allowed)

* Stateless, State, Non‑determinism, Action, Run, Temporal
* Subsumption highlights: Action ⟂ Temporal (not mixed); Run ≥ {Stateless, State, Action}

### 10.5 Expressions & control

* **If‑then‑else**: `if (p) e1 else e2`
* **Let‑style via vals inside defs**: `val x = ...; <expr using x>` (use inside `def` bodies)
* **Equality**: `==`, `!=` (same‑type)
* **Boolean ops**: `not(x)` / `x.not()`, `x and y` / `x.and(y)` / `and{...}`, `x or y` / `x.or(y)`, `x implies y`, `x iff y`
* **Arithmetic**: `^`, `*`, `/`, `%`, `+`, `-`, unary `-`
* **Comparison**: `<`, `<=`, `>`, `>=`
* **Match expressions and effect system**: Match expressions cannot mix pure and action branches. If one branch returns an action, all branches must be actions. Extract pure computations before match expressions when used in actions.

### 10.6 Tuples & records

* **Tuple**: `(e1, e2, ...)`; projection: `t._1`, `t._2`, ...
* **Record**: `{ f: e, g: e2 }`; update/extend: `{ ...r, f: eNew }`

### 10.7 Sets

* **Literal**: `Set(e1, e2, ...)`
* **Membership**: `e.in(S)` ⇔ `S.contains(e)`
* **Combinators**: `map`, `filter`, `size`, `flatten`
* **Predicates/quantifiers**: `exists`, `forall`, `subseteq`
* **Binary ops**: `union`, `intersect`, `diff`
* **Ranges**: `a.to(b)` → `Set[int]`
* **Choice**: `S.oneOf()` (used with `nondet`)

### 10.8 Maps (finite functions)

* **Construct/derive**: `D.setToMap(k => v)` / `D.mapBy(k => v)`
* **Access/update**: `m.get(k)`, `m.set(k, v)` (**alias:** `m.put(k, v)`)
* **Keys**: `m.keys()`; mapping: `m.map((k,v) => ...)`

### 10.9 Lists (sequences)

* **Type**: `List[T]`; common ops: `length`, `head`, `tail`, `slice`, `select`, `foldl`

### 10.10 Quantifiers (bounded & unbounded)

* **Over sets**: `S.exists(x => P)`, `S.forall(x => P)`
* **Unbounded**: integer domain quantifiers (use carefully; see manual’s “Unbounded quantifiers”)

### 10.11 Actions (next‑state logic)

* **Prime**: `x' = e` (assign next value of state var `x`)
* **Choreographic programming limitation**: Quint's effect system makes it difficult to implement true choreographic programming patterns. Actions cannot be used inside higher-order functions like `exists`, `forall`, or `fold`. Consider using the Choreo library instead for distributed protocol specifications.
* **Conjunction blocks**: `all { e1, e2, ... }` (must all hold)
* **Disjunction blocks**: `any { e1, e2, ... }` (non‑deterministic choice among branches)
* **Preconditions / assertions**: `require(p)` to guard; `assert(p)` for diagnostic checks
* **Delayed assignment** (define first, assign later within `all { ... }` semantics)
* **Non‑deterministic choice**:

  ```quint
  nondet k = S.oneOf()   // or 0.to(N).oneOf()
  ```
* **Assert inside actions**: `assert(<bool>)` (debug/diagnostics)

### 10.12 Runs (finite executions)

* **Run expression**: sequences of actions producing executions
* **Then**: `A.then(B)`
* **Reps**: `A.reps(n)` (repeat up to `n`)
* **Example/Expect/Fail**: helpers for REPL/tests

### 10.13 Temporal operators

* **Always / Eventually / Next**: `always(P)`, `eventually(P)`, `next(P)`
* **OrKeep** (TLA+ `[A]_x`): `orKeep(A, x)` / `A.orKeep(x)`
* **MustChange** (TLA+ `<A>_x`): `mustChange(A, x)` / `A.mustChange(x)`
* **Enabled** (TLA+ `ENABLED A`): `enabled(A)` / `A.enabled`
* **Fairness**: `weakFair(A, x)` / `A.weakFair(x)`; `strongFair(A, x)` / `A.strongFair(x)`
* **Guarantees**: `guarantees(P, Q)` / `P.guarantees(Q)`

### 10.14 Instances & imports (advanced)

* **Common**: `import Mod as m from "Mod"`; use `m::name`
* **Anonymous/parameterized instances**: `import M(N = 3) as m3`

---

# Appendix

* **Language Manual** — [docs/lang](https://quint-lang.org/docs/lang)
* **Checking properties** — [docs/checking-properties](https://quint-lang.org/docs/checking-properties)
* **REPL tutorial** — [docs/repl](https://quint-lang.org/docs/repl)
* **Literate Quint** — [docs/literate](https://quint-lang.org/docs/literate)
* **Model‑based testing** — [docs/model-based-testing](https://quint-lang.org/docs/model-based-testing)
* **Model checkers** — [docs/model-checkers](https://quint-lang.org/docs/model-checkers)
* **Simulator details** — [docs/simulator](https://quint-lang.org/docs/simulator)
* **What Quint does / Why Quint** — [docs/why](https://quint-lang.org/docs/why)
* **Choreo tutorial** — [choreo/tutorial](https://quint-lang.org/choreo/tutorial)
* **Cue pattern** — [choreo/cue-pattern](https://quint-lang.org/choreo/cue-pattern)
* **Custom effects & extensions** — [choreo/custom-effects-extensions](https://quint-lang.org/choreo/custom-effects-extensions)
* **Micro‑steps (`step_micro`)** — [choreo/step-micro](https://quint-lang.org/choreo/step-micro)
* **Lessons** — [Booleans](https://quint-lang.org/docs/lessons/booleans), [Integers](https://quint-lang.org/docs/lessons/integers), [Sets](https://quint-lang.org/docs/lessons/sets), [Coin](https://quint-lang.org/docs/lessons/coin)
