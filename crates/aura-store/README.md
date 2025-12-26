# Aura Store - Layer 2: Specification

**Purpose**: Storage domain types, semantics, and capability-based access control logic.

This crate provides pure storage domain logic for the Aura platform, including content addressing, chunk management, and access control semantics.

## Architecture

**Layer 2** - Domain specification crate that depends only on `aura-core`.

- ✅ Storage domain types and semantics
- ✅ Content-addressed chunk management
- ✅ Search query types and filtering
- ✅ Storage CRDT operations
- ❌ NO effect handlers (use `StorageEffects` from `aura-effects`)
- ❌ NO handler composition (that's `aura-composition`)
- ❌ NO multi-party coordination (that's `aura-protocol`)

## Storage Authorization - MOVED ⚠️

**Storage authorization has moved to `aura-authorization`** as part of the authorization unification effort.

### Migration Guide

**Old import** (deprecated):
```rust
use aura_store::{BiscuitStorageEvaluator, StoragePermission};
```

**New import**:
```rust
use aura_wot::{
    BiscuitStorageEvaluator,
    StoragePermission,
    StorageResource,
    check_biscuit_access,
    evaluate_biscuit_access,
};
```

### Why the Move?

Authorization logic belongs in the `aura-authorization` (Web of Trust) crate to maintain proper domain separation:
- `aura-store`: Pure storage semantics (chunks, manifests, indices)
- `aura-authorization`: Authorization and capability evaluation (Biscuit tokens, permissions)

This eliminates circular dependencies and improves architectural clarity.

### See Also

- [`docs/109_authorization.md`](../../docs/109_authorization.md) - Complete authorization guide
- [`aura-authorization` crate](../aura-authorization/) - Authorization implementation

## Core Concepts

### Content Addressing

All stored data is content-addressed using cryptographic hashes:

```rust
use aura_store::{ChunkId, ContentId, compute_chunk_layout};

// Content is split into chunks with erasure coding
let layout = compute_chunk_layout(file_size, erasure_config);
```

### Storage Capabilities

Access to storage is controlled through capability-based permissions:

```rust
use aura_store::{StorageCapability, StoragePermission, StorageResource};

// Define what operations are allowed
let capability = StorageCapability {
    resource: StorageResource::Path("/data/documents".into()),
    permissions: vec![StoragePermission::Read, StoragePermission::Write],
};
```

### Search and Indexing

Storage supports efficient search through indexed metadata:

```rust
use aura_store::{SearchQuery, SearchScope};

let query = SearchQuery {
    terms: vec!["document".into(), "project".into()],
    scope: SearchScope::AuthorityScoped(authority_id),
};
```

## What's NOT in this Crate

- **Effect handlers**: Actual filesystem operations live in `aura-effects`
- **Coordination**: Multi-party storage protocols live in `aura-protocol`
- **Authorization**: Biscuit-based access control now lives in `aura-authorization`
- **Runtime**: Storage system assembly lives in `aura-composition` and `aura-agent`

## API Overview

### Chunk Management

- `ChunkLayout` - Content chunking configuration
- `ChunkManifest` - Chunk metadata and reconstruction info
- `ContentManifest` - Complete content metadata
- `ErasureConfig` - Erasure coding parameters

### Access Control

- `StorageCapability` - Permission definitions (pure semantics)
- `StoragePermission` - Read, Write, Delete operations
- `StorageResource` - Path-based or chunk-based resources
- `AccessDecision` - Allow/Deny with reasons

**Note**: Biscuit token evaluation now in `aura-authorization`.

### Search

- `SearchQuery` - Query structure with terms and filters
- `SearchResults` - Search result metadata
- `SearchIndexEntry` - Indexed content metadata
- `SearchScope` - Authority or context-scoped search

### CRDT State

- `StorageState` - Replicated storage state
- `StorageIndex` - Content index CRDT
- `StorageOpLog` - Operation log for synchronization

## Usage Example

```rust
use aura_store::{
    compute_chunk_layout, ErasureConfig,
    StorageCapability, StoragePermission, StorageResource,
};
use aura_core::AuthorityId;

// Configure chunking with erasure coding
let config = ErasureConfig {
    data_shards: 4,
    parity_shards: 2,
};

let layout = compute_chunk_layout(1024 * 1024, config); // 1MB file

// Define storage permissions (evaluation in aura-authorization)
let capability = StorageCapability {
    resource: StorageResource::Path("/documents/report.pdf".into()),
    permissions: vec![StoragePermission::Read],
};
```

## Documentation

- [Authorization](../../docs/109_authorization.md) - Biscuit token system (in aura-authorization)
- [Architecture](../../docs/001_system_architecture.md) - Layer 2 domain patterns

## Testing

```bash
cargo test --package aura-store
```

## License

See the LICENSE file in the repository root.
