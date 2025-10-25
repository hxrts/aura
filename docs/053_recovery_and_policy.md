# 050 · Recovery & Policy (Phase 1)

## 1. Recovery Model

- **Guardians**: trusted contacts holding recovery-only shares. Default target: 2-of-3 (user’s secondary device + 2 guardians).
- **Cooldown**: minimum 24 hours between guardian approval and share issuance. Stored in CRDT to allow cancellation.
- **Flow**:
  1. User requests recovery from UI; event recorded in CRDT.
  2. Guardians approve via DeviceAgent, signing `RecoveryApproval` events after decrypting their `RecoveryShareEnvelope`.
  3. Cooldown counter ticks; user can issue `RecoveryCancel`.
  4. After cooldown, MPC orchestrator issues new device share, re-encrypts guardian envelopes from the refreshed polynomial, and the session epoch is bumped to invalidate old presence tickets.
- **Out-of-scope for MVP**: service signer co-signatures, multi-stage verifications, biometrics on guardian side.

## 2. Policy Engine (Cedar-lite)

- Cedar policies stored as manifests with versioning (referenced in CRDT).
- MVP rule set covers:
  - Threshold requirements (`default: 2-of-3`).
  - Device posture (e.g., “native or guardian required for recovery”).
  - Rate limits (expressed as cooldown durations).
- Evaluation occurs during high-impact operations (add device, recovery init, threshold change).
- Resulting decision is cached in the CRDT as `PolicySnapshot` to avoid re-evaluation on hot paths.

### Example Policy Snippet

```cedar
permit (
    principal in Device::"native",
    action   in Action::["initiate_recovery"],
    resource == Account::"self"
) when {
    context.guardian_approvals >= 2 &&
    context.cooldown_seconds >= 86400
};
```

## 3. Capability Tokens (Biscuit)

- Generated automatically from Cedar decisions when an operation needs to be delegated to another participant (e.g., transport replication).
- Encodes:
  - `account_id`
  - `chunk_cid`
  - `expiry`
  - `allowed_action`
- Consumer verifies Biscuit using account public key; policy isn’t re-evaluated.

### Mapping Cedar → Biscuit

1. Policy engine approves a request and returns `CapabilityContext`.  
2. DeviceAgent serializes Biscuit token with relevant facts/constraints.  
3. Token shared with transport/cache peers; stored in manifest if long-lived.  
4. On revocation, publish manifest update referencing revoked capability hash (simple denylist).

## 4. Presence Tickets Recap

- Issued per derived identity (`PresenceTicket`).  
- Required to initiate any transport session.  
- Automatically invalidated on epoch bump or expiry.  
- Prevents leaked keys probing peers post-recovery.

## 5. Implementation Checklist

- [ ] Cedar policy parser + evaluator with small standard library.  
- [ ] `PolicySnapshot` struct persisted alongside CRDT entries.  
- [ ] Biscuit schema + signing utility.  
- [ ] Recovery UI flow + guardian notifications.  
- [ ] MPC integration: reshape shares after cooldown.  
- [ ] Guardian envelope rotation + manifest version enforcement.  
- [ ] Epoch bump + ticket reissue hook post-recovery.

Future work (post Phase 1) may add richer policies (service signers, multi-factor), but the above is sufficient to ship a coherent recovery story with minimal complexity.***
