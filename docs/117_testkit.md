# Test Infrastructure Reference

This document describes the architecture of `aura-testkit`, the test infrastructure crate that provides fixtures, mock handlers, and verification utilities for testing Aura protocols.

## Overview

The `aura-testkit` crate occupies Layer 8 in the Aura architecture. It provides reusable test infrastructure without containing production code. All test utilities follow effect system guidelines to ensure deterministic execution.

The crate serves three purposes. It provides stateful effect handlers for controllable test environments. It offers fixture builders for consistent test setup. It includes verification utilities for property testing and differential testing.

## Stateful Effect Handlers

Stateful effect handlers maintain internal state across calls. They enable deterministic testing by controlling time, randomness, and storage. These handlers implement the same traits as production handlers but store state for inspection and manipulation.

### Handler Categories

The `stateful_effects` module provides handlers for each effect category.

| Handler | Effect Trait | Purpose |
|---------|--------------|---------|
| `SimulatedTimeHandler` | `PhysicalTimeEffects` | Controllable simulated time |
| `MockRandomHandler` | `RandomCoreEffects` | Seeded deterministic randomness |
| `MemoryStorageHandler` | `StorageEffects` | In-memory storage with inspection |
| `MockJournalHandler` | `JournalEffects` | Journal with fact tracking |
| `MockCryptoHandler` | `CryptoCoreEffects` | Crypto with key inspection |
| `MockConsoleHandler` | `ConsoleEffects` | Captured console output |

### Time Handler

The `SimulatedTimeHandler` provides controllable time for tests.

```rust
use aura_testkit::stateful_effects::SimulatedTimeHandler;
use aura_core::effects::PhysicalTimeEffects;

let time = SimulatedTimeHandler::new();
let now = time.physical_time().await?;
time.advance_time(5000);
let later = time.physical_time().await?;
```

This handler starts at the current system time by default. Use `SimulatedTimeHandler::new_at_epoch()` for tests starting at Unix epoch, or `SimulatedTimeHandler::new_with_time(start_ms)` for a specific start time. Tests can verify time-dependent behavior without wall-clock delays.

### Random Handler

The `MockRandomHandler` provides seeded randomness for reproducible tests.

```rust
use aura_testkit::stateful_effects::MockRandomHandler;

let random = MockRandomHandler::with_seed(42);
let bytes = random.random_bytes(32).await;
```

Given the same seed, this handler produces identical sequences across runs. This enables deterministic property testing and failure reproduction.

## Fixture System

The fixture system provides consistent test environment setup. Fixtures encapsulate common configuration patterns and reduce boilerplate.

### TestFixture

The `TestFixture` type provides a complete test environment.

```rust
use aura_testkit::infrastructure::harness::{TestFixture, TestConfig};

let fixture = TestFixture::new().await?;
let device_id = fixture.device_id();
let context = fixture.context();
```

A fixture creates deterministic identifiers, initializes effect handlers, and provides access to test context. The default configuration suits most unit tests.

### TestConfig

Custom configurations enable specialized test scenarios.

```rust
let config = TestConfig {
    name: "threshold_test".to_string(),
    deterministic_time: true,
    capture_effects: true,
    timeout: Some(Duration::from_secs(60)),
};
let fixture = TestFixture::with_config(config).await?;
```

The `deterministic_time` flag enables `StatefulTimeHandler`. The `capture_effects` flag records effect calls for later inspection.

## Builder Utilities

Builder functions create test data with deterministic inputs. They live in the `builders` module.

### Account Builders

```rust
use aura_testkit::builders::test_account_with_seed;

let account = test_account_with_seed(42).await;
```

This creates an account with deterministic keys derived from the seed. Multiple calls with the same seed produce identical accounts.

### Key Builders

```rust
use aura_testkit::builders::test_key_pair;

let (signing_key, verifying_key) = test_key_pair(1337);
```

Key pairs derive from the provided seed. This enables testing signature verification with known keys.

### Identifier Generation

Tests must use deterministic identifiers to ensure reproducibility.

```rust
use aura_core::identifiers::AuthorityId;

let auth1 = AuthorityId::from_entropy([1u8; 32]);
let auth2 = AuthorityId::from_entropy([2u8; 32]);
```

Never use `Uuid::new_v4()` or similar entropy-consuming methods in tests. Incrementing byte patterns create distinct but reproducible identifiers.

## Verification Utilities

The `verification` module provides utilities for property testing and differential testing.

### Proptest Strategies

The `strategies` module defines proptest strategies for Aura types.

```rust
use aura_testkit::verification::strategies::{arb_device_id, arb_account_id, arb_key_pair};
use proptest::prelude::*;

proptest! {
    #[test]
    fn device_id_deterministic(id in arb_device_id()) {
        assert_ne!(id.to_string(), "");
    }

    #[test]
    fn key_pair_valid((sk, vk) in arb_key_pair()) {
        assert_eq!(sk.verifying_key(), vk);
    }
}
```

Available strategies include `arb_device_id`, `arb_account_id`, `arb_session_id`, and `arb_key_pair`. These generate valid, deterministic instances for property testing.

### Lean Oracle

The `lean_oracle` module provides integration with Lean theorem proofs.

```rust
use aura_testkit::verification::lean_oracle::LeanOracle;

let oracle = LeanOracle::new()?;
let result = oracle.verify_journal_merge(&state1, &state2)?;
```

The oracle invokes compiled Lean code to verify properties. This enables differential testing against proven implementations.

### Capability Soundness

The `capability_soundness` module provides formal verification for capability system properties.

```rust
use aura_testkit::verification::capability_soundness::{
    CapabilitySoundnessVerifier, SoundnessProperty, CapabilityState
};

let mut verifier = CapabilitySoundnessVerifier::with_defaults();
let result = verifier.verify_property(
    SoundnessProperty::NonInterference,
    initial_state
).await?;
assert!(result.holds);
```

The verifier checks five soundness properties: `NonInterference`, `Monotonicity`, `TemporalConsistency`, `ContextIsolation`, and `AuthorizationSoundness`. Use `verify_all_properties` to check all properties at once.

## Consensus Testing

The `consensus` module provides infrastructure for consensus protocol testing.

### ITF Loader

The `itf_loader` module loads ITF traces for replay testing.

```rust
use aura_testkit::consensus::itf_loader::ITFLoader;

let trace = ITFLoader::load("traces/consensus_happy_path.itf.json")?;
for state in trace.states {
    // Verify state against implementation
}
```

ITF traces come from Quint model checking. The loader parses them into Rust types for conformance testing.

### Reference Implementation

The `reference` module provides a minimal consensus implementation for differential testing.

```rust
use aura_testkit::consensus::reference::ReferenceConsensus;

let reference = ReferenceConsensus::new(config);
let expected = reference.process_vote(vote)?;
let actual = production_consensus.process_vote(vote)?;
assert_eq!(expected.outcome, actual.outcome);
```

The reference implementation prioritizes clarity over performance. It serves as a specification against which production code is tested.

## Mock Runtime Bridge

The `MockRuntimeBridge` simulates the runtime environment for TUI testing.

```rust
use aura_testkit::mock_runtime_bridge::MockRuntimeBridge;

let bridge = MockRuntimeBridge::new();
bridge.inject_chat_update(chat_state);
bridge.inject_contact_update(contacts);
```

This bridge injects signals that would normally come from the reactive pipeline. It enables testing TUI state machines without a full runtime.

## Conformance Framework

The `conformance` module provides artifact validation for native/WASM parity testing.

### Artifact Format

The `AuraConformanceArtifactV1` captures execution state for comparison. The type is defined in `aura_core::conformance`:

```rust
use aura_core::{AuraConformanceArtifactV1, AuraConformanceRunMetadataV1, ConformanceSurfaceName};

let mut artifact = AuraConformanceArtifactV1::new(AuraConformanceRunMetadataV1 {
    target: "native".to_string(),
    profile: "native_coop".to_string(),
    scenario: "test_scenario".to_string(),
    seed: Some(42),
    commit: None,
    async_host_transcript_entries: None,
    async_host_transcript_digest_hex: None,
});

// Insert required surfaces
artifact.insert_surface(ConformanceSurfaceName::Observable, observable_surface);
artifact.insert_surface(ConformanceSurfaceName::SchedulerStep, scheduler_surface);
artifact.insert_surface(ConformanceSurfaceName::Effect, effect_surface);

artifact.validate_required_surfaces()?;
```

Every conformance artifact must capture three surfaces:

| Surface | Purpose | Content |
|---------|---------|---------|
| `Observable` | Protocol-visible outputs | Normalized message contents |
| `SchedulerStep` | Logical progression | Step index, session state, role progression |
| `Effect` | Effect envelope trace | Sequence of effect calls with arguments |

Missing surfaces cause validation failure.

Metadata aids debugging but does not affect comparison:

```rust
pub struct AuraConformanceRunMetadataV1 {
    pub target: String,
    pub profile: String,
    pub scenario: String,
    pub seed: Option<u64>,
    pub commit: Option<String>,
    pub async_host_transcript_entries: Option<usize>,
    pub async_host_transcript_digest_hex: Option<String>,
}
```

### Effect Envelope Classification

Each effect kind has a comparison class that determines how differences are evaluated:

| Effect Kind | Class | Comparison Rule |
|-------------|-------|-----------------|
| `send_decision` | `commutative` | Order-insensitive under normalization |
| `invoke_step` | `commutative` | Scheduler interleavings normalized |
| `handle_recv` | `strict` | Byte-exact match required |
| `handle_choose` | `strict` | Branch choice must match |
| `handle_acquire` | `strict` | Guard semantics must match |
| `handle_release` | `strict` | Guard semantics must match |
| `topology_event` | `algebraic` | Reduced via topology-normal form |

The `strict` class requires exact matches. The `commutative` class normalizes order before comparison. The `algebraic` class applies domain-specific reduction before comparison.

New effect kinds must be classified before use:

```rust
use aura_core::conformance::AURA_EFFECT_ENVELOPE_CLASSIFICATIONS;

AURA_EFFECT_ENVELOPE_CLASSIFICATIONS.insert(
    "new_effect_kind",
    ComparisonClass::Strict,
);

aura_core::assert_effect_kinds_classified(&effect_trace)?;
```

Unclassified effect kinds cause conformance checks to fail.

## Module Structure

```
aura-testkit/
├── src/
│   ├── builders/           # Test data builders
│   ├── configuration/      # Test configuration
│   ├── consensus/          # Consensus testing utilities
│   ├── conformance.rs      # Conformance artifact support
│   ├── differential.rs     # Differential testing
│   ├── fixtures/           # Reusable test fixtures
│   ├── foundation.rs       # Core test utilities
│   ├── handlers/           # Mock effect handlers
│   ├── infrastructure/     # Test harness infrastructure
│   ├── mock_effects.rs     # Simple mock implementations
│   ├── stateful_effects/   # Stateful effect handlers
│   └── verification/       # Property testing utilities
├── tests/                  # Integration tests
└── benches/                # Performance benchmarks
```

## Related Documentation

See [Testing Guide](804_testing_guide.md) for how to write tests using this infrastructure. See [Effect System](105_effect_system.md) for effect trait definitions.
