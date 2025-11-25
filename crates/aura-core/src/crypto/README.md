# Aura Cryptographic Primitives

This module contains core cryptographic primitives used throughout Aura.

## Threshold Signature Abstraction Status

### Current State: FROST Only

Aura currently uses FROST (Flexible Round-Optimized Schnorr Threshold signatures) directly through the `tree_signing` module. We explicitly **defer** creating a generic `ThresholdSigEffects` trait.

### YAGNI Gate

We will introduce a `ThresholdSigEffects` abstraction only if:

1. **Second scheme required**: Another threshold signature scheme (e.g., BLS, RSA threshold) is needed in production
2. **FROST replacement**: Strategic decision to replace FROST with an alternative scheme
3. **Never for testing**: Use mock FROST implementations in tests rather than abstracting the trait

### Why Direct FROST Usage

- **Simplicity**: One less abstraction layer to maintain
- **Performance**: No dynamic dispatch overhead
- **Clarity**: Code explicitly shows we use FROST
- **YAGNI**: No current need for multiple schemes

### Expected Interface (Future)

If abstraction becomes necessary, the interface would look like:

```rust
#[async_trait]
trait ThresholdSigEffects {
    type GroupPublicKey;
    type Signature;
    type SignatureShare;
    
    /// Verify a threshold signature
    async fn verify(
        &self,
        ctx: &EffectContext,
        group_key: &Self::GroupPublicKey,
        msg: &[u8],
        sig: &Self::Signature,
    ) -> Result<bool, AuraError>;
    
    /// Generate nonce commitments (for pipelining)
    async fn precompute_nonce(
        &self,
        ctx: &EffectContext,
    ) -> Result<(NonceCommitment, NonceToken), AuraError>;
    
    /// Sign with precomputed nonce
    async fn sign_with_nonce(
        &self,
        ctx: &EffectContext,
        group_key: &Self::GroupPublicKey,
        msg: &[u8],
        my_nonce: NonceToken,
        peer_commitments: &[NonceCommitment],
    ) -> Result<Self::SignatureShare, AuraError>;
    
    /// Aggregate signature shares
    fn aggregate(
        &self,
        group_key: &Self::GroupPublicKey,
        msg: &[u8],
        shares: &[Self::SignatureShare],
    ) -> Result<Self::Signature, AuraError>;
}
```

Until then, consensus and other systems use the FROST API directly through `tree_signing`.

### Testing Approach

For testing threshold signatures:

```rust
// Use concrete FROST with deterministic randomness
let mut rng = StdRng::seed_from_u64(12345);
let (group_key, shares) = frost::keygen(&mut rng, threshold, participants);

// Mock at the consensus level, not the crypto level
struct MockConsensus {
    pre_agreed_result: CommitFact,
}
```

### Migration Path (If Needed)

If we ever need to abstract:

1. Create trait in `aura-core::effects::crypto`
2. Move FROST implementation to `aura-effects`
3. Update call sites to use trait
4. Add second implementation
5. Use feature flags or runtime selection

But until that day comes, **keep it simple**.

## Modules

- `tree_signing.rs` - FROST primitives for commitment tree operations
- `keys.rs` - Key derivation and management
- `hash.rs` - Cryptographic hash functions