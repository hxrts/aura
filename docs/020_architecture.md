# 020 · Architecture Overview

## Layered Stack

```
┌─────────────────────────────────────────────────────────────┐
│ Application Clients                                         │
│  - Wallets, BitChat-lite, secure notes                      │
│  - Call DeviceAgent + Storage APIs                          │
└──────────────▲──────────────────────────────────────────────┘
               │ identity, recovery, storage calls
┌──────────────┴──────────────────────────────────────────────┐
│ Trust & Identity Services                                   │
│  - Threshold signing/MPC orchestrator (interactive)         │
│  - Policy engine (Cedar) + capability generator (Biscuit)   │
│  - Presence ticket issuance                                 │
└──────────────▲──────────────────────────────────────────────┘
               │ authorized events / tickets / configs
┌──────────────┴──────────────────────────────────────────────┐
│ Authenticated CRDT Ledger (Automerge)                       │
│  - Devices, guardians, thresholds                           │
│  - Session epoch + cached presence tickets                  │
│  - Cooldown counters                                        │
│  - Stores threshold-signed events for high-impact changes   │
└──────────────▲──────────────────────────────────────────────┘
               │ references to manifests & events
┌──────────────┴──────────────────────────────────────────────┐
│ Content & Event Store                                       │
│  - Encrypted object manifests + chunks                      │
│  - Threshold-signed event blobs                             │
│  - Device certificates/capability cache                     │
└──────────────▲──────────────────────────────────────────────┘
               │ proofs & payloads over transport
┌──────────────┴──────────────────────────────────────────────┐
│ Transport Layer (MVP = single adapter)                      │
│  - HTTPS relay                                              │
│  - Noise/TLS-secured channels                               │
│  - Proof-of-storage challenge/response                      │
│  - Enforces PresenceTicket on handshake                     │
└──────────────▲──────────────────────────────────────────────┘
               │ sockets, QUIC, WebSocket, etc.
┌──────────────┴──────────────────────────────────────────────┐
│ Networking Substrate                                        │
│  - NAT traversal, retry/backoff                             │
│  - Connection pooling                                       │
└─────────────────────────────────────────────────────────────┘
```

## Key Principles

1. **Threshold everywhere it matters** – high-impact changes (device add, policy
   update, session epoch bump) are threshold-signed events before they touch the CRDT.
2. **Local-first** – CRDT provides offline writes that reconcile once peers connect.
3. **Single-transport MVP** – we pick one battle-tested transport for Phase 1; others wait.
4. **Inline metadata** – one manifest per logical write keeps latency predictable.
5. **Capability gating** – presence tickets + Cedar/Biscuit ensure only authorised peers can initiate sessions or replicate data.

## Data Flow Highlights

1. **Identity Derivation** – application calls `derive_simple_identity(app_id, context)`. DeviceAgent runs DKD, caches result, issues presence ticket.
2. **CRDT Update** – Device adds event `AddGuardian(Bob)` by collecting threshold signatures; event is gossiped and verified before apply.
3. **Storage Write** – Device encrypts payload, packages inline metadata, creates manifest, pushes chunks via transport, challenges replicas, and records success in CRDT/metadata indexes.
4. **Recovery** – Guardian approvals + cooldown recorded in CRDT; once complete, MPC orchestrator issues new shares and bumps session epoch (revoking old tickets).

Each layer is swappable later (extra transports, richer policy) without revisiting the MVP contracts defined here.***
