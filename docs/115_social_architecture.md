# Social Architecture

This document defines Aura's social organization model using a digital urban metaphor. The system layers social organization, privacy, consent, and governance into three tiers: messages, homes, and neighborhoods.

## 1. Overview

### 1.1 Design Goals

The model produces human-scaled social structures with natural scarcity based on physical analogs. Organic community dynamics emerge from bottom-up governance. The design aligns with Aura's consent-based privacy guarantees and capability-based authorization.

### 1.2 Three-Tier Structure

Messages are communication contexts. Direct messages are private relational contexts. Home messages are semi-public messaging for home members and participants.

Homes are semi-public communities capped by storage constraints. Each home has a 10 MB total allocation. Members and participants allocate storage to participate.

Neighborhoods are authority-level collections of homes connected via 1-hop links and access policies. Homes allocate storage to neighborhood infrastructure.

### 1.3 Terminology

An authority (`AuthorityId`) is the cryptographic identity that holds capabilities and participates in consensus. A nickname is a local mapping from an authority to a human-understandable name. Each device maintains its own nickname mappings. There is no global username registry.

A nickname suggestion (`nickname_suggestion`) is metadata an authority optionally shares when connecting with someone. Users configure a default suggestion sent to all new connections. Users can share different suggestions with different people or opt out entirely.

### 1.4 Unified Naming Pattern

The codebase uses a consistent naming pattern across entities (contacts, devices, discovered peers). The `EffectiveName` trait in `aura-app/src/views/naming.rs` defines the resolution order:

1. **Local nickname** (user-assigned override) if non-empty
2. **Shared nickname_suggestion** (what entity wants to be called) if non-empty
3. **Fallback identifier** (truncated authority/device ID)

This pattern ensures consistent display names across all UI surfaces while respecting both local preferences and shared suggestions.

## 2. Message Types

### 2.1 Direct Messages

Direct messages are small private relational contexts built on AMP. There is no hop-based expansion across homes. All participants must be explicitly added. New members do not receive historical message sync.

### 2.2 Home Messages

Home messages are semi-public messaging for home members and participants. They use the same AMP infrastructure as direct messages. When a new participant joins, current members send a window of recent messages.

Membership and participation are tied to home policy. Leaving the home revokes access. Multiple channels may exist per home for different purposes.

```rust
pub struct HomeMessage {
    home_id: HomeId,
    channel: String,
    content: Vec<u8>,
    author: AuthorityId,
    timestamp: TimeStamp,
}
```

The message structure identifies the home, channel, content, author, and timestamp. Historical sync is configurable, typically the last 500 messages.

## 3. Home Architecture

### 3.1 Home Structure

A home is an authority-scoped context with its own journal. The total storage allocation is 10 MB. Capability templates define limited, partial, full, participant, and moderator patterns. Local governance is encoded via policy facts.

```rust
pub struct Home {
    /// Unique identifier for this home
    pub home_id: HomeId,
    /// Total storage limit in bytes
    pub storage_limit: u64,
    /// Maximum number of participants
    pub max_participants: u8,
    /// Maximum number of neighborhoods this home can join
    pub neighborhood_limit: u8,
    /// Current participants (authority IDs)
    pub participants: Vec<AuthorityId>,
    /// Current moderators with their capabilities
    pub moderators: Vec<(AuthorityId, ModeratorCapabilities)>,
    /// Current storage budget tracking
    pub storage_budget: HomeStorageBudget,
}
```

The home structure contains the identifier, storage limit, configuration limits, participant list, moderator designation list with capabilities, and storage budget tracking.

### 3.2 Membership and Participation

Home participation derives from possessing capability bundles, meeting entry requirements defined by policy, and allocating participant-specific storage. In v1, each user belongs to exactly one home.

Joining a home follows a defined sequence. The authority requests capability. Home governance approves using local policy via Biscuit evaluation and consensus. The authority accepts the capability bundle and allocates storage. Historical home messages sync from current members.

### 3.3 Moderator Designation

Moderators are designated via governance decisions in the home. A moderator must also be a member. Moderator capability bundles include moderation, pin and unpin operations, and governance facilitation. Moderator designation is auditable because capability issuance is visible via relational facts.

## 4. Neighborhood Architecture

### 4.1 Neighborhood Structure

A neighborhood is an authority type linking multiple homes. It contains a combined pinned infrastructure pool equal to the number of homes times 1 MB. A 1-hop link graph connects homes. Access-level and inter-home policy logic define movement rules.

```rust
pub struct Neighborhood {
    /// Unique identifier for this neighborhood
    pub neighborhood_id: NeighborhoodId,
    /// Member homes
    pub member_homes: Vec<HomeId>,
    /// 1-hop links between homes
    pub one_hop_links: Vec<(HomeId, HomeId)>,
}
```

The neighborhood structure contains the identifier, member homes, and 1-hop link edges.

### 4.2 Home Membership

Homes allocate 1 MB of their budget per neighborhood joined. In v1, each home may join a maximum of 4 neighborhoods. This limits 1-hop graph complexity and effect delegation routing.

## 5. Position and Traversal

### 5.1 Discovery Layers

Discovery is represented through the `DiscoveryLayer` enum, which indicates the best strategy to reach a target based on social relationships:

```rust
pub enum DiscoveryLayer {
    /// No relationship with target - must use rendezvous/flooding discovery
    Rendezvous,
    /// We have neighborhood presence and can use traversal
    Neighborhood,
    /// Target is reachable via home-level relay
    Home,
    /// Target is personally known - we have a direct relationship
    Direct,
}
```

The discovery layer determines routing strategy and flow costs. `Direct` has minimal cost. `Home` relays through same-home participants. `Neighborhood` uses multi-hop traversal. `Rendezvous` requires global flooding and has the highest cost.

### 5.2 Movement Rules

Movement is possible when a Biscuit capability authorizes entry, neighborhood policy allows movement along a 1-hop link, and home policy or invitations allow deeper access levels. Movement does not replicate pinned data. Visitors operate on ephemeral local state.

Traversal does not reveal global identity. Only contextual identities within encountered homes are visible.

## 6. Storage Constraints

### 6.1 Block-Level Allocation

Homes have a fixed size of 10 MB total. Allocation depends on neighborhood participation.

| Neighborhoods | Allocation | Participant Storage | Shared Storage |
|---------------|----------|------------------|--------------|
| 1             | 1.0 MB   | 1.6 MB           | 7.4 MB       |
| 2             | 2.0 MB   | 1.6 MB           | 6.4 MB       |
| 3             | 3.0 MB   | 1.6 MB           | 5.4 MB       |
| 4             | 4.0 MB   | 1.6 MB           | 4.4 MB       |

More neighborhood connections mean less local storage for home culture. This creates meaningful trade-offs.

### 6.2 Flow Budget Integration

Storage constraints are enforced via the flow budget system.

```rust
pub struct HomeFlowBudget {
    /// Home ID (typed identifier)
    pub home_id: HomeId,
    /// Current number of participants
    pub participant_count: u8,
    /// Storage used by participants (spent counter as fact)
    pub participant_storage_spent: u64,
    /// Number of neighborhoods joined
    pub neighborhood_count: u8,
    /// Total neighborhood allocations
    pub neighborhood_allocations: u64,
    /// Storage used by pinned content (spent counter as fact)
    pub pinned_storage_spent: u64,
}
```

The spent counters are persisted as journal facts. The count fields track current membership. Limits are derived at runtime from home policy and Biscuit capabilities. Participant storage limit is 1.6 MB for 8 participants at 200 KB each.

## 7. Fact Schema

### 7.1 Home Facts

Home facts enable Datalog queries.

```datalog
home(home_id, created_at, storage_limit).
home_config(home_id, max_participants, neighborhood_limit).
participant(authority_id, home_id, joined_at, storage_allocated).
moderator(authority_id, home_id, designated_by, designated_at, capabilities).
pinned(content_hash, home_id, pinned_by, pinned_at, size_bytes).
```

These facts express home existence, configuration, participation, moderator designation, and pin state.

### 7.2 Neighborhood Facts

Neighborhood facts express neighborhood existence, home membership, 1-hop links, and access permissions.

```datalog
neighborhood(neighborhood_id, created_at).
home_member(home_id, neighborhood_id, joined_at, allocated_storage).
one_hop_link(home_a, home_b, neighborhood_id).
access_allowed(from_home, to_home, capability_requirement).
```

### 7.3 Query Examples

Queries use Biscuit Datalog.

```datalog
participants_of(Home) <- participant(Auth, Home, _, _).

visitable(Target) <-
    participant(Me, Current, _, _),
    one_hop_link(Current, Target, _),
    access_allowed(Current, Target, Cap),
    has_capability(Me, Cap).
```

The first query finds all participants of a home. The second finds homes a user can visit from their current position via 1-hop links.

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

Moderator commands require moderator capabilities, and moderators must be members.

| Command | Description | Capability |
|---------|-------------|------------|
| `/kick <user>` | Remove from home | `moderate:kick` |
| `/ban <user>` | Ban from home | `moderate:ban` |
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

### 9.1 Home Governance

Homes govern themselves through capability issuance, consensus-based decisions, member moderation designations, and moderation. Home governance uses Aura Consensus for irreversible or collective decisions.

### 9.2 Neighborhood Governance

Neighborhoods govern home admission, 1-hop graph maintenance, access rules, and shared civic norms. High-stakes actions use Aura Consensus.

## 10. Privacy Model

### 10.1 Contextual Identity

Identity in Aura is contextual and relational. Joining a home reveals a home-scoped identity. Leaving a home causes that contextual identity to disappear. Profile data shared inside a context stays local to that context.

### 10.2 Consent Model

Disclosure is consensual. The device issues a join request. Home governance approves using local policy. The authority accepts the capability bundle. This sequence ensures all participation is explicit.

## 11. V1 Constraints

For the initial release, the model is simplified with three constraints.

Each user is a member of exactly one home. This eliminates multi-membership complexity and allows core infrastructure to stabilize.

Each home has a maximum of 8 participants. This human-scale limit enables strong community bonds and manageable governance.

Each home may join a maximum of 4 neighborhoods. This limits 1-hop graph complexity and effect delegation routing overhead.

## 12. Infrastructure Roles

Homes and neighborhoods provide infrastructure services beyond social organization. The `aura-social` crate implements these roles through materialized views and relay selection.

### 12.1 Home Infrastructure

Homes provide data availability and relay services for members and participants:

- **Data Replication**: Home members and participants replicate pinned data across available devices. The `HomeAvailability` type coordinates replication factor and failover.
- **Message Relay**: 0-hop relays serve as first-hop relays for unknown destinations. Social topology queries return currently reachable relays.
- **Storage Coordination**: The `StorageService` enforces storage budgets per participant and tracks usage facts.

### 12.2 Neighborhood Infrastructure

Neighborhoods enable multi-hop routing and cross-home coordination:

- **Descriptor Propagation**: Neighborhood 1-hop links define descriptor propagation paths. Connected homes exchange routing information.
- **Access Capabilities**: `TraversalAllowedFact` grants movement between homes. Access-level limits constrain routing overhead.
- **Multi-Hop Relay**: When home-level relay fails, neighborhood traversal provides alternate paths.

### 12.3 Progressive Discovery Layers

The `aura-social` crate implements a four-layer discovery model:

| Layer | Priority | Resources Required | Flow Cost |
|-------|----------|-------------------|-----------|
| Direct | 0 | Known peer relationship | Minimal |
| Home | 1 | 0-hop relays available | Low |
| Neighborhood | 2 | 1-hop/2-hop routes | Medium |
| Rendezvous | 3 | Global flooding | High |

Discovery layer selection uses `SocialTopology::discovery_layer()`:

```rust
let topology = SocialTopology::new(local_authority, home, neighborhoods);
let layer = topology.discovery_layer(&target);
```

Lower priority layers are preferred when available. This creates economic incentives to establish social relationships before communication.

### 12.4 Relay Selection

The `SocialTopology` provides relay candidate generation:

```rust
let topology = SocialTopology::new(local_authority, home, neighborhoods);
let candidates = topology.build_relay_candidates(&destination, |peer| is_reachable(peer));
```

Candidates are returned in priority order: same-home relays first, then 1-hop and 2-hop neighborhood relays, then guardians. Reachability checks filter unreachable peers. Each candidate includes relay-relationship metadata indicating same-home, neighborhood-hop, or guardian routing.

## See Also

[Database Architecture](107_database.md) describes fact storage and queries. [Transport and Information Flow](111_transport_and_information_flow.md) covers AMP messaging. [Authorization](106_authorization.md) describes capability evaluation. [Rendezvous Architecture](113_rendezvous.md) details the four-layer discovery model integration.
