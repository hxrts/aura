# Distributed Protocol Development

Use this guide for developing, testing, and deploying distributed applications and choreographies in Aura.

## Choreographic Programming Patterns

**Basic Protocol Structure:**
```rust,ignore
use aura_macros::choreography;

choreography! {
    #[namespace = "my_protocol"]
    protocol MyProtocol {
        roles: Alice, Bob;

        Alice[guard_capability = "my_protocol:send_request", flow_cost = 100]
        -> Bob: Request(RequestData);

        Bob[guard_capability = "my_protocol:respond", flow_cost = 50, journal_facts = "response_sent"]
        -> Alice: Response(ResponseData);
    }
}
```

**Advanced Patterns:**
- Use choice constructs for branching: `choice Alice { accept: {...} reject: {...} }`
- Add loops for iterative protocols: `loop (count: 5) { ... }`
- Support dynamic roles: `roles: Coordinator, Workers[N];`
- Compose protocols sequentially or in parallel

## Effect System Integration

**Create Protocol-Specific Effect Traits:**
```rust,ignore
use aura_core::effects::{
    CryptoEffects, NetworkEffects, PhysicalTimeEffects, RandomEffects, StorageEffects,
};

pub trait MyProtocolEffects:
    NetworkEffects + StorageEffects + CryptoEffects + PhysicalTimeEffects + RandomEffects {}
impl<T> MyProtocolEffects for T where
    T: NetworkEffects + StorageEffects + CryptoEffects + PhysicalTimeEffects + RandomEffects {}

pub async fn execute_protocol<E: MyProtocolEffects>(
    ctx: &EffectContext,
    effects: &E,
) -> Result<(), TimeError> {
    let _timestamp = effects.physical_time().await?;
    let _nonce = effects.random_bytes(32).await;
    Ok(())
}
```

**Handler Selection:**
- Production: `AgentBuilder::new().with_authority(...).build_production(&ctx).await`
- Testing: `AuraEffectSystem::simulation_for_named_test_with_salt(&config, "test_name", salt)`
- Simulation: `SimulationEffectComposer::for_testing(device_id).await`

## Security and Privacy Patterns

**Guard Chain Integration:**
- Authorization: `guard_capability = "my_protocol:request"` at the choreography DSL boundary; first-party Rust code should use typed capability families or `capability_name!`
- Flow budgets: `flow_cost = 100`
- Journal facts: `journal_facts = "event_name"`
- Leakage budgets: `leak = "observer_class"`

**Authority and Context Management:**
- Derive context IDs explicitly (e.g., `ContextId::new_from_entropy(hash(label))`)
- Signature verification lives in `aura-signature` or `CryptoEffects`
- Capability evaluation is enforced by CapGuard/Biscuit policies

## CRDT Integration

**Fact-Based Journals:**
- Facts accumulate via join operations (union)
- Capabilities refine via meet operations (intersection)
- Version vectors track causal ordering

## Error Handling and Resilience

**Retry / Circuit Breaker (ReliabilityEffects):**
```rust,ignore
use aura_core::effects::ReliabilityEffects;
use std::time::Duration;

let _ = effects
    .with_retry(
        || async { protocol_operation().await },
        3,
        Duration::from_millis(50),
        Duration::from_secs(1),
    )
    .await?;

let _ = effects
    .with_circuit_breaker(
        || async { protocol_operation().await },
        "my-protocol",
        5,
        Duration::from_secs(10),
    )
    .await?;
```

## Common Patterns

**Protocol Composition:**
- Sequential: Chain protocols with dependency flow
- Parallel: Execute multiple protocols concurrently
- Hierarchical: Use coordinator hierarchies for scale

**Dynamic Role Management:**
- Support runtime-determined participant counts
- Handle membership changes gracefully
- Use commitment trees for forward secrecy

## Next Steps

- Start with Hello World protocol from `docs/801_hello_world_guide.md`
- Study coordination patterns in `docs/803_choreography_guide.md`
- Use testing infrastructure from `docs/804_testing_guide.md`
