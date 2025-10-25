# Aura DB Test - Datafrog WASM Demo

This crate demonstrates Datafrog (a Datalog engine) compiled to WebAssembly for use in Aura's distributed database.

## Overview

Datafrog will power Aura's query layer, enabling efficient incremental queries over:
- Social graph relationships (friend-of-friend, neighborhood traversal)
- Capability delegation chains
- Access control policies
- Distributed data discovery

## Building

Make sure you're in the Nix development environment:

```bash
nix develop
```

Then build the WASM module:

```bash
just build-wasm-db-test
```

This compiles the Rust code to WebAssembly using `wasm-pack`.

## Running the Demo

After building, start the web server:

```bash
just serve-wasm-db-test
```

Or build and serve in one command:

```bash
just test-wasm-db
```

Then open your browser to http://localhost:8000

## What It Tests

The demo includes two Datalog queries:

### 1. Transitive Closure
Given a directed graph with edges A→B, B→C, C→D, A→C, computes all reachable pairs using fixed-point iteration.

This demonstrates how Aura can compute neighborhood relationships like "all users within N hops" efficiently.

### 2. Friend-of-Friend
Given friendship relationships, computes all friend-of-friend connections.

This demonstrates social graph queries that will be used for:
- Determining data visibility in neighborhoods
- Computing trust scores
- Discovering peers for replication

## Implementation Details

The code uses Datafrog's Rust API:
- `Iteration` for fixed-point computation
- `Relation` for storing facts
- `from_join` for relational joins
- Incremental computation for efficiency

All computation happens in WebAssembly in the browser, demonstrating that Datafrog's performance is suitable for client-side use.

## Future Integration

This test validates that Datafrog can:
1. Compile to WASM for browser environments
2. Perform graph queries efficiently
3. Handle incremental updates (critical for CRDT integration)

The next step is integrating Datafrog with the Journal CRDT to enable:
- Live queries over the account ledger
- Permission-aware data access
- Distributed indexing and discovery
