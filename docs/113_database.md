# Database Architecture

This document specifies the architecture for Aura's distributed database layer. The journal is the database. Biscuit is the query engine. CRDTs are the replication layer.

## 1. Core Principles

### 1.1 Database-as-Journal Equivalence

Aura's fact-based journal functions as the database. There is no separate database layer. The equivalence maps traditional database concepts to Aura components.

A database table corresponds to a journal reduction view. A database row corresponds to a fact implementing `JoinSemilattice`. A database transaction corresponds to an atomic fact append. A database index corresponds to Merkle trees and Bloom filters. A database query corresponds to Biscuit Datalog evaluation. Database replication corresponds to `CrdtCoordinator` with delta sync.

### 1.2 Authority-First Data Model

Unlike traditional databases that model users and permissions, Aura's database is partitioned by cryptographic authorities. An `AuthorityId` owns facts that implement `JoinSemilattice`. State is derived from those facts.

Data is naturally sharded by authority. Cross-authority operations require explicit choreography. Privacy is the default because no cross-authority visibility exists without permission.

## 2. Query System

### 2.1 Biscuit Datalog Engine

Biscuit includes a full Datalog engine. Aura extends its usage beyond authorization to general queries.

```rust
pub struct AuraQuery {
    authorizer: biscuit_auth::Authorizer,
}

impl AuraQuery {
    pub fn add_journal_facts(&mut self, facts: &[Fact]) -> Result<()> {
        for fact in facts {
            self.authorizer.add_fact(fact.to_biscuit_fact()?)?;
        }
        Ok(())
    }

    pub fn query(&self, rule: &str) -> Result<Vec<biscuit_auth::Fact>> {
        self.authorizer.query(rule)
    }
}
```

The `AuraQuery` wrapper loads facts from the journal into Biscuit's authorizer. Query execution uses Biscuit's Datalog engine directly.

### 2.2 Query Scoping

Query scoping uses the existing `ResourceScope` type from `aura-core`. The `ScopedQuery` wraps a resource scope with a query string.

```rust
pub struct ScopedQuery {
    scope: ResourceScope,
    query: String,
}

impl ScopedQuery {
    pub async fn execute<E: DatabaseEffects + AuthorizationEffects>(
        &self,
        effects: &E,
        token: &Biscuit,
    ) -> AuraResult<Vec<Fact>> {
        effects.authorize(token, "query", &self.scope).await?;
        let scoped_facts = effects.facts_in_scope(&self.scope).await?;
        let mut query = AuraQuery::new();
        query.add_journal_facts(&scoped_facts)?;
        query.query(&self.query)
    }
}
```

Authorization happens via the existing guard chain before query execution. Facts are filtered to the requested scope before loading into the query engine.

### 2.3 Built-in Predicates

Biscuit's Datalog provides string operations, comparisons, and membership tests. Aura adds authority-aware predicates by injecting ambient facts.

```rust
impl AuraQuery {
    fn add_authority_context(&mut self, authority: AuthorityId) -> Result<()> {
        self.authorizer.add_fact(fact!("current_authority({authority})"))?;
        Ok(())
    }
}
```

The authority context enables queries to reference the current authority without hardcoding identifiers.

## 3. Fact Types

### 3.1 Semilattice Implementation

All fact types implement the `JoinSemilattice` trait from `aura-core`. Facts are append-only. The join operation is set union.

```rust
impl JoinSemilattice for FactSet {
    fn join(&self, other: &Self) -> Self {
        let mut result = self.clone();
        for fact in &other.facts {
            if !result.contains(fact) {
                result.insert(fact.clone());
            }
        }
        result
    }
}
```

This implementation guarantees convergence. Two replicas with the same facts will always produce the same result regardless of operation order.

### 3.2 Fact Variants

Facts come in three variants. Simple facts contain a predicate, value, and authority. Temporal facts add a timestamp using the unified `TimeStamp` type. Relational facts express cross-authority references with optional context scoping.

```rust
pub enum Fact {
    Simple { predicate: String, value: Value, authority: AuthorityId },
    Temporal { entity: EntityId, predicate: String, time: TimeStamp, authority: AuthorityId },
    Relational { subject: EntityId, predicate: String, object: EntityId,
                 context: Option<ContextId>, authority: AuthorityId },
}
```

All variants implement `JoinSemilattice` via `FactSet`.

### 3.3 Constraint Types

Restrictions use `MeetSemilattice` to narrow when combined. Time windows take the latest start and earliest end. Capability sets take the intersection.

```rust
impl MeetSemiLattice for TimeWindow {
    fn meet(&self, other: &Self) -> Self {
        Self {
            start: max(self.start, other.start),
            end: min(self.end, other.end),
        }
    }
}
```

This ensures capability attenuation works correctly. Delegated capabilities can only be narrowed, never expanded.

## 4. Indexing Layer

### 4.1 Index Types

The `IndexedJournalEffects` trait extends `JournalEffects` with efficient lookups.

```rust
pub trait IndexedJournalEffects: JournalEffects {
    async fn facts_by_predicate(&self, predicate: &str) -> AuraResult<Vec<Fact>>;
    async fn facts_by_authority(&self, authority: &AuthorityId) -> AuraResult<Vec<Fact>>;
    async fn facts_in_range(&self, start: TimeStamp, end: TimeStamp) -> AuraResult<Vec<Fact>>;
    fn might_contain(&self, predicate: &str, value: &Value) -> bool;
}
```

The `might_contain` method uses Bloom filters for fast negative answers. The other methods use B-tree indexes for ordered lookups.

### 4.2 Index Implementation

The `AuthorityIndex` structure maintains three index types.

```rust
pub struct AuthorityIndex {
    merkle_tree: MerkleTree<FactHash>,
    predicate_filters: BTreeMap<String, BloomFilter>,
    by_predicate: BTreeMap<String, Vec<FactId>>,
    by_authority: BTreeMap<AuthorityId, Vec<FactId>>,
    by_time: BTreeMap<TimeStamp, Vec<FactId>>,
}
```

Merkle trees provide integrity verification. Bloom filters provide fast membership tests with less than 1% false positive rate. B-trees provide ordered lookups with O(log n) performance.

Indexes update on fact commit. Performance target is less than 10ms for 10k facts.

## 5. Subscription API

### 5.1 Fact Subscriptions

The `DatabaseSubscriptionEffects` trait enables reactive updates.

```rust
#[async_trait]
pub trait DatabaseSubscriptionEffects: JournalEffects {
    async fn subscribe_facts(&self, filter: FactFilter) -> AuraResult<FactStream>;
    async fn subscribe_query<T: FromQueryResult>(
        &self,
        query: &str,
        scope: QueryScope,
    ) -> AuraResult<Dynamic<T>>;
}

pub struct FactStream {
    receiver: broadcast::Receiver<FactDelta>,
}

pub enum FactDelta {
    Added(Fact),
}
```

Facts are append-only. The delta type has only an `Added` variant. There is no `Removed` variant because facts implementing `JoinSemilattice` cannot be retracted.

### 5.2 Materialized Views

Materialized views wrap `CvHandler` from the CRDT infrastructure.

```rust
pub struct MaterializedView<T: JoinSemilattice + Clone> {
    handler: CvHandler<T>,
    query: String,
    authority: AuthorityId,
}

impl<T: JoinSemilattice + Clone> MaterializedView<T> {
    pub fn subscribe(&self) -> Dynamic<T> {
        Dynamic::from_cv_handler(&self.handler)
    }

    pub fn apply_facts(&mut self, new_facts: &[Fact]) {
        let query_result = execute_query(&self.query, new_facts);
        self.handler.on_recv(StateMsg::new(query_result));
    }
}
```

Views cache query results and update incrementally via the CRDT merge operation. Cross-authority view sync uses delta handlers.

## 6. Guard Chain Integration

Database operations flow through the existing guard chain.

```mermaid
flowchart LR
    A[Query Request] --> B[CapGuard];
    B --> C[FlowGuard];
    C --> D[JournalCoupler];
    D --> E[DatabaseEffects];
```

The `CapGuard` evaluates Biscuit token authorization. The `FlowGuard` charges the budget for query cost. The `JournalCoupler` logs the query execution. The `DatabaseEffects` handler executes the query.

Each guard must succeed before the next executes. Any failure returns locally with no observable side effect.

## 7. Effect Traits

### 7.1 Database Effects

The `DatabaseQueryEffects` trait extends `JournalEffects` with query capabilities.

```rust
#[async_trait]
pub trait DatabaseQueryEffects: JournalEffects + Send + Sync {
    async fn query_local(&self, ctx: &EffectContext, query: &str) -> AuraResult<QueryResult>;
    async fn query_federated(
        &self,
        ctx: &EffectContext,
        query: &str,
        scope: QueryScope,
    ) -> AuraResult<QueryResult>;
    async fn subscribe_query<T: FromQueryResult>(
        &self,
        ctx: &EffectContext,
        query: &str,
    ) -> AuraResult<Dynamic<T>>;
}
```

Local queries execute against a single authority's facts. Federated queries coordinate across multiple authorities using choreography.

### 7.2 Production Handler

The production handler combines existing infrastructure.

```rust
pub struct ProductionDatabaseEffects {
    journal: Arc<dyn JournalEffects>,
    index: AuthorityIndex,
    biscuit_bridge: BiscuitAuthorizationBridge,
    crdt_coordinator: CrdtCoordinator,
}
```

The journal provides fact storage. The index provides efficient lookups. The Biscuit bridge provides authorization. The CRDT coordinator provides replication.

## 8. Federation

### 8.1 Federated Queries

Cross-authority queries use existing choreography infrastructure. The initiator collects authorization tokens. Queries execute in parallel across authorities. Results merge using `JoinSemilattice`.

```rust
let merged = partial_results.into_iter()
    .fold(QueryResult::bottom(), |acc, r| acc.join(&r));
```

The merge operation guarantees consistent results regardless of response order.

### 8.2 Effect Delegation

Delegation uses Biscuit token attenuation.

```rust
let delegation = account_authority
    .attenuate_for_query("documents() where can_read($auth, ?doc)")
    .with_time_limit(Duration::from_secs(300))
    .with_result_limit(100)?;
```

Delegated tokens can only narrow capabilities. Time limits and result limits provide additional constraints.

## 9. Implementation Location

The indexing layer lives in `aura-effects/src/database/`. The query wrapper lives in the same location. The subscription API extends `JournalEffects` from `aura-journal`.

The `IndexedJournalEffects` trait is defined in `aura-core/src/effects/indexed_journal.rs`. Production handlers implement this trait in the effects crate.

## See Also

[Journal System](102_journal.md) describes fact storage and reduction. [Authorization](109_authorization.md) covers Biscuit token evaluation. [Effect System and Runtime](106_effect_system_and_runtime.md) details effect implementation patterns.
