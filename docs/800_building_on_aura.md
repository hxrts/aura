# Building on Aura: Complete Developer Guide

This comprehensive guide covers everything you need to build applications and distributed protocols on the Aura platform, from basic setup to advanced distributed systems implementation.

## Table of Contents

1. [Getting Started](#getting-started)
2. [Development Environment Setup](#development-environment-setup)
3. [Understanding Aura's Architecture](#understanding-auras-architecture)
4. [Your First Aura Application](#your-first-aura-application)
5. [Effect System and Handler Factories](#effect-system-and-handler-factories)
6. [CRDT-Based State Management](#crdt-based-state-management)
7. [Authentication and Authorization](#authentication-and-authorization)
8. [Creating Distributed Protocols](#creating-distributed-protocols)
9. [Advanced Protocol Patterns](#advanced-protocol-patterns)
10. [Testing and Validation](#testing-and-validation)
11. [Deployment and Operations](#deployment-and-operations)

---

## Getting Started

### Prerequisites

- **Nix with flakes enabled** (required for development environment)
- Basic knowledge of Rust programming and async/await patterns
- Understanding of distributed systems concepts (helpful but not required)
- Familiarity with algebraic data types and effect systems (recommended)

### What You'll Learn

This guide teaches you to build distributed applications using Aura's unique approach:
- **Algebraic Effects**: Clean separation between logic and runtime concerns
- **CRDT-Based State**: Eventually-consistent distributed state without coordination
- **Threshold Cryptography**: Multi-device security without single points of failure
- **Choreographic Programming**: Write distributed protocols from a global perspective

### Current Implementation Status

Aura has undergone significant architectural refinement. The current implementation provides:

**Solid Foundation:**
- Complete algebraic effects system (`aura-core`, `aura-protocol`)
- Comprehensive CRDT infrastructure (`aura-journal`, semilattice types)
- Threshold cryptography (`aura-frost`, `aura-crypto`)
- Modular protocol crates (`aura-authenticate`, `aura-identity`, etc.)

**Under Development:**
- Full choreographic protocol implementations
- Cross-platform application layer
- Integration between protocol crates

### 🔧 Today's API vs 🚀 Planned API

This guide covers both **current working APIs** and **planned future APIs**. Look for these markers:

- **✅ Working Today**: APIs you can use now in tests and development
- **⚠️ Partial Implementation**: Infrastructure exists, but some features are incomplete
- **🚀 Future Work**: Planned APIs that don't exist yet (marked with "Future Work" badges)

**What You Can Build Today:**
- Effect-based applications using `AuraEffectSystem`
- CRDT-based state management with journal semilattices
- Threshold cryptography with FROST signatures
- Basic distributed protocols using effect composition

**What Requires Future Implementation:**
- Full choreographic protocol generation
- Cross-platform GUI applications
- Advanced privacy features (cover traffic, onion routing)
- Production deployment tooling

---

## Development Environment Setup

### 1. Clone and Enter Development Environment

```bash
git clone https://github.com/your-org/aura.git
cd aura

# Enter the Nix development shell
nix develop

# OR with direnv for automatic activation
echo "use flake" > .envrc && direnv allow
```

### 2. Verify Your Setup

```bash
# Check that everything builds
just build

# Run the test suite
just test

# Check formatting and linting
just ci
```

### 3. Explore the Current Architecture

```bash
# See the workspace structure
find crates -name Cargo.toml | head -15

# Examine core types and effects
ls crates/aura-core/src/
ls crates/aura-protocol/src/effects/

# Look at CRDT implementations
ls crates/aura-journal/src/semilattice/
```

---

## Understanding Aura's Architecture

### Architectural Principles

Aura is built on five foundational principles that distinguish it from traditional distributed systems:

1. **Algebraic Effects**: Clean separation between "what to do" (effects) and "how to do it" (handlers)
2. **Semilattice CRDTs**: Monotonic state that merges without conflicts using join (⊔) and meet (⊓) operations
3. **Threshold Cryptography**: Security through M-of-N schemes instead of single-device trust
4. **Context Isolation**: Privacy-preserving communication through cryptographic contexts
5. **Choreographic Programming**: Global protocol specification with automatic local projection

### Layer Architecture

```text
┌─────────────────────────────────────────────────┐
│                Application Layer                │  ← Your Code Here
│            (Your applications)                  │
├─────────────────────────────────────────────────┤
│              Business Logic Layer               │
│  aura-agent │ aura-journal │ aura-frost │ ...   │
├─────────────────────────────────────────────────┤
│             Coordination Layer                  │
│         aura-protocol │ aura-mpst               │
├─────────────────────────────────────────────────┤
│              Foundation Layer                   │
│                 aura-core                       │
└─────────────────────────────────────────────────┘
```

**Layer Descriptions:**

- **Foundation** (`aura-core`): Core types, effect interfaces, semilattice traits, identifiers
- **Coordination** (`aura-protocol`): Effect handlers, runtime composition, choreographic adapters
- **Business Logic**: Domain-specific protocols and types (authentication, identity, etc.)
- **Application**: User-facing applications built on the foundation

### Key Concepts

**Effects and Handlers:**
```rust
// Effect interface defines operations without specifying implementation
#[async_trait]
pub trait CryptoEffects {
    async fn blake3_hash(&self, data: &[u8]) -> [u8; 32];
}

// Real handler uses actual cryptography for production
pub struct RealCryptoHandler;

// Mock handler provides deterministic behavior for testing
pub struct MockCryptoHandler;
```
Effect interfaces separate "what to do" from "how to do it", enabling the same application logic to work with different implementations for testing, simulation, and production environments.

**Semilattice CRDTs:**
```rust
// Join semilattice: data grows monotonically
impl JoinSemilattice for Counter {
    fn join(&self, other: &Self) -> Self {
        Counter(self.0.max(other.0))
    }
}

// Meet semilattice: permissions become more restrictive
impl MeetSemilattice for CapabilitySet {
    fn meet(&self, other: &Self) -> Self {
        self.intersection(other)
    }
}
```
Semilattices provide automatic conflict resolution: join semilattices accumulate data (like counters and logs), while meet semilattices restrict capabilities (like permissions and access controls).

**Context Isolation:**
```rust
// Each relationship has unique cryptographic context
let context_alice_bob = RelationshipId::derive(alice_id, bob_id);
let context_alice_charlie = RelationshipId::derive(alice_id, charlie_id);

// Messages are restricted to their specific context
send_message(context_alice_bob, message);
```
Context isolation prevents metadata leakage by ensuring communication within specific relationships remains cryptographically separated from other interactions.

---

## Your First Aura Application

Let's build a collaborative note-taking application that demonstrates Aura's core capabilities.

### 1. Create Your Application Crate

```bash
# Create new workspace member for your application
mkdir crates/note-app
cd crates/note-app

# Basic Cargo.toml with Aura dependencies
cat > Cargo.toml << 'EOF'
[package]
name = "note-app"
version = "0.1.0"
edition = "2021"

[dependencies]
aura-core = { path = "../aura-core" }
aura-protocol = { path = "../aura-protocol" }
aura-journal = { path = "../aura-journal" }
EOF

mkdir src
```
This creates a minimal project structure that links to Aura's core crates for building distributed applications.

### 2. Define Your Domain Types

Create `src/types.rs`:

```rust
//! Domain types for the collaborative note-taking application

use aura_core::{AccountId, DeviceId, ContentId};
use aura_journal::semilattice::{JoinSemilattice, Bottom, CvState};
use serde::{Serialize, Deserialize};
use std::collections::HashMap;

/// A collaborative note that syncs across devices
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Note {
    pub id: ContentId,
    pub title: String,
    pub content: String,
    pub created_by: DeviceId,
    pub created_at: u64, // Unix timestamp
    pub last_modified: u64,
    pub version: u64,
}

/// CRDT collection of notes with automatic conflict resolution
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NoteCollection {
    pub notes: HashMap<ContentId, Note>,
    pub version_vector: HashMap<DeviceId, u64>,
}

impl JoinSemilattice for NoteCollection {
    fn join(&self, other: &Self) -> Self {
        let mut notes = self.notes.clone();
        let mut version_vector = self.version_vector.clone();

        // Keep most recent note version for each ID
        for (id, other_note) in &other.notes {
            match notes.get(id) {
                Some(our_note) if other_note.last_modified > our_note.last_modified => {
                    notes.insert(*id, other_note.clone());
                }
                None => {
                    notes.insert(*id, other_note.clone());
                }
                _ => {} // Our version is newer
            }
        }

        // Version vector tracks maximum version per device
        for (device, version) in &other.version_vector {
            let current = version_vector.get(device).unwrap_or(&0);
            version_vector.insert(*device, (*current).max(*version));
        }

        NoteCollection { notes, version_vector }
    }
}
```
The join operation automatically resolves conflicts by keeping the most recently modified version of each note, while version vectors track causality between updates.

impl Bottom for NoteCollection {
    fn bottom() -> Self {
        Self {
            notes: HashMap::new(),
            version_vector: HashMap::new(),
        }
    }
}

impl CvState for NoteCollection {}

impl NoteCollection {
    pub fn add_note(&mut self, note: Note, device_id: DeviceId) {
        // Increment version vector for this device
        let current_version = self.version_vector.get(&device_id).unwrap_or(&0);
        self.version_vector.insert(device_id, current_version + 1);
        self.notes.insert(note.id, note);
    }

    pub fn update_note(
        &mut self,
        note_id: ContentId,
        title: String,
        content: String,
        device_id: DeviceId
    ) {
        if let Some(note) = self.notes.get_mut(&note_id) {
            note.title = title;
            note.content = content;
            note.last_modified = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();

            let current_version = self.version_vector.get(&device_id).unwrap_or(&0);
            note.version = current_version + 1;
            self.version_vector.insert(device_id, current_version + 1);
        }
    }

    pub fn list_notes(&self) -> Vec<&Note> {
        let mut notes: Vec<&Note> = self.notes.values().collect();
        notes.sort_by_key(|n| n.created_at);
        notes
    }
}
```

### 3. Create the Application Handler **✅ Working Today**

Create `src/app.rs`:

```rust
//! Main application logic demonstrating Aura's effect system

use crate::types::{Note, NoteCollection};
use aura_core::{AccountId, DeviceId, ContentId};
use aura_protocol::effects::system::AuraEffectSystem;
use aura_protocol::effects::{CryptoEffects, StorageEffects, TimeEffects, ConsoleEffects};
use aura_journal::effects::JournalEffects;
use async_trait::async_trait;
use std::sync::Arc;

pub struct NoteApp {
    device_id: DeviceId,
    account_id: AccountId,
    handler: Arc<AuraEffectSystem>,
    note_collection: NoteCollection,
}

impl NoteApp {
    pub async fn new(device_id: DeviceId, account_id: AccountId) -> Result<Self, Box<dyn std::error::Error>> {
        // Create effect handler for testing environment
        let handler = AuraEffectSystem::for_testing(device_id.clone())?;

        // Load existing notes from storage (if any)
        let note_collection = Self::load_notes_from_storage(&handler, &account_id).await
            .unwrap_or_else(|_| NoteCollection::bottom());

        handler.console().print("Note application initialized").await?;

        Ok(Self {
            device_id,
            account_id,
            handler: Arc::new(handler),
            note_collection,
        })
    }

    pub async fn create_note(&mut self, title: String, content: String) -> Result<ContentId, Box<dyn std::error::Error>> {
        let note_id = ContentId::generate();
        let timestamp = self.handler.time().current_timestamp().await?;

        let note = Note {
            id: note_id,
            title: title.clone(),
            content: content.clone(),
            created_by: self.device_id,
            created_at: timestamp,
            last_modified: timestamp,
            version: 1,
        };

        // Update local CRDT state
        self.note_collection.add_note(note, self.device_id);

        // Persist to storage
        self.save_to_storage().await?;

        self.handler.console().print(&format!("Created note: {}", title)).await?;

        Ok(note_id)
    }

    pub async fn update_note(
        &mut self,
        note_id: ContentId,
        title: String,
        content: String
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.note_collection.update_note(note_id, title.clone(), content, self.device_id);
        self.save_to_storage().await?;

        self.handler.console().print(&format!("Updated note: {}", title)).await?;
        Ok(())
    }

    pub fn list_notes(&self) -> Vec<&Note> {
        self.note_collection.list_notes()
    }

    /// Simulate syncing with another device by merging their state
    pub async fn sync_with_device(&mut self, other_collection: NoteCollection) -> Result<(), Box<dyn std::error::Error>> {
        let old_count = self.note_collection.notes.len();

        // CRDT join automatically handles conflicts
        self.note_collection = self.note_collection.join(&other_collection);

        let new_count = self.note_collection.notes.len();
        self.handler.console().print(&format!("Synced: {} -> {} notes", old_count, new_count)).await?;

        self.save_to_storage().await?;
        Ok(())
    }

    async fn save_to_storage(&self) -> Result<(), Box<dyn std::error::Error>> {
        let data = serde_json::to_vec(&self.note_collection)?;
        let key = format!("notes::{}", self.account_id);

        self.handler.storage().store(&key, data).await?;
        Ok(())
    }

    async fn load_notes_from_storage(
        handler: &AuraEffectSystem,
        account_id: &AccountId
    ) -> Result<NoteCollection, Box<dyn std::error::Error>> {
        let key = format!("notes::{}", account_id);
        let data = handler.storage().retrieve(&key).await?;
        let collection = serde_json::from_slice(&data)?;
        Ok(collection)
    }
}
```

### 4. Create the CLI Interface

Create `src/main.rs`:

```rust
mod types;
mod app;

use app::NoteApp;
use types::NoteCollection;
use aura_core::{AccountId, DeviceId, ContentId};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "note-app")]
#[command(about = "A collaborative note-taking app built on Aura")]
struct Cli {
    #[arg(long)]
    device_id: Option<String>,

    #[arg(long)]
    account_id: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a new note
    Create {
        /// Note title
        title: String,
        /// Note content
        content: String,
    },
    /// List all notes
    List,
    /// Update an existing note
    Update {
        /// Note ID to update
        id: String,
        /// New title
        title: String,
        /// New content
        content: String,
    },
    /// Simulate syncing with another device
    Sync {
        /// Path to another device's note collection
        other_data: String,
    },
    /// Show application info
    Info,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // Generate or parse device/account IDs
    let device_id = match cli.device_id {
        Some(id) => DeviceId::from_string(&id)?,
        None => {
            let id = DeviceId::generate();
            println!("Generated device ID: {}", id);
            id
        }
    };

    let account_id = match cli.account_id {
        Some(id) => AccountId::from_string(&id)?,
        None => {
            let id = AccountId::generate();
            println!("Generated account ID: {}", id);
            id
        }
    };

    // Initialize the application
    let mut app = NoteApp::new(device_id, account_id).await?;

    // Execute the requested command
    match cli.command {
        Commands::Create { title, content } => {
            let note_id = app.create_note(title, content).await?;
            println!("Created note with ID: {}", note_id);
        },

        Commands::List => {
            let notes = app.list_notes();
            if notes.is_empty() {
                println!("No notes found.");
            } else {
                println!("\nYour Notes:");
                for note in notes {
                    let preview = if note.content.len() > 50 {
                        format!("{}...", &note.content[..50])
                    } else {
                        note.content.clone()
                    };
                    println!("  {}", note.title);
                }
            }
        },

        Commands::Update { id, title, content } => {
            let note_id = ContentId::from_string(&id)?;
            app.update_note(note_id, title, content).await?;
        },

        Commands::Sync { other_data } => {
            let data = tokio::fs::read_to_string(other_data).await?;
            let other_collection: NoteCollection = serde_json::from_str(&data)?;
            app.sync_with_device(other_collection).await?;
        },

        Commands::Info => {
            println!("Aura Note App");
            println!("Device ID: {}", device_id);
            println!("Account ID: {}", account_id);
            println!("Notes: {}", app.list_notes().len());
            println!("\nFeatures Demonstrated:");
            println!("  Algebraic effects for clean I/O separation");
            println!("  CRDT-based automatic conflict resolution");
            println!("  Cross-device synchronization");
            println!("  Persistent storage with content addressing");
        },
    }

    Ok(())
}
```

### 5. Build and Test Your Application

```bash
# Add to workspace Cargo.toml
cd ../..
echo '[workspace.dependencies]' >> Cargo.toml
echo 'note-app = { path = "crates/note-app" }' >> Cargo.toml

# Build
cargo build -p note-app

# Test basic functionality
cargo run -p note-app -- create "My First Note" "Built with Aura's effect system!"
cargo run -p note-app -- list
cargo run -p note-app -- info
```

---

## Effect System and Handler Factories

The effect system is Aura's foundation for building composable, testable distributed applications.

### Understanding the Effect Architecture

**Effects** define *what* operations are available:
```rust
#[async_trait]
pub trait CryptoEffects {
    async fn blake3_hash(&self, data: &[u8]) -> [u8; 32];
    async fn generate_keypair(&self) -> KeyPair;
    async fn sign(&self, key: &PrivateKey, data: &[u8]) -> Signature;
}
```

**Handlers** define *how* operations are implemented:
```rust
pub struct RealCryptoHandler;  // Uses actual cryptography
pub struct MockCryptoHandler;  // Returns deterministic test values
pub struct SimCryptoHandler;   // Controlled simulation behavior
```

### Available Effect Types

| Category | Effect Type | Purpose |
|----------|-------------|---------|
| **Core** | TimeEffects | Timestamps, timeouts, controlled time |
| | CryptoEffects | Hashing, signatures, key generation |
| | StorageEffects | Persistent key-value storage |
| | NetworkEffects | Peer communication and discovery |
| | ConsoleEffects | Logging and debugging output |
| | RandomEffects | Cryptographic randomness |
| **Distributed** | JournalEffects | CRDT journal operations |
| | TreeEffects | Ratchet tree operations |
| | SyncEffects | Anti-entropy synchronization |
| | ChoreographicEffects | Protocol coordination |

### Working with AuraEffectSystem **✅ Working Today**

The `AuraEffectSystem` provides all effects through a single interface:

```rust
use aura_protocol::effects::system::AuraEffectSystem;
use aura_protocol::effects::*;

async fn demonstrate_effects() -> Result<(), Box<dyn std::error::Error>> {
    let device_id = DeviceId::generate();
    let handler = AuraEffectSystem::for_testing(device_id)?;

    // Get current time using time effects
    let timestamp = handler.time().current_timestamp().await?;

    // Hash data using crypto effects
    let data = b"hello world";
    let hash = handler.crypto().blake3_hash(data).await?;

    // Store and retrieve data using storage effects
    handler.storage().store("key", data.to_vec()).await?;
    let retrieved = handler.storage().retrieve("key").await?;

    Ok(())
}
```
The AuraEffectSystem provides unified access to all effect types, enabling clean separation between application logic and implementation details.

### Execution Modes

Choose appropriate handlers for your context:

```rust
// Testing mode: fast, deterministic, no side effects
let test_handler = AuraEffectSystem::for_testing(device_id)?;

// Production mode: real cryptography, persistent storage
let prod_handler = AuraEffectSystem::for_production(device_id)?;

// Simulation mode: controlled faults, deterministic randomness
let sim_handler = AuraEffectSystem::for_simulation(device_id, seed)?;
```
Different execution modes allow the same application code to run in testing (fast, isolated), production (real effects), or simulation (controlled chaos) environments.

### Creating Custom Effects

For domain-specific operations, define custom effect traits:

```rust
#[async_trait]
pub trait BiometricEffects {
    async fn capture_fingerprint(&self) -> Option<Vec<u8>>;
    async fn verify_fingerprint(&self, template: &[u8]) -> bool;
}

pub struct MockBiometricHandler;

#[async_trait]
impl BiometricEffects for MockBiometricHandler {
    async fn capture_fingerprint(&self) -> Option<Vec<u8>> {
        Some(b"test_fingerprint".to_vec())
    }

    async fn verify_fingerprint(&self, _template: &[u8]) -> bool {
        true  // Always succeeds in tests
    }
}
```

---

## CRDT-Based State Management

Aura uses Conflict-free Replicated Data Types (CRDTs) for distributed state that merges automatically without coordination.

### Understanding Semilattice Types

**Join Semilattices** (growing, accumulative):
- Used for facts, logs, counters, sets
- Merge operator: `⊔` (join/union)
- Satisfies: `a ⊔ b = b ⊔ a`, `(a ⊔ b) ⊔ c = a ⊔ (b ⊔ c)`, `a ⊔ a = a`

**Meet Semilattices** (restricting, constraint-based):
- Used for capabilities, permissions, policies
- Merge operator: `⊓` (meet/intersection)
- Satisfies: `a ⊓ b = b ⊓ a`, `(a ⊓ b) ⊓ c = a ⊓ (b ⊓ c)`, `a ⊓ a = a`

### Building CRDTs

#### Simple Counter (Join Semilattice)

```rust
use aura_core::semilattice::{JoinSemilattice, Bottom, CvState};

#[derive(Debug, Clone, PartialEq)]
pub struct Counter(pub u64);

impl JoinSemilattice for Counter {
    fn join(&self, other: &Self) -> Self {
        Counter(self.0.max(other.0))
    }
}

impl Bottom for Counter {
    fn bottom() -> Self {
        Counter(0)  // Identity element for maximum operation
    }
}

impl CvState for Counter {}

// Counter automatically resolves conflicts by taking maximum value
let counter_a = Counter(5);
let counter_b = Counter(8);
let merged = counter_a.join(&counter_b);
assert_eq!(merged, Counter(8));
```

#### OR-Set (Add-Remove Set)

```rust
use std::collections::{HashSet, HashMap};

#[derive(Debug, Clone, PartialEq)]
pub struct OrSet<T: Clone + Eq + std::hash::Hash> {
    added: HashMap<T, HashSet<u64>>,    // Item -> set of add timestamps
    removed: HashMap<T, HashSet<u64>>,  // Item -> set of remove timestamps
}

impl<T: Clone + Eq + std::hash::Hash> JoinSemilattice for OrSet<T> {
    fn join(&self, other: &Self) -> Self {
        let mut added = self.added.clone();
        let mut removed = self.removed.clone();

        // Merge add and remove operations from both sets
        for (item, timestamps) in &other.added {
            added.entry(item.clone()).or_default().extend(timestamps);
        }
        
        Self { added, removed }
    }
}

impl<T: Clone + Eq + std::hash::Hash> OrSet<T> {
    pub fn contains(&self, item: &T) -> bool {
        let add_count = self.added.get(item).map(|s| s.len()).unwrap_or(0);
        let rem_count = self.removed.get(item).map(|s| s.len()).unwrap_or(0);
        add_count > rem_count  // Item present if more adds than removes
    }
}
```
OR-Set (Observed-Remove Set) tracks both add and remove operations with timestamps, resolving conflicts by counting operations: an item is present if it has been added more times than removed.
```

#### Capability Set (Meet Semilattice)

```rust
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq)]
pub struct CapabilitySet {
    permissions: BTreeSet<String>,
}

impl MeetSemilattice for CapabilitySet {
    fn meet(&self, other: &Self) -> Self {
        let permissions = self.permissions
            .intersection(&other.permissions)
            .cloned()
            .collect();
        Self { permissions }
    }
}

impl CapabilitySet {
    pub fn can(&self, permission: &str) -> bool {
        self.permissions.contains(permission)
    }
}
```
Capability sets use intersection (meet) to combine permissions restrictively: when two capability sets are merged, only permissions present in both are retained, ensuring the principle of least authority.
```

### Advanced CRDT: Collaborative Text Document

```rust
use std::collections::BTreeMap;

type Position = u64;
type AuthorId = DeviceId;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct CharacterId {
    position: Position,
    author: AuthorId,
    sequence: u64,  // Unique sequence number from author
}

#[derive(Debug, Clone)]
pub struct CollaborativeText {
    characters: BTreeMap<CharacterId, char>,
    author_counters: HashMap<AuthorId, u64>,
}

impl JoinSemilattice for CollaborativeText {
    fn join(&self, other: &Self) -> Self {
        let mut characters = self.characters.clone();
        let mut author_counters = self.author_counters.clone();

        // Merge all character insertions
        for (id, ch) in &other.characters {
            characters.insert(id.clone(), *ch);
        }

        // Track maximum counter per author for causality
        for (author, counter) in &other.author_counters {
            let current = author_counters.get(author).unwrap_or(&0);
            author_counters.insert(*author, (*current).max(*counter));
        }

        Self { characters, author_counters }
    }
}
```
Collaborative text uses unique character IDs with position, author, and sequence number to enable conflict-free merging of concurrent edits from multiple authors.
```

### Using CRDTs with Effect Handlers

```rust
use aura_protocol::effects::semilattice::{CvHandler, execute_cv_sync};

async fn crdt_sync_example() -> Result<(), Box<dyn std::error::Error>> {
    // Create CRDT handler for our note collection
    let mut cv_handler = CvHandler::<NoteCollection>::new();

    // Simulate receiving state from remote devices
    let remote_state = NoteCollection {
        notes: /* notes from other device */,
        version_vector: /* version vector from other device */,
    };

    // CRDT automatically merges without conflicts
    cv_handler.merge_state(remote_state);

    // Execute distributed synchronization protocol
    let participants = vec![device1_id, device2_id, device3_id];
    execute_cv_sync(&mut cv_handler, participants, session_id).await?;

    Ok(())
}
```

---

## Authentication and Authorization

Aura's security model combines cryptographic identity verification with capability-based authorization.

### Identity and Key Management

```rust
use aura_authenticate::{DeviceAuthentication, GuardianAuthentication};
use aura_core::{DeviceId, AccountId, GuardianId};

async fn setup_device_authentication() -> Result<(), Box<dyn std::error::Error>> {
    let device_id = DeviceId::generate();
    let account_id = AccountId::generate();

    // Create device authentication handler
    let mut device_auth = DeviceAuthentication::new(device_id, account_id);

    // Generate cryptographic keys for this device
    device_auth.generate_device_keys().await?;

    // Enable platform-specific biometric security
    #[cfg(target_os = "macos")]
    device_auth.enable_biometric_auth().await?;

    Ok(())
}
```

### Guardian-Based Social Recovery

```rust
use aura_recovery::{GuardianRecovery, RecoveryRequest, RecoveryPolicy};

async fn setup_social_recovery() -> Result<(), Box<dyn std::error::Error>> {
    let account_id = AccountId::generate();
    let device_id = DeviceId::generate();

    // Set up social recovery with threshold voting
    let mut recovery = GuardianRecovery::new(account_id, device_id);

    // Add trusted guardians
    recovery.add_guardian(guardian1, "Alice".to_string()).await?;
    recovery.add_guardian(guardian2, "Bob".to_string()).await?;
    recovery.add_guardian(guardian3, "Charlie".to_string()).await?;

    // Configure recovery policy: 2-of-3 threshold with dispute window
    let policy = RecoveryPolicy {
        threshold: 2,
        timeout_hours: 48,
        require_biometric: true,
    };
    recovery.set_policy(policy).await?;

    Ok(())
}
```

### Capability-Based Authorization

```rust
use aura_wot::{CapabilitySet, AuthorizationRequest, authorize_operation};

async fn capability_authorization_example() -> Result<(), Box<dyn std::error::Error>> {
    // Define permission hierarchies
    let admin_caps = CapabilitySet::from_permissions(&[
        "storage:read", "storage:write", "storage:admin"
    ]);

    let user_caps = CapabilitySet::from_permissions(&[
        "storage:read", "storage:write"
    ]);

    // Capabilities use intersection for restriction
    let effective_caps = admin_caps.meet(&user_caps);
    assert!(effective_caps.can("storage:read"));
    assert!(!effective_caps.can("storage:admin"));

    // Check authorization for specific operation
    let request = AuthorizationRequest {
        operation: "storage:write",
        required_capabilities: CapabilitySet::from_permissions(&["storage:write"]),
    };

    let authorized = authorize_operation(request, &user_caps).await?;

    Ok(())
}
```

### Threshold Identity Operations

```rust
use aura_frost::{FrostSigner, ThresholdSignature, KeyGeneration};
use aura_identity::{IdentityTree, TreeOperation};

async fn threshold_operations() -> Result<(), Box<dyn std::error::Error>> {
    // Create 2-of-3 threshold signers
    let devices = vec![DeviceId::generate(), DeviceId::generate(), DeviceId::generate()];

    let mut signers = Vec::new();
    for device_id in &devices {
        let signer = FrostSigner::new(*device_id, 2, 3).await?;
        signers.push(signer);
    }

    // Generate shared public key through distributed protocol
    let public_key = KeyGeneration::generate_threshold_key(&mut signers).await?;

    // Create identity tree with threshold root
    let mut tree = IdentityTree::new(AccountId::generate(), public_key);

    // Sign tree operation with subset of devices (2 of 3)
    let operation = TreeOperation::AddDevice {
        device_id: DeviceId::generate(),
        parent_node: 0,
    };

    let signature = ThresholdSignature::sign(&operation, &[signers[0].clone(), signers[2].clone()]).await?;

    Ok(())
}
```

---

## Creating Distributed Protocols

Aura enables writing distributed protocols using choreographic programming - describing the global protocol behavior and automatically generating local implementations.

### Protocol Foundation

The current implementation provides infrastructure for choreographic protocols:

```rust
use aura_protocol::choreography::{Protocol, ProtocolMessage, ProtocolRole};
use aura_protocol::effects::choreographic::ChoreographicEffects;

/// Example: Deterministic Key Derivation protocol
#[derive(Debug, Clone)]
pub struct DkdProtocol {
    participants: Vec<DeviceId>,
    context: String,
    app_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DkdMessage {
    Commitment(Vec<u8>),
    Reveal(Vec<u8>),
    Confirm(bool),
}

impl DkdProtocol {
    pub async fn execute<H: ChoreographicEffects>(
        &self,
        handler: &H,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        // Phase 1: Generate and share commitments
        let commitment = handler.crypto().generate_commitment(&self.context).await?;
        handler.choreographic().broadcast(DkdMessage::Commitment(commitment.clone())).await?;

        // Phase 2: Reveal commitments
        let reveal = handler.crypto().reveal_commitment(&commitment).await?;
        handler.choreographic().broadcast(DkdMessage::Reveal(reveal.clone())).await?;

        // Phase 3: Derive deterministic shared key
        let reveals = handler.choreographic().collect_messages(self.participants.len()).await?;
        let derived_key = handler.crypto().derive_key(&self.app_id, &self.context, &reveals).await?;

        Ok(derived_key)
    }
}
```
Deterministic Key Derivation (DKD) enables multiple parties to derive the same cryptographic key without revealing individual inputs through a commit-reveal protocol.
```

### Multi-Party Protocol Patterns

#### Request-Response Pattern

```rust
#[derive(Debug)]
pub struct RequestResponseProtocol<Req, Resp> {
    client: DeviceId,
    server: DeviceId,
    _phantom: std::marker::PhantomData<(Req, Resp)>,
}

impl<Req, Resp> RequestResponseProtocol<Req, Resp>
where
    Req: Serialize + DeserializeOwned + Send + Sync,
    Resp: Serialize + DeserializeOwned + Send + Sync,
{
    pub async fn execute_client<H: ChoreographicEffects>(
        &self,
        handler: &H,
        request: Req,
    ) -> Result<Resp, Box<dyn std::error::Error>> {
        // Send request to server
        handler.choreographic().send_message(self.server, serde_json::to_vec(&request)?).await?;

        // Wait for response
        let response_data = handler.choreographic().receive_message(self.server).await?;
        let response: Resp = serde_json::from_slice(&response_data)?;

        Ok(response)
    }
```
Request-response pattern enables simple client-server communication using choreographic effects for reliable message delivery.
}
```

#### Consensus Protocol

```rust
#[derive(Debug)]
pub struct ConsensusProtocol {
    participants: Vec<DeviceId>,
    threshold: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConsensusMessage {
    Propose(Vec<u8>),
    Vote(bool),
    Commit(Vec<u8>),
    Abort,
}

impl ConsensusProtocol {
    pub async fn execute<H: ChoreographicEffects>(
        &self,
        handler: &H,
        proposer: DeviceId,
        proposal: Vec<u8>,
    ) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error>> {
        // Phase 1: Proposal
        if handler.get_device_id() == proposer {
            handler.choreographic().broadcast(
                ConsensusMessage::Propose(proposal.clone())
            ).await?;
        }

        let proposals = handler.choreographic().collect_messages(1).await?;
        let ConsensusMessage::Propose(proposed_value) = &proposals[0] else {
            return Err("Invalid proposal message".into());
        };

        // Phase 2: Vote on proposal
        let vote = self.validate_proposal(proposed_value);
        handler.choreographic().broadcast(ConsensusMessage::Vote(vote)).await?;

        // Phase 3: Count votes and decide
        let votes = handler.choreographic().collect_messages(self.participants.len()).await?;
        let positive_votes = votes.iter().filter(|msg| matches!(msg, ConsensusMessage::Vote(true))).count();

        if positive_votes >= self.threshold {
            handler.choreographic().broadcast(ConsensusMessage::Commit(proposed_value.clone())).await?;
            Ok(Some(proposed_value.clone()))
        } else {
            Ok(None)
        }
    }
```
Consensus protocol uses three phases: proposal broadcast, voting, and threshold-based decision making to ensure all participants agree on a single value.
}
```

#### Gossip Dissemination

```rust
#[derive(Debug)]
pub struct GossipProtocol {
    participants: Vec<DeviceId>,
    fanout: usize,  // Number of peers to gossip to each round
}

impl GossipProtocol {
    pub async fn disseminate<H: ChoreographicEffects>(
        &self,
        handler: &H,
        data: Vec<u8>,
        rounds: usize,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let my_id = handler.get_device_id();
        let mut known_data = std::collections::HashSet::new();
        known_data.insert(data);

        for round in 0..rounds {
            // Select random peers to gossip to
            // Select random subset of peers for this round
            let peers: Vec<_> = self.participants.iter().filter(|&&id| id != my_id).collect();
            let mut rng = handler.random().get_rng().await?;
            let selected_peers = self.select_random_peers(&peers, self.fanout, &mut rng);

            // Send known data to selected peers
            for &peer in &selected_peers {
                for item in &known_data {
                    handler.choreographic().send_message(peer, item.clone()).await?;
                }
            }

            // Collect new data from peers
            let incoming_messages = handler.choreographic().collect_messages_timeout(
                std::time::Duration::from_millis(100)
            ).await?;

            for message in incoming_messages {
                known_data.insert(message);
            }
        }

        Ok(())
    }

    fn select_random_peers(
        &self,
        peers: &[&DeviceId],
        count: usize,
        rng: &mut impl rand::Rng
    ) -> Vec<DeviceId> {
        use rand::seq::SliceRandom;
        peers.choose_multiple(rng, count).map(|&id| *id).collect()
    }
}
```
Gossip protocol spreads information through the network by having each node randomly select peers to share data with, achieving eventual consistency without global coordination.
}
```

### Integrating with CRDTs

Choreographic protocols can integrate with CRDT state for automatic conflict resolution:

```rust
use aura_protocol::effects::semilattice::CvHandler;

pub struct CrdtSyncProtocol<T: CvState> {
    participants: Vec<DeviceId>,
    _phantom: std::marker::PhantomData<T>,
}

impl<T: CvState + Serialize + DeserializeOwned> CrdtSyncProtocol<T> {
    pub async fn synchronize<H: ChoreographicEffects>(
        &self,
        handler: &H,
        crdt_handler: &mut CvHandler<T>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let my_id = handler.get_device_id();

        // Phase 1: Broadcast current state
        let current_state = crdt_handler.get_state().clone();
        let state_data = serde_json::to_vec(&current_state)?;

        handler.choreographic().broadcast(state_data).await?;

        // Phase 2: Collect all states
        let state_messages = handler.choreographic().collect_messages(
            self.participants.len() - 1  // Exclude ourselves
        ).await?;

        // Phase 3: Merge all states using CRDT join
        let mut merged_state = current_state;
        for msg in state_messages {
            let remote_state: T = serde_json::from_slice(&msg)?;
            merged_state = merged_state.join(&remote_state);
        }

        // Update local state
        crdt_handler.update_state(merged_state);

        handler.console().print("Synchronization complete").await?;

        Ok(())
    }
}
```

---

## Advanced Protocol Patterns

### State Machine Replication

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StateMachineOp {
    Increment,
    Decrement,
    Reset,
    SetValue(i64),
}

#[derive(Debug, Clone)]
pub struct ReplicatedStateMachine {
    participants: Vec<DeviceId>,
    sequence_number: u64,
}

impl ReplicatedStateMachine {
    pub async fn execute_operation<H: ChoreographicEffects>(
        &mut self,
        handler: &H,
        operation: StateMachineOp,
    ) -> Result<i64, Box<dyn std::error::Error>> {
        let my_id = handler.get_device_id();

        // Phase 1: Propose operation with sequence number
        self.sequence_number += 1;
        let proposal = (self.sequence_number, operation.clone());

        handler.choreographic().broadcast(
            serde_json::to_vec(&proposal)?
        ).await?;

        // Phase 2: Collect all proposals
        let mut proposals: Vec<(u64, StateMachineOp)> = vec![proposal];

        let messages = handler.choreographic().collect_messages(
            self.participants.len() - 1
        ).await?;

        for msg in messages {
            let remote_proposal: (u64, StateMachineOp) = serde_json::from_slice(&msg)?;
            proposals.push(remote_proposal);
        }

        // Phase 3: Order operations by sequence number
        proposals.sort_by_key(|(seq, _)| *seq);

        // Phase 4: Apply operations in order
        let mut state = 0i64;
        for (_, op) in proposals {
            state = self.apply_operation(state, op);
        }

        Ok(state)
    }

    fn apply_operation(&self, current_state: i64, operation: StateMachineOp) -> i64 {
        match operation {
            StateMachineOp::Increment => current_state + 1,
            StateMachineOp::Decrement => current_state - 1,
            StateMachineOp::Reset => 0,
            StateMachineOp::SetValue(value) => value,
        }
    }
}
```

### Leader Election

```rust
#[derive(Debug)]
pub struct LeaderElection {
    participants: Vec<DeviceId>,
    timeout_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElectionMessage {
    candidate: DeviceId,
    timestamp: u64,
    priority: u64,  // Higher number = higher priority
}

impl LeaderElection {
    pub async fn elect_leader<H: ChoreographicEffects>(
        &self,
        handler: &H,
    ) -> Result<DeviceId, Box<dyn std::error::Error>> {
        let my_id = handler.get_device_id();
        let timestamp = handler.time().current_timestamp().await?;

        // Calculate priority based on device ID hash (deterministic)
        let priority = self.calculate_priority(my_id);

        // Phase 1: Broadcast candidacy
        let candidacy = ElectionMessage {
            candidate: my_id,
            timestamp,
            priority,
        };

        handler.choreographic().broadcast(
            serde_json::to_vec(&candidacy)?
        ).await?;

        // Phase 2: Collect all candidacies
        let mut candidacies = vec![candidacy];

        let messages = handler.choreographic().collect_messages_timeout(
            std::time::Duration::from_millis(self.timeout_ms)
        ).await?;

        for msg in messages {
            let candidacy: ElectionMessage = serde_json::from_slice(&msg)?;
            candidacies.push(candidacy);
        }

        // Phase 3: Deterministic leader selection
        candidacies.sort_by_key(|c| (c.priority, c.timestamp));
        let leader = candidacies.last().unwrap().candidate;

        handler.console().print(&format!("Elected leader: {}", leader)).await?;

        Ok(leader)
    }

    fn calculate_priority(&self, device_id: DeviceId) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        device_id.hash(&mut hasher);
        hasher.finish()
    }
}
```

---

## Testing and Validation

Aura provides comprehensive testing infrastructure for distributed protocols and applications.

### Unit Testing with Effect Mocks

```rust
use aura_protocol::effects::system::AuraEffectSystem;
use tokio_test;

#[tokio::test]
async fn test_note_creation() {
    let device_id = DeviceId::generate();
    let account_id = AccountId::generate();

    // Use test handler with deterministic behavior
    let handler = AuraEffectSystem::for_testing(device_id)?;
    let mut app = NoteApp::new_with_handler(device_id, account_id, handler).await?;

    // Test note creation
    let note_id = app.create_note(
        "Test Note".to_string(),
        "This is a test".to_string(),
    ).await?;

    // Verify state
    let notes = app.list_notes();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].title, "Test Note");
    assert_eq!(notes[0].created_by, device_id);
}
```

### Property-Based Testing for CRDTs

```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn crdt_join_is_associative(
        a: NoteCollection,
        b: NoteCollection,
        c: NoteCollection,
    ) {
        // (a ⊔ b) ⊔ c = a ⊔ (b ⊔ c)
        let left = a.join(&b).join(&c);
        let right = a.join(&b.join(&c));
        prop_assert_eq!(left, right);
    }

    #[test]
    fn crdt_join_is_commutative(
        a: NoteCollection,
        b: NoteCollection,
    ) {
        // a ⊔ b = b ⊔ a
        let left = a.join(&b);
        let right = b.join(&a);
        prop_assert_eq!(left, right);
    }

    #[test]
    fn crdt_join_is_idempotent(a: NoteCollection) {
        // a ⊔ a = a
        let result = a.join(&a);
        prop_assert_eq!(result, a);
    }
}

// Strategy for generating arbitrary NoteCollections
impl Arbitrary for NoteCollection {
    type Parameters = ();
    type Strategy = BoxedStrategy<NoteCollection>;

    fn arbitrary_with(_args: ()) -> Self::Strategy {
        (
            prop::collection::hash_map(
                any::<ContentId>(),
                any::<Note>(),
                0..10
            ),
            prop::collection::hash_map(
                any::<DeviceId>(),
                any::<u64>(),
                0..5
            )
        ).prop_map(|(notes, version_vector)| {
            NoteCollection { notes, version_vector }
        }).boxed()
    }
}
```

### Protocol Integration Testing

```rust
#[tokio::test]
async fn test_dkd_protocol_integration() {
    let participants = vec![
        DeviceId::generate(),
        DeviceId::generate(),
        DeviceId::generate(),
    ];

    // Create handlers for each participant
    let mut handlers = Vec::new();
    for device_id in &participants {
        let handler = AuraEffectSystem::for_testing(*device_id)?;
        handlers.push(handler);
    }

    // Execute protocol concurrently
    let protocol = DkdProtocol {
        participants: participants.clone(),
        context: "test_context".to_string(),
        app_id: "test_app".to_string(),
    };

    let mut tasks = Vec::new();
    for (i, handler) in handlers.into_iter().enumerate() {
        let protocol = protocol.clone();
        let device_id = participants[i];

        let task = tokio::spawn(async move {
            protocol.execute(&handler, device_id).await
        });
        tasks.push(task);
    }

    // Wait for all participants to complete
    let results: Vec<_> = futures::future::join_all(tasks).await;

    // Verify all participants derived the same key
    let keys: Result<Vec<_>, _> = results.into_iter().collect::<Result<Vec<_>, _>>()?
        .into_iter().collect();
    let keys = keys?;

    assert_eq!(keys.len(), 3);
    assert_eq!(keys[0], keys[1]);
    assert_eq!(keys[1], keys[2]);
}
```

### Simulation Testing

```rust
use aura_simulator::{NetworkSimulator, FaultType};

#[tokio::test]
async fn test_consensus_under_network_faults() {
    let simulator = NetworkSimulator::new()
        .with_participants(5)
        .with_faults(vec![
            FaultType::PacketLoss(0.1),      // 10% packet loss
            FaultType::NetworkDelay(100),     // 100ms delays
            FaultType::PartitionHeal(5000),   // Network partition for 5s
        ])
        .with_seed(12345);  // Deterministic testing

    let protocol = ConsensusProtocol {
        participants: simulator.participant_ids(),
        threshold: 3,  // 3 out of 5 needed
    };

    let proposal = b"test_proposal".to_vec();
    let result = simulator.execute_protocol(protocol, proposal).await?;

    // Consensus should succeed despite faults
    assert!(result.is_some());

    // Verify safety properties
    let trace = simulator.get_execution_trace();
    assert!(trace.verify_safety_properties());
    assert!(trace.verify_liveness_properties());
}
```

### Chaos Testing

```rust
#[tokio::test]
async fn test_note_app_chaos() {
    let chaos_config = ChaosConfig {
        duration: Duration::from_secs(60),
        fault_injection_rate: 0.2,
        fault_types: vec![
            ChaosEvent::DeviceRestart,
            ChaosEvent::NetworkPartition,
            ChaosEvent::StorageCorruption,
            ChaosEvent::MessageDuplication,
        ],
    };

    let mut simulation = ChaosSimulation::new(chaos_config);

    // Add multiple note apps
    for i in 0..5 {
        let device_id = DeviceId::generate();
        let account_id = AccountId::generate();
        let app = NoteApp::new(device_id, account_id).await?;
        simulation.add_participant(app);
    }

    // Run chaos testing
    simulation.run().await?;

    // Verify system remained consistent despite chaos
    let final_states = simulation.collect_final_states();
    assert!(all_states_converged(&final_states));
}
```

---

## Deployment and Operations

### Production Deployment **🚀 Future Work**

> **Note**: This API is aspirational and demonstrates planned production capabilities. Current implementation supports basic `AuraEffectSystem::for_production()` but advanced features like biometric auth and secure enclave integration are not yet implemented.

```rust
use aura_protocol::effects::system::AuraEffectSystem;

async fn production_deployment() -> Result<(), Box<dyn std::error::Error>> {
    // Load persistent device identity
    let device_id = load_device_identity_from_keychain().await?;
    let account_id = load_account_identity().await?;

    // Create production handler with security features
    let handler = AuraEffectSystem::for_production(device_id)?
        .with_biometric_auth(true)?
        .with_secure_enclave(true)?
        .with_network_encryption(true)?;

    // Initialize application with production settings
    let app = NoteApp::new_with_handler(device_id, account_id, handler).await?;

    // Start background synchronization
    tokio::spawn(async move {
        app.start_background_sync().await
    });

    println!("Production deployment successful");
    Ok(())
}

#[cfg(target_os = "macos")]
async fn load_device_identity_from_keychain() -> Result<DeviceId, Box<dyn std::error::Error>> {
    use security_framework::keychain;

    let query = keychain::SecItemQuery::new()
        .class(keychain::SecClass::GenericPassword)
        .service("aura-device-identity")
        .account("primary");

    let result = keychain::SecItem::search(query).await?;
    let device_id_bytes = result.data().ok_or("No identity data found")?;
    let device_id_str = String::from_utf8(device_id_bytes)?;

    Ok(DeviceId::from_string(&device_id_str)?)
}
```

### Cross-Platform Considerations **🚀 Future Work**

> **Note**: Platform-specific handler implementations are planned but not yet available. Current implementation provides unified handlers that work across platforms with basic functionality.

```rust
// Platform-specific handler configuration
async fn create_platform_handler(device_id: DeviceId) -> Result<AuraEffectSystem, Box<dyn std::error::Error>> {
    let mut builder = AuraEffectSystem::builder(device_id);

    #[cfg(target_arch = "wasm32")]
    {
        builder = builder
            .with_storage(WebStorageHandler::new()?)
            .with_network(WebSocketHandler::new()?)
            .with_crypto(WebCryptoHandler::new()?);
    }

    #[cfg(target_os = "ios")]
    {
        builder = builder
            .with_storage(KeychainStorageHandler::new()?)
            .with_network(UrlSessionHandler::new()?)
            .with_biometric_auth(true)?;
    }

    #[cfg(target_os = "android")]
    {
        builder = builder
            .with_storage(KeystoreHandler::new()?)
            .with_network(OkHttpHandler::new()?)
            .with_biometric_auth(true)?;
    }

    #[cfg(target_family = "unix")]
    {
        builder = builder
            .with_storage(FilesystemStorageHandler::new()?)
            .with_network(TcpHandler::new()?)
            .with_crypto(NativeCryptoHandler::new()?);
    }

    builder.build()
}
```

### Monitoring and Observability

```rust
use aura_protocol::middleware::{MetricsMiddleware, TracingMiddleware};

async fn setup_monitoring() -> Result<(), Box<dyn std::error::Error>> {
    let device_id = DeviceId::generate();

    // Create handler with observability middleware
    let handler = AuraEffectSystem::for_production(device_id)?
        .with_middleware(MetricsMiddleware::new("aura-app"))
        .with_middleware(TracingMiddleware::new("distributed"))
        .with_health_checks(true);

    // Start metrics collection
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));

        loop {
            interval.tick().await;

            let metrics = handler.collect_metrics().await;
            println!("Metrics: {}", serde_json::to_string_pretty(&metrics)?);

            let health = handler.check_health().await;
            if !health.is_healthy() {
                eprintln!("Health check failed: {:?}", health.issues);
            }
        }
    });

    Ok(())
}
```

### Error Recovery and Resilience

```rust
use aura_protocol::middleware::{RetryMiddleware, CircuitBreakerMiddleware};

async fn resilient_application() -> Result<(), Box<dyn std::error::Error>> {
    let device_id = DeviceId::generate();

    // Configure resilience middleware
    let handler = AuraEffectSystem::for_production(device_id)?
        .with_middleware(RetryMiddleware::new()
            .with_max_attempts(3)
            .with_exponential_backoff()
        )
        .with_middleware(CircuitBreakerMiddleware::new()
            .with_failure_threshold(5)
            .with_timeout(Duration::from_secs(30))
        );

    // Application automatically retries failed operations
    let result = handler.network().send_message(peer_id, message).await;

    match result {
        Ok(_) => println!("Message sent successfully"),
        Err(e) => {
            // Circuit breaker may have opened
            eprintln!("Failed after retries: {}", e);
        }
    }

    Ok(())
}
```

---

## Advanced Topics and Best Practices

### Performance Optimization

**Use Direct Effect Traits for Hot Paths:**
```rust
// Direct effect access: zero overhead
async fn fast_crypto_operation<C: CryptoEffects>(crypto: &C) -> [u8; 32] {
    crypto.blake3_hash(data).await
}

// Composite handler: small overhead for flexibility
async fn flexible_operation(handler: &AuraEffectSystem) -> [u8; 32] {
    handler.crypto().blake3_hash(data).await
}
```
Direct effect trait access provides zero-cost abstractions, while composite handlers add minimal overhead for greater flexibility.

**Batch Operations for Efficiency:**
```rust
// Instead of individual operations
for item in items {
    handler.storage().store(&item.key, item.data).await?;
}

// Batch operations together
let batch: Vec<_> = items.iter().map(|item| (&item.key, &item.data)).collect();
handler.storage().store_batch(batch).await?;
```

### Security Best Practices

**Always Use Context Isolation:**
```rust
// Good: Explicit context for each relationship
let context_ab = RelationshipId::derive(alice_id, bob_id);
send_message(context_ab, message);

// Bad: Global message sending without context
send_message_global(message);
```

**Validate Capability Requirements:**
```rust
// Good: Check capabilities before operations
if !capabilities.can("storage:write") {
    return Err("Insufficient permissions".into());
}
handler.storage().store(key, data).await?;

// Bad: Operations without permission checks
handler.storage().store(key, data).await?;
```

### Common Pitfalls and Solutions

**Pitfall: Forgetting CRDT Laws**
```rust
// Bad: Non-monotonic merge
impl JoinSemilattice for BadCounter {
    fn join(&self, other: &Self) -> Self {
        BadCounter(self.0 - other.0)  // Can decrease!
    }
}

// Good: Monotonic merge
impl JoinSemilattice for GoodCounter {
    fn join(&self, other: &Self) -> Self {
        GoodCounter(self.0.max(other.0))  // Always increasing
    }
}
```

**Pitfall: Blocking Operations in Choreographies**
```rust
// Bad: Synchronous blocking in async protocol
async fn bad_protocol(handler: &Handler) {
    let result = std::thread::spawn(|| {
        expensive_computation()  // Blocks thread pool
    }).join().unwrap();
}

// Good: Async operations
async fn good_protocol(handler: &Handler) {
    let result = tokio::task::spawn_blocking(|| {
        expensive_computation()  // Runs on blocking thread pool
    }).await?;
}
```

### Development Workflow

1. **Start with Effect Interfaces**: Define what your application needs to do
2. **Implement with Test Handlers**: Use `AuraEffectSystem::for_testing()`
3. **Add CRDT Types**: Design your state as semilattice CRDTs
4. **Write Property Tests**: Verify CRDT laws and protocol invariants
5. **Create Production Handlers**: Implement real effects for deployment
6. **Add Monitoring**: Instrument with metrics and tracing middleware

---

## Conclusion

You've now learned how to build distributed applications on Aura's unique architecture. This guide covered:

**Key Concepts Mastered:**
- Algebraic effects for clean separation of concerns
- CRDT-based state management for automatic conflict resolution
- Context isolation for privacy-preserving communication
- Choreographic programming for distributed protocols
- Threshold cryptography for resilient security

**Practical Skills Developed:**
- Building applications with the effect system
- Creating custom CRDTs with proper semilattice laws
- Implementing distributed protocols using choreography
- Testing distributed systems with simulation and property-based testing
- Deploying secure, production-ready applications

**Advanced Topics Explored:**
- Multi-party consensus and leader election protocols
- Integration between choreographic programming and CRDT synchronization
- Cross-platform deployment strategies
- Performance optimization and security best practices

### Next Steps

1. **Explore the Codebase**: Examine existing protocol implementations in `crates/aura-*`
2. **Contribute to Development**: The choreographic protocol implementations need completion
3. **Build Real Applications**: Use the patterns from this guide to create your own distributed applications
4. **Join the Community**: Connect with other Aura developers and contribute to the ecosystem

### Key Takeaways

- **Think in Effects**: Separate what you want to do from how it's implemented
- **Design for Distribution**: Use CRDTs and semilattice properties from the start
- **Test Thoroughly**: Property-based testing and simulation catch distributed system bugs
- **Security by Design**: Context isolation and capabilities should be built-in, not added later
- **Start Simple**: Begin with basic patterns and compose them into complex systems

Aura provides a mathematically sound foundation for building distributed applications that are secure, performant, and correct by construction. The investment in learning these patterns pays dividends in building systems that work reliably across network partitions, device failures, and adversarial conditions.

Happy building!

---

## Related Documentation

- **[Theoretical Foundations](001_theoretical_foundations.md)** - Mathematical foundations and formal model
- **[System Architecture](002_system_architecture.md)** - Implementation patterns and system design
- **[Distributed Applications](003_distributed_applications.md)** - Concrete examples and integration patterns
- **[Privacy Model](004_info_flow_model.md)** - Privacy guarantees and threat modeling
