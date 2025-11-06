# Effects System Enforcement

This document explains the lint enforcement system that ensures consistent use of the injected effects system throughout the Aura codebase.

## Overview

Aura uses an injected effects system for deterministic testing and choreographic programming. This system provides controlled access to:
- **Time**: `effects.now()` instead of `SystemTime::now()`
- **Randomness**: `effects.random_bytes()` instead of `rand::random()`
- **UUIDs**: `effects.gen_uuid()` instead of `Uuid::new_v4()`

To enforce these patterns, we use Clippy lints that deny direct usage of non-deterministic functions.

## Enforcement Rules

### Denied Methods
The following methods are prohibited in the codebase:

```rust
// Time functions
std::time::SystemTime::now()
std::time::Instant::now()
chrono::Utc::now()

// Random functions  
rand::random()
rand::thread_rng()
rand::rngs::OsRng::new()

// UUID functions
uuid::Uuid::new_v4()
uuid::Builder::from_random_bytes()
```

### Denied Types
These types should not be used directly:
```rust
rand::rngs::OsRng
rand::rngs::ThreadRng
```

## Correct Usage Patterns

### Effects-Based (Preferred)
```rust
fn create_session(effects: &impl aura_protocol::CryptoEffects) -> Session {
    Session {
        id: effects.gen_uuid(),           // [VERIFIED] Deterministic UUID
        created_at: effects.now().unwrap_or(0), // [VERIFIED] Controlled time
        nonce: effects.random_bytes::<32>(), // [VERIFIED] Seeded randomness
    }
}

#[cfg(test)]
fn test_session_creation() {
    let effects = aura_protocol::AuraEffectSystem::for_test("session_test");
    let session = create_session(&effects);
    // Test is deterministic and reproducible
}
```

### Direct Usage (Prohibited)
```rust
fn create_session_bad() -> Session {
    Session {
        id: Uuid::new_v4(),              // [NOT IMPLEMENTED] Lint error!
        created_at: SystemTime::now()   // [NOT IMPLEMENTED] Lint error!
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        nonce: rand::random(),           // [NOT IMPLEMENTED] Lint error!
    }
}
```

## Acceptable Exceptions

Some code legitimately needs direct access to system resources. These cases are marked with explicit `#[allow]` annotations:

### Production Effect Handler Implementation
```rust
impl TimeEffects for RealTimeHandler {
    async fn current_epoch(&self) -> u64 {
        #[allow(clippy::disallowed_methods)] // [VERIFIED] Acceptable in effect handlers
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }
}
```

### Default Implementations
```rust
impl Default for OperationId {
    fn default() -> Self {
        // Note: Default implementation uses non-deterministic UUID
        // Prefer using new_with_effects() for deterministic behavior
        #[allow(clippy::disallowed_methods)] // [VERIFIED] Documented exception
        Self(Uuid::new_v4())
    }
}
```

## Running Lint Checks

### Basic Clippy
```bash
just clippy
```

### Strict Effects Enforcement
```bash
just clippy-strict
```

### Full CI with Effects Enforcement
```bash
just ci
```

### Test Lint Enforcement
```bash
just lint-test  # Should fail on violations
```

## CI Integration

The GitHub Actions CI workflow includes:

1. **Format Check**: Ensures consistent code formatting
2. **Clippy Effects Enforcement**: Fails on any effects violations
3. **Test Suite**: Runs all tests with deterministic effects
4. **Pattern Search**: Uses `ripgrep` to catch missed violations
5. **Build Check**: Ensures all code compiles
6. **Lint Test**: Verifies the lint rules work correctly

## Migration Guide

### For New Code
Always use effects from the start:
```rust
fn new_function(effects: &impl aura_protocol::CryptoEffects) {
    let id = effects.gen_uuid();
    let timestamp = effects.now().unwrap_or(0);
    let random_bytes = effects.random_bytes::<32>();
}
```

### For Existing Code
1. Add `effects: &impl aura_protocol::EffectType` parameter
2. Replace direct calls with effects-based equivalents:
   - `SystemTime::now()` → `effects.now().unwrap_or(0)`
   - `Uuid::new_v4()` → `effects.gen_uuid()`
   - `rand::random()` → `effects.random_bytes()` or `effects.rng()`
3. Update all callers to pass effects
4. Add test with `aura_protocol::AuraEffectSystem::for_test("test_name")`

### Example Migration
```rust
// Before
fn old_function() -> Record {
    Record {
        id: Uuid::new_v4(),
        timestamp: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
    }
}

// After  
fn new_function(effects: &impl aura_protocol::CryptoEffects) -> Record {
    Record {
        id: effects.gen_uuid(),
        timestamp: effects.now().unwrap_or(0),
    }
}
```

## Benefits

This enforcement system provides:

- **Deterministic Testing**: All tests use controlled time/randomness
- **Reproducible Bugs**: Test failures can be reproduced with the same seed
- **Choreographic Testing**: Multi-participant protocols can be tested deterministically
- **Time Travel Debugging**: Tests can control time progression
- **Consistent Patterns**: All code follows the same dependency injection approach

## Configuration Files

- **`clippy.toml`**: Clippy configuration with disallowed methods/types
- **`Cargo.toml`**: Workspace-level lint configuration
- **`.github/workflows/ci.yml`**: CI enforcement workflow
- **`justfile`**: Development commands with lint enforcement

## Troubleshooting

### "Disallowed method" Error
```
error: use of a disallowed method `std::time::SystemTime::now`
```
**Solution**: Use `effects.now()` instead and add `effects: &impl aura_protocol::TimeEffects` parameter.

### "Missing effects parameter" 
When functions require effects but you don't have access:
1. Add effects parameter to your function
2. Thread effects through the call chain
3. At the top level, use `aura_protocol::AuraEffectSystem::for_production()` or `aura_protocol::AuraEffectSystem::for_test()`

### Legitimate System Access
If you genuinely need direct system access:
1. Add `#[allow(clippy::disallowed_methods)]` 
2. Document why this exception is necessary
3. Consider if the code can be moved to the effect handler implementation in `aura-protocol` instead
