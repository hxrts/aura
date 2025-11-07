# Distributed Key-Value Store with Neighborhood Visibility

**Status:** Brainstorming/Proposal

This document outlines a design for a distributed, peer-to-peer key-value store built on the Aura architecture. It leverages the existing primitives of the Unified Journal (CRDT), Social Bulletin Board (SBB), and Keyhive capabilities to create a system for storing and sharing "semi-private" data within a user's social graph.

## 1. Core Concepts & Building Blocks

This design synthesizes several powerful primitives already specified in the Aura documentation:

-   **Unified CRDT Journal:** A single, eventually consistent source of truth for all system state, including identity, storage metadata, and capabilities. This eliminates complex state synchronization issues.
-   **Content-Addressable Storage:** The existing storage layer (`040_storage.md`) is built on Content Identifiers (CIDs), providing data integrity and a natural keying mechanism for our K/V store.
-   **Keyhive Convergent Capabilities:** A CRDT-native authorization system (`120_keyhive_integration.md`) that allows for fine-grained, delegable, and revocable access control without a central authority.
-   **Social Bulletin Board (SBB):** A peer discovery mechanism (`041_rendezvous.md`) that establishes a web-of-trust social graph. This graph is the foundation for defining "neighborhoods."
-   **Proxy Re-Encryption (PRE):** A cryptographic method (mentioned as a future enhancement) that allows a proxy to transform a ciphertext from one user's key to another's, without the proxy being able to decrypt the content itself.

## 2. The Distributed Key-Value Store

The fundamental K/V store is an extension of the existing storage specification.

-   **Logical Key:** Applications continue to address records by human-meaningful keys (e.g., `"profile:alice"`). We model this using a CRDT map called the **NeighborhoodMap**.
-   **Physical Key:** Each write produces a new `Cid` for the stored value. This is content-addressed, ensuring that the key always corresponds to a specific version of the data.
-   **Value:** The "value" is the combination of the `ObjectManifest` (metadata) and its associated encrypted data `Chunk(s)`.
-   **Operations:** The basic `put` and `get` operations are handled by the `store_encrypted` and `fetch_encrypted` APIs.

The NeighborhoodMap is an add-wins map replicated through the Unified Journal. Each logical key maps to the current head CID plus lineage metadata:

```rust
struct NeighborhoodMapEntry {
    current_cid: Cid,
    previous_cids: Vec<Cid>,
    tombstoned: bool,
    last_writer: DeviceId,
    lamport_clock: u64,
}
```

`put` operations append a new manifest/chunk set to storage, then emit a `NeighborhoodMapUpdated` journal event that moves the logical key to the new `Cid`. If two writers race, the CRDT resolves according to `(lamport_clock, last_writer)` ordering, and the losing write remains in `previous_cids` for auditability. Deletes emit a `NeighborhoodMapTombstoned` event so readers can detect tombstones without downloading stale chunks.

Reads consult the NeighborhoodMap first to obtain the authoritative head CID (ignoring entries flagged `tombstoned`). Clients may optionally fall back to `previous_cids` to support version history or rollback UI flows.

### 2.1. Socially-Aware Replication Policies

```rust
// Example of a socially-aware replication policy
enum NeighborhoodReplicationPolicy {
    // Replicate to N of my direct friends (1-hop)
    Friends { count: u32 },
    // Replicate to N peers within H hops in the trust graph
    Hops { hops: u8, count: u32 },
    // Replicate to peers belonging to a specific private DAO/group
    Group { group_id: String, count: u32 },
}
```

When a user `put`s a value with such a policy, their agent runs the **Replication Placement Choreography**:

1.  Rank candidate peers from the SBB graph using configurable heuristics (latency, trust score, available storage, historical uptime).
2.  Propose placements to the top-ranked peers and require a signed `ReplicaAccepted` acknowledgement that includes the target CID, policy hash, and an expiry height.
3.  Complete the placement only after the owner verifies a `ProofOfStorage` challenge for each acknowledgement.
4.  Schedule periodic health checks; if a replica stops answering challenges, the agent selects a replacement peer and emits a `ReplicaRetired` journal event.

This ensures the `count` in each policy reflects confirmed replicas rather than best-effort pushes.

### 2.2. Consistency Guarantees (Jepsen Models)

We describe correctness using the register and transactional models defined in the [Jepsen consistency taxonomy](https://jepsen.io/consistency/models).

*   **Per-Key Model:** Each logical key behaves as a multi-value register whose states are merged by an add-wins CRDT. Projected into Jepsen's register formalism, this yields a **last-write-wins register**: replicas may observe different prefixes of the log, but once all messages are delivered they converge to the write with the greatest `(lamport_clock, last_writer)` pair. This satisfies *Eventual Consistency (EC)*.
*   **Session Guarantees:** Because a device applies its own journal events locally before gossiping them, every client session enjoys *Read-Your-Writes (RYW)* and *Monotonic Reads (MR)*. The system does **not** guarantee *Monotonic Writes* or *Writes-Follow-Reads* across independent sessions without additional choreography.
*   **Lack of Stronger Guarantees:** Outside the optional transactional path, the store is neither *Sequentially Consistent* nor *Linearizable*; caches may briefly serve stale data until they receive the updated NeighborhoodMap entry.

These properties apply to both direct reads and responses served from neighborhood caches, provided the caches replay the CRDT journal in order.

## 3. Visibility & Access Control: "Semi-Private" Data

The core challenge is managing the visibility of data that is neither fully public nor fully private. This is solved by combining encryption with a flexible, decentralized authorization model.

1.  **Default to Private:** All data is, by default, encrypted with a key derived from the owner's identity. Only the owner and their devices can read it.
2.  **Granting Neighborhood Access:** To make data "semi-private," the owner delegates a **capability** to a specific audience. This capability grants `read` permission for the data's `Cid`.
3.  **Defining the "Neighborhood":** The audience for the capability *is* the neighborhood.
    *   **1-Hop (Friends):** The owner delegates the read capability to the `IndividualId` of each of their direct contacts.
    *   **N-Hops:** The owner delegates a `read` capability *and* a limited `delegate` capability to their 1-hop friends. This allows them to further delegate the read-only capability to their own friends (the owner's 2-hop neighborhood). The delegation chain within the `CapabilityToken` carries a `max_hops` counter that is decremented at each hop; once it reaches zero, further delegation fails.
    *   **Private Groups:** The owner delegates the read capability to a group's threshold identity, making it accessible to all group members.

4.  **Delegation Journal:** Every delegation and re-delegation is logged as a `CapabilityDelegated` event, creating an immutable audit trail that the owner (or automation) can traverse.
5.  **Revocation:** Access is revoked by revoking the delegated capability in the owner's CRDT journal. Revocation removes the branch from the delegation tree and publishes a `CapabilityRevoked` event that references the parent capability ID. The `VisibilityIndex` ensures the change propagates, and peers will no longer be able to access the data once the revocation epoch surpasses their local watermark. Capabilities also include an absolute expiry, so forgotten grants lapse automatically.

## 4. Proxy Re-Encryption for Neighborhoods

To make neighborhood sharing more efficient and asynchronous, we can use Proxy Re-Encryption (PRE), as hinted at in the documentation. This avoids the need for the owner to be online to grant access to new members of the neighborhood.

**Workflow:**

1.  **Owner Encrypts Once:** Alice encrypts her data with her own key, `PK_A`, creating a single canonical ciphertext.
2.  **Designate Proxies:** Alice designates her direct friends (e.g., Bob) as proxies.
3.  **Threshold Re-Encryption Keys:** Alice uses a threshold PRE scheme (like `rust-umbral`). She decides on a policy (e.g., "2 of my 5 friends must approve"). She generates 5 re-encryption key **fragments** and distributes one to each of the 5 friend-proxies. These fragments do *not* allow the proxies to decrypt the data and are wrapped under device-local hardware keys. Each fragment is versioned and bound to the capability it supports.
4.  **Neighbor Discovers & Requests:** Carol (a 2-hop neighbor) discovers the data's CID. She sees Alice is the owner and finds they have 2 mutual friends who are proxies. She sends requests to them, providing her public key, `PK_C`.
5.  **Proxies Generate Re-Encryption Shares:** The 2 friend-proxies use their key fragments to generate "re-encryption shares," but only if the request includes a still-valid capability reference and falls within the fragment's `not_after` timestamp. They send these shares to Carol.
6.  **Neighbor Decrypts:** Carol collects the 2 shares, combines them, and can now either transform the original ciphertext into one she can decrypt, or directly decrypt the original ciphertext. If she cannot assemble quorum within the timeout, she retries with alternate proxies pulled from the neighborhood roster.

**Advantages of PRE:**

-   **Owner Offline:** Alice can be offline while her neighborhood continues to get access to the data she shared.
-   **Zero-Knowledge Proxies:** The friends acting as proxies cannot read the data they are re-encrypting.
-   **Decentralized Trust:** Threshold PRE prevents a single proxy from having sole power over access, and revoked proxies publish `ReencryptionFragmentRevoked` events so the owner rotates fragments proactively.
-   **Storage Efficiency:** The data is only stored once, encrypted for the owner.
-   **Adaptive Liveness:** The owner monitors proxy health via lightweight heartbeats; if too many proxies are offline, she updates the threshold policy (e.g., 2-of-3) and republishes fresh fragments so the neighborhood stays responsive.

Fragments are checkpointed to the owner's durable devices, sealed under the account recovery key. On device replacement, the agent restores the fragment set and publishes a `ReencryptionFragmentRotated` event so proxies re-fetch the latest material. Revoked devices lose access because fragment renewal requires fresh capability proofs.

## 5. Distributed Indexing and Discovery

To find semi-private data without knowing its CID, we need a distributed index.

1.  **Index Entry Creation:** When an owner shares a piece of data with a neighborhood, their device also creates a compact `IndexEntry` message. The payload is encrypted under the neighborhood session key negotiated via Keyhive and contains:
    *   The `Cid` of the `ObjectManifest`.
    *   A Bloom filter or hashed tags derived from the public, searchable `app_metadata` (e.g., hash(`#aura`), hash(`post`)).
    *   The `CapabilityScope` required for access.
    *   An entry expiry timestamp so stale references age out naturally.

2.  **Index Distribution via SBB:** The encrypted `IndexEntry` is gossiped via the SBB's envelope-flooding mechanism. The `rtag` is derived from the neighborhood's shared secret, but the metadata is opaque to outsiders. Nodes rate-limit how many index envelopes they will accept per peer per hour to mitigate scraping.

3.  **Local Index Construction:** Every node in the neighborhood receives these entries, decrypts them with the neighborhood key, and builds a local, searchable key-value index (e.g., using `sled`) that maps hashed terms to lists of CIDs alongside their expiry.

**Search Workflow:**

1.  **Local-First:** A user first queries their own local index for instant results.
2.  **Broadcast Query:** To discover new data, the user can broadcast a "search query" envelope to the neighborhood via the SBB.
3.  **Peer Response:** Peers receive the query, run it against their local index, and return matching CIDs directly to the querier, attaching fresh capability proofs so the requester can validate authorization before attempting a fetch.

## 6. Caching and Replication: A Two-Tiered Approach

A resilient and performant distributed system requires a clear strategy for both data safety and fast access. In Aura's design, we treat replication and caching as two distinct but complementary mechanisms that serve different goals.

**The Guiding Principle:**

-   **Replication is for the data owner.** Its purpose is **durability and data preservation**. It is an explicit, "push"-based action to ensure the canonical data is safe.
-   **Caching is for the data consumer.** Its purpose is **performance and data availability**. It is an implicit, "pull"-based side effect of data being accessed.

### Replication: Ensuring Durability

Replication is the process of creating and maintaining a specific number of copies of the encrypted data chunks at designated, trusted locations.

-   **Who:** The **data owner** is in control.
-   **Why:** To ensure the data is not lost if the owner's primary device fails or goes offline permanently. It is a conscious backup strategy.
-   **How:** The owner defines a `ReplicationHint` in the `ObjectManifest`. This policy dictates where the data should be copied, for example, to a static list of the owner's other devices or to a dynamic set of peers based on social trust (e.g., "3 of my guardians"). Replication is considered complete only after the target peers return signed `ReplicaAccepted` receipts and the owner verifies a `ProofOfStorage` round for each.
-   **What:** The core subject of replication is the **encrypted data chunks**. The metadata (`ObjectManifest`) is small and gossiped more widely anyway.
-   **Lifecycle:** Replicas are meant to be long-lived and "pinned." They should not be deleted unless the owner changes the replication policy or deletes the data. The `Proof-of-Storage` challenge protocol is used to periodically verify that these designated replicas are still holding the data.

### Caching: Improving Performance & Availability

Caching is the process of keeping a temporary, local copy of data that a node has recently accessed.

-   **Who:** Any **node that accesses data** can become a cache.
-   **Why:**
    1.  **Performance:** If Bob views a photo, caching it on his device means the next time he views it, it loads instantly from local storage instead of being fetched over the network.
    2.  **Availability:** If Alice posts a photo and then goes offline, Bob (who has it cached) can serve it directly to Carol, who also wants to see it. Carol doesn't have to wait for Alice to come back online. This is especially powerful in the neighborhood model.
-   **How:** This is an opportunistic and automatic process. When a node retrieves data chunks to display to its user, it can decide to keep those chunks in a local cache managed by an eviction policy (like Least Recently Used). When the cache is full, the oldest items are deleted.
-   **What:** Caching typically applies to both data chunks and their associated metadata/manifests.
-   **Lifecycle:** Cached copies are ephemeral and can be deleted at any time by the caching node to manage its own storage space.

### Summary: Replication vs. Caching

| Feature          | Replication                                       | Caching                                                  |
| :--------------- | :------------------------------------------------ | :------------------------------------------------------- |
| **Primary Goal** | **Durability** (Don't lose the data)               | **Performance** (Get the data fast) & **Availability**   |
| **Initiator**    | Data Owner (Push-based)                           | Data Consumer (Pull-based)                               |
| **Control**      | Explicit, Policy-driven                           | Implicit, Opportunistic                                  |
| **Data Stored**  | Encrypted Chunks                                  | Encrypted Chunks & Metadata                              |
| **Lifetime**     | Long-term, "pinned"                               | Temporary, subject to eviction (LRU)                     |
| **Example**      | Alice backs up her encrypted diary to 3 guardians. | A popular photo is temporarily held on many friends' devices after they view it. |

### Putting It All Together: A Workflow Example

Imagine Alice posts a photo to a group:

1.  **Replication (Durability):** Alice's `put` operation includes a policy to replicate the photo to her home server. Her agent pushes the encrypted chunks there. Now, even if her phone is lost, the photo is safe.
2.  **Access & Caching (Performance):** Bob, a group member, views the photo. His device fetches the chunks from Alice's home server and **caches** them locally. The next time he opens the chat, the photo loads instantly.
3.  **Social Caching (Availability):** Carol, another group member, comes online later. Alice is offline. Carol's device asks the neighborhood for the photo's CID. Bob's device sees the request and serves the chunks directly from its cache. Carol gets the photo without ever needing to connect to Alice.
4.  **Eviction:** After a few weeks, Bob hasn't looked at the photo. His device is running low on space. The LRU eviction policy sees the photo is old and purges it from the cache to make room for new content. The replicated copies on Alice's devices are unaffected.

## 7. Achieving ACID-like Guarantees (Optional Transactions)

Aura's architecture is fundamentally a **BASE** (Basically Available, Soft state, Eventually consistent) system, prioritizing availability and partition tolerance. This is the correct choice for a decentralized, local-first network.

However, for specific high-stakes operations we can layer ACID-like transactional guarantees on top of the BASE foundation. This is **opt-in** for operations that require stronger consistency.

Within Jepsen's taxonomy, the combination of the threshold-locked critical section and two-phase commit yields **Serializability**: every committed transaction is equivalent to some total order consistent with commit events. Because commits propagate asynchronously, the system does *not* offer **Strict Serializability** or **Linearizability**—observers may not see a freshly committed transaction until its journal entry arrives.

### Atomicity (All or Nothing)

We can achieve atomicity using a **Two-Phase Commit (2PC)** pattern within the CRDT journal.

1.  **Propose:** A transaction begins with a `ProposeTransaction` event written to the journal. This event contains a unique transaction ID and outlines the intended operations.
2.  **Log:** Participants log their parts of the transaction as events (e.g., `LogDebit`, `LogCredit`), all tagged with the transaction ID. The actual state is **not yet modified**.
3.  **Commit:** The transaction is finalized with a `CommitTransaction` event, which **requires a threshold signature** from the participants.
4.  **Materialize:** Only when a client sees the threshold-signed `CommitTransaction` event does it apply the preceding logged events to its view of the state. If the commit is missing or an `AbortTransaction` event appears, the logged events are ignored.

This ensures the entire transaction appears to happen atomically once the commit is finalized.

### Consistency (Valid State)

The database state must always be valid. Aura's architecture already provides strong tools for this.

1.  **CRDT Convergence:** The underlying CRDT ensures that all nodes will eventually converge to the same, valid state.
2.  **Event Validation:** The `apply_event` logic in the journal can enforce application-specific invariants (e.g., an account cannot be debited below zero). Events that would violate these invariants are rejected.
3.  **Capability System:** Keyhive ensures that only authorized actors can even propose certain state changes, preventing invalid states from being created by unauthorized parties.

### Isolation (Concurrent Transactions Don't Interfere)

This is the most difficult property in a distributed system. For operations that require true isolation, we must trade some availability and use the **distributed locking protocol**.

1.  **Acquire Lock:** Before starting a transaction on a contentious resource (e.g., a shared wallet), the initiator must run the locking choreography to acquire a threshold-signed `OperationLock`.
2.  **Exclusive Access:** The deterministic lottery ensures only one participant can hold the lock for a given resource at a time. All other attempts will fail until the lock is released.
3.  **Perform Transaction:** The lock holder can now safely execute their atomic transaction using the 2PC pattern described above.
4.  **Release Lock:** Upon commit or abort, a `ReleaseOperationLock` event is written, allowing others to contend for the resource.

This provides serializable isolation for critical sections at the cost of liveness, and should be used sparingly.

### Durability (Committed Data is Permanent)

Once a transaction is committed, it must not be lost.

1.  **CRDT Journal:** A transaction is considered "committed" locally as soon as the `CommitTransaction` event is written to the journal.
2.  **Replication:** Durability is achieved by replicating the CRDT journal to multiple peers, as defined by the data's `ReplicationHint`. An application can consider a transaction fully durable once it has received acknowledgements that the commit event has been replicated to a sufficient number of peers.
3.  **Proof-of-Storage:** The challenge-response protocol can be used to periodically verify that replicas of the journal are being maintained over the long term.

## 8. Query Language and Data Discovery

The choice of a query language must align with the database's decentralized, eventually-consistent, and security-focused architecture. Traditional languages like SQL are a poor fit as they assume a structured schema and strong consistency.

Instead, we propose a hybrid approach: a simple, programmatic API for common queries, powered by a sophisticated Datalog engine for security and expressiveness.

### Primary Interface: A Simple Document/K-V Query API

For most developers, the primary interaction with the database will be through a simple, programmatic API that filters document metadata. This aligns perfectly with the local-first indexing model.

**Example: Programmatic Query API**

```rust
let query = QueryBuilder::new()
    .with_tag("#aura")
    .with_field("type", "post")
    .with_scope(Visibility::Neighborhood(2)); // Search up to 2 hops

let results: Vec<Cid> = agent.db_query(query).await?;
```

This API is intuitive and handles the two-stage process of querying the local index first, then broadcasting the query to the neighborhood if necessary.

### Underlying Engine: Datalog

The true power of the system comes from using **Datalog** as the underlying query engine. Datalog, a declarative logic programming language, is exceptionally well-suited for Aura:

1.  **Graph-Oriented:** It excels at the kind of recursive, graph-traversal queries needed to navigate the SBB web-of-trust ("friends of friends") and the Keyhive capability delegation graph.
2.  **Unifies Data and Policy:** It allows us to express rules about both data and permissions in the same language. A query can seamlessly check both the data index and the capability graph, making it impossible for a query to return data the user is not authorized to see.
3.  **Decentralization-Friendly:** Datalog rules can be evaluated on local, partial sets of data, which is a perfect match for our eventually-consistent, local-first model.

**Recommended Rust Implementation:** The **Datafrog** library is the ideal choice for this system. Datafrog is a lightweight Datalog engine written in pure Rust that compiles to WebAssembly, making it perfect for client-side execution in browsers and edge environments. Its ability to perform **incremental computation** is a perfect match for our CRDT-based journal. As new events arrive, we can feed them to Datafrog as fact *deltas*, and it will efficiently update query results without re-computing everything from scratch. Unlike DDlog which requires compilation to platform-specific binaries, Datafrog's WASM compatibility ensures Aura can run anywhere Rust can compile.

### Implementation Strategy

1.  **Phase 1: Engine Integration & Fact Generation:** Integrate the Datafrog library. Implement a "projection" layer that reads the `UnifiedAccountLedger` CRDT state and translates it into the initial set of Datalog facts as Datafrog relations (e.g., `friend(A, B).`, `data(Cid, Tag, Value).`, `has_capability(User, Op, Resource).`).

2.  **Phase 2: Rule Definition:** Define the core logic of the system using Datafrog's Rust API. This includes rules for neighborhood traversal, capability delegation, and access rights. Unlike DDlog which uses external `.dl` files, Datafrog rules are written directly in Rust using its join and iteration primitives.

3.  **Phase 3: Query API Integration:** Implement the simple `QueryBuilder` API. This layer will translate the developer-friendly query object into Datafrog relation joins and filters. A crucial feature is that this layer will **always** inject the `can_read("my_id", Cid)` constraint into the query, ensuring all results are filtered by permissions at the engine level.

4.  **Phase 4: Incremental Updates:** Hook into the CRDT journal's event stream. As new events arrive, translate them into fact deltas and use Datafrog's incremental computation capabilities to keep the materialized query views constantly up-to-date without full re-computation.

### Example Datalog Query

A query to find all images tagged `#aura` within a user's 2-hop neighborhood that they are allowed to see would be expressed like this:

```datalog
// Rules defining access and relationships
friend_of_friend(A, C) :- friend(A, B), friend(B, C).
can_read(User, Cid) :- has_direct_capability(User, "read", Cid).
can_read(User, Cid) :- friend(User, Friend), has_delegated_capability(Friend, "read", Cid).

// The final query
find_images(Cid) :-
    data(Cid, "type", "image"),
    data(Cid, "tag", "#aura"),
    (friend("my_id", Owner) ; friend_of_friend("my_id", Owner)),
    owner(Cid, Owner),
    can_read("my_id", Cid).
```

This hybrid approach provides a simple API for common cases while leveraging a powerful, security-aware Datalog engine that is perfectly suited to the decentralized and graph-like nature of Aura's data.

## 9. References

- [Unified Storage Specification](040_storage.md)
- [Rendezvous & Social Bulletin Board (SBB)](051_rendezvous.md)
- [Keyhive Integration for Authorization and Group Key Agreement](120_keyhive_integration.md)
- [Garbage Collection and State Compaction](401_garbage_collection.md)

## 10. Glossary

-   **Linearizability:** Each operation appears to take effect atomically at some point between its invocation and response, preserving real-time ordering across clients.
-   **Strict Serializability:** Serializability that also respects real-time ordering, equivalent to linearizability for transactions.
-   **Serializability:** The outcome of concurrent transactions is indistinguishable from some sequential execution that preserves transaction semantics.
-   **Eventual Consistency:** All replicas that stop receiving new updates eventually converge to the same state.
-   **Read-Your-Writes (RYW):** A client always observes its own completed writes in subsequent reads within the same session.
-   **Monotonic Reads (MR):** Once a client has seen a particular value for a key, it will never see an older value for that key in the same session.
