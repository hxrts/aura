# Social Architecture

This document defines Aura's social organization model using a digital urban metaphor. The system layers social organization, privacy, consent, and governance into three tiers: messages, blocks, and neighborhoods.

## 1. Overview

### 1.1 Design Goals

The model produces human-scaled social structures with natural scarcity based on physical analogs. Organic community dynamics emerge from bottom-up governance. The design aligns with Aura's consent-based privacy guarantees and capability-based authorization.

### 1.2 Three-Tier Structure

Messages are communication contexts. Direct messages are private relational contexts. Block messages are semi-public messaging for block residents.

Blocks are semi-public communities capped by storage constraints. Each block has a 10 MB total allocation. Residents allocate storage to participate.

Neighborhoods are collections of blocks connected via adjacency and traversal policies. Blocks donate storage to neighborhood infrastructure.

### 1.3 Terminology

An authority (`AuthorityId`) is the cryptographic identity that holds capabilities and participates in consensus. A petname is a local mapping from an authority to a human-understandable name. Each device maintains its own petname mappings. There is no global username registry.

A contact suggestion is metadata an authority optionally shares when connecting with someone. Users configure a default suggestion sent to all new connections. Users can share different suggestions with different people or opt out entirely.

## 2. Message Types

### 2.1 Direct Messages

Direct messages are small private relational contexts built on AMP. There is no public frontage or traversal. All participants must be explicitly added. New members do not receive historical message sync.

### 2.2 Block Messages

Block messages are semi-public messaging for block residents. They use the same AMP infrastructure as direct messages. When a new resident joins, current members send a window of recent messages.

Membership is tied to block residency. Leaving the block revokes access. Multiple channels may exist per block for different purposes.

```rust
pub struct BlockMessage {
    block_id: BlockId,
    channel: String,
    content: Vec<u8>,
    author: AuthorityId,
    timestamp: TimeStamp,
}
```

The message structure identifies the block, channel, content, author, and timestamp. Historical sync is configurable, typically the last 500 messages.

## 3. Block Architecture

### 3.1 Block Structure

A block is a relational context with its own journal. The total storage allocation is 10 MB. Capability templates define visitor, frontage, guest, resident, and steward patterns. Local governance is encoded via policy facts.

```rust
pub struct Block {
    block_id: BlockId,
    storage_limit: u64,
    residents: Vec<AuthorityId>,
    stewards: Vec<AuthorityId>,
    channels: Vec<String>,
}
```

The block structure contains the identifier, storage limit, resident list, steward list, and message channels.

### 3.2 Residency

Block membership derives from possessing capability bundles, meeting entry requirements defined by policy, and allocating resident-specific storage. In v1, each user belongs to exactly one block.

Joining a block follows a defined sequence. The authority requests capability. Block governance approves using local policy via Biscuit evaluation and consensus. The authority accepts the capability bundle and allocates storage. Historical block messages sync from current residents.

### 3.3 Stewardship

Stewards emerge via governance decisions in the block. Steward capability bundles include moderation, pin and unpin operations, and governance facilitation. Stewardship is auditable because capability issuance is visible via relational facts.

## 4. Neighborhood Architecture

### 4.1 Neighborhood Structure

A neighborhood is a relational context linking multiple blocks. It contains a combined pinned infrastructure pool equal to the number of blocks times 1 MB. An adjacency graph connects blocks. Traversal and inter-block policy logic define movement rules.

```rust
pub struct Neighborhood {
    neighborhood_id: NeighborhoodId,
    blocks: Vec<BlockId>,
    adjacency: Vec<(BlockId, BlockId)>,
}
```

The neighborhood structure contains the identifier, member blocks, and adjacency edges.

### 4.2 Block Membership

Blocks donate 1 MB of their budget per neighborhood joined. In v1, each block may join a maximum of 4 neighborhoods. This limits adjacency graph complexity and effect delegation routing.

## 5. Position and Traversal

### 5.1 Position Structure

Position is represented as a structured type.

```rust
pub struct TraversalPosition {
    neighborhood: Option<NeighborhoodId>,
    block: Option<BlockId>,
    depth: TraversalDepth,
    capabilities: BiscuitToken,
    entered_at: TimeStamp,
}

pub enum TraversalDepth {
    Street,
    Frontage,
    Interior,
}
```

The position tracks current neighborhood, current block, traversal depth, capabilities, and entry time. Street depth allows seeing frontage with no interior access. Frontage depth allows limited interaction. Interior depth provides full resident-level access.

### 5.2 Movement Rules

Movement is possible when a Biscuit capability authorizes entry, neighborhood policy allows traversal along an adjacency edge, and block frontage or invitations allow deeper entry. Traversal does not replicate pinned data. Visitors operate on ephemeral local state.

Traversal does not reveal global identity. Only contextual identities within encountered blocks are visible.

## 6. Storage Constraints

### 6.1 Block-Level Allocation

Blocks have a fixed size of 10 MB total. Allocation depends on neighborhood participation.

| Neighborhoods | Donation | Resident Storage | Public Space |
|---------------|----------|------------------|--------------|
| 1             | 1.0 MB   | 1.6 MB           | 7.4 MB       |
| 2             | 2.0 MB   | 1.6 MB           | 6.4 MB       |
| 3             | 3.0 MB   | 1.6 MB           | 5.4 MB       |
| 4             | 4.0 MB   | 1.6 MB           | 4.4 MB       |

More neighborhood connections mean less local storage for block culture. This creates meaningful trade-offs.

### 6.2 Flow Budget Integration

Storage constraints are enforced via the flow budget system.

```rust
pub struct BlockFlowBudget {
    block_id: BlockId,
    resident_storage_spent: u64,
    pinned_storage_spent: u64,
    neighborhood_donations: u64,
}
```

The spent counters are persisted as journal facts. Limits are derived at runtime from block policy and Biscuit capabilities. Resident storage limit is 1.6 MB for 8 residents at 200 KB each.

## 7. Fact Schema

### 7.1 Block Facts

Block facts enable Datalog queries.

```datalog
block(block_id, created_at, storage_limit).
block_config(block_id, max_residents, neighborhood_limit).
resident(authority_id, block_id, joined_at, storage_allocated).
steward(authority_id, block_id, granted_at, capabilities).
pinned_content(content_hash, block_id, pinned_by, pinned_at, size_bytes).
```

These facts express block existence, configuration, residency, stewardship, and pinned content.

### 7.2 Neighborhood Facts

Neighborhood facts express neighborhood existence, block membership, adjacency, and traversal permissions.

```datalog
neighborhood(neighborhood_id, created_at).
block_member(block_id, neighborhood_id, joined_at, donated_storage).
adjacent(block_a, block_b, neighborhood_id).
traversal_allowed(from_block, to_block, capability_requirement).
```

### 7.3 Query Examples

Queries use Biscuit Datalog.

```datalog
residents_of(Block) <- resident(Auth, Block, _, _).

visitable(Target) <-
    resident(Me, Current, _, _),
    adjacent(Current, Target, _),
    traversal_allowed(Current, Target, Cap),
    has_capability(Me, Cap).
```

The first query finds all residents of a block. The second finds blocks a user can visit from their current position.

## 8. IRC-Style Commands

### 8.1 User Commands

User commands are available to all participants.

| Command | Description | Capability |
|---------|-------------|------------|
| `/msg <user> <text>` | Send private message | `send_dm` |
| `/me <action>` | Send action | `send_message` |
| `/nick <name>` | Update contact suggestion | `update_contact` |
| `/who` | List participants | `view_members` |
| `/leave` | Leave current context | `leave_context` |

### 8.2 Moderator Commands

Moderator commands require steward capabilities.

| Command | Description | Capability |
|---------|-------------|------------|
| `/kick <user>` | Remove from block | `moderate:kick` |
| `/ban <user>` | Ban from block | `moderate:ban` |
| `/mute <user>` | Silence user | `moderate:mute` |
| `/pin <msg>` | Pin message | `pin_content` |

### 8.3 Command Execution

Commands execute through the guard chain.

```mermaid
flowchart LR
    A[Parse Command] --> B[CapGuard];
    B --> C[FlowGuard];
    C --> D[JournalCoupler];
    D --> E[TransportEffects];
```

The command is parsed into a structured type. CapGuard checks capability requirements. FlowGuard charges the moderation action budget. JournalCoupler commits the action fact. TransportEffects notifies affected parties.

## 9. Governance

### 9.1 Block Governance

Blocks govern themselves through capability issuance, consensus-based decisions, stewardship roles, and moderation. Block governance uses Aura Consensus for irreversible or collective decisions.

### 9.2 Neighborhood Governance

Neighborhoods govern block admission, adjacency graph maintenance, traversal rules, and shared civic norms. High-stakes actions use Aura Consensus.

## 10. Privacy Model

### 10.1 Contextual Identity

Identity in Aura is contextual and relational. Joining a block reveals a block-scoped identity. Leaving a block causes that contextual identity to disappear. Profile data shared inside a context stays local to that context.

### 10.2 Consent Model

Disclosure is consensual. The device issues a join request. Block governance approves using local policy. The authority accepts the capability bundle. This sequence ensures all participation is explicit.

## 11. V1 Constraints

For the initial release, the model is simplified with three constraints.

Each user resides in exactly one block. This eliminates multi-residency complexity and allows core infrastructure to stabilize.

Each block has a maximum of 8 residents. This human-scale limit enables strong community bonds and manageable governance.

Each block may join a maximum of 4 neighborhoods. This limits adjacency graph complexity and effect delegation routing overhead.

## 12. Infrastructure Roles

Blocks and neighborhoods provide infrastructure services beyond social organization. The `aura-social` crate implements these roles through materialized views and relay selection.

### 12.1 Block Infrastructure

Blocks provide data availability and relay services for residents:

- **Data Replication**: Block residents replicate pinned data across available devices. The `BlockAvailability` type coordinates replication factor and failover.
- **Message Relay**: Block peers serve as first-hop relays for unknown destinations. The `SocialTopology::block_peers()` method returns available relays.
- **Storage Coordination**: The `StorageService` enforces storage budgets per resident and tracks usage facts.

### 12.2 Neighborhood Infrastructure

Neighborhoods enable multi-hop routing and cross-block coordination:

- **Descriptor Propagation**: Neighborhood adjacency edges define descriptor propagation paths. Adjacent blocks exchange routing information.
- **Traversal Capabilities**: `TraversalAllowedFact` grants movement between blocks. Traversal depth limits constrain routing overhead.
- **Multi-Hop Relay**: When block-level relay fails, neighborhood traversal provides alternate paths.

### 12.3 Progressive Discovery Layers

The `aura-social` crate implements a four-layer discovery model:

| Layer | Priority | Resources Required | Flow Cost |
|-------|----------|-------------------|-----------|
| Direct | 0 | Known peer relationship | Minimal |
| Block | 1 | Block peers available | Low |
| Neighborhood | 2 | Neighborhood traversal | Medium |
| Rendezvous | 3 | Global flooding | High |

Discovery layer selection uses `SocialTopology::discovery_layer()`:

```rust
let topology = SocialTopology::new(local_authority, block, neighborhoods);
let layer = topology.discovery_layer(&target);
```

Lower priority layers are preferred when available. This creates economic incentives to establish social relationships before communication.

### 12.4 Relay Selection

The `RelayCandidateBuilder` generates relay candidates based on social topology:

```rust
let builder = RelayCandidateBuilder::from_topology(topology);
let candidates = builder.build_candidates(&context, &reachability);
```

Candidates are returned in priority order: block peers first, then neighborhood peers, then guardians. Reachability checks filter unreachable peers.

## See Also

[Database Architecture](113_database.md) describes fact storage and queries. [Transport and Information Flow](108_transport_and_information_flow.md) covers AMP messaging. [Authorization](109_authorization.md) describes capability evaluation. [Rendezvous Architecture](110_rendezvous.md) details the four-layer discovery model integration.
