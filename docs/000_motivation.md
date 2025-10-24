# 010 · Motivation & Goals

## Why Aura?

Current identity systems force an impossible choice: trust a single device that
can be lost or compromised, or trust a corporation that can lock you out. Aura
rejects both. Instead, it builds on the trust you already have: friends, family,
and your own devices working together through threshold cryptography and social
recovery. No single point of failure, no corporate gatekeeper, just the people
and devices you actually trust.

| Pain Point                         | Desired Outcome                          |
|-----------------------------------|-------------------------------------------|
| Single-device compromise          | Threshold identity: no lone device can act |
| Password resets & SMS takeovers   | Guardian recovery grounded in real trust   |
| Cloud lock-in                     | Self-hosted, portable identity & storage   |
| Offline/unreliable connectivity   | Works peer-to-peer; no central coordinator |
| Impersonal global identity systems| Privacy-first social graph, invitation-based |

## MVP Success Criteria

We consider the simplified MVP “shipped” when:

1. **Identity Core**  
   - Threshold Ed25519 key (FROST) is live as the sole root for the prototype.  
   - Deterministic key derivation (DKD) issues app-specific keys via a one-line helper.  
   - Session epoch + presence tickets ensure leaked keys cannot probe active devices.  
   - Each account operates as a private, single-member DAO with role-based membership.

2. **Authenticated CRDT Ledger**  
   - Account state (devices, guardians, policies, session epoch) is replicated through a signed Automerge CRDT.  
   - Threshold-signed events guard high-impact changes; device-scoped signatures cover high-churn fields.  
   - Private social graph: relationships between accounts remain confidential.

3. **Storage MVP**  
   - One transport adapter (HTTPS relay) handles encrypted chunk exchange.  
   - Object manifests carry inline metadata for single-pass writes.  
   - Basic quota + LRU eviction keeps local storage bounded.  
   - Proof-of-storage challenge verifies replica claims.

4. **Recovery & Policy**  
   - Guardian invitation via out-of-band channels (Signal, QR codes) grounds trust in real connections.  
   - Recovery with mandatory cooldown (48h) allows dispute/veto windows for safety.  
   - Cedar policies cover a minimal rule set (e.g., "require native device for high-risk ops").  
   - Biscuit capabilities are generated from Cedar decisions so transports can enforce them offline.

## Out of Scope (for MVP)

- Multiple transports (BitChat BLE mesh, WebRTC, etc.)  
- Erasure coding, opportunistic friend caching tiers  
- Complex policy constructs (service signers, fine-grained rate limits)  
- Full automation of cross-app integrations

These features stay documented in `docs2/` and resurface in later phases (_see
060_Phased_Roadmap_). For now we deliver the smallest coherent stack that
proves self-custody identity plus encrypted replication can work together.***
