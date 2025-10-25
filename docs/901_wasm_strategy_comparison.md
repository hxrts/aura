# WASM Build Strategy Comparison

**Date**: 2025-10-25  
**Purpose**: Compare vanity-miner-wasm (production) vs db-test (prototype) approaches

## Executive Summary

After analyzing the `feels-solana/vanity-miner-wasm` implementation, I've identified significant architectural differences between it and our current `db-test` prototype. The vanity-miner represents a **production-grade** approach with advanced optimization, while db-test is a **basic prototype** suitable for initial testing.

### Key Recommendation

**For Aura's distributed database**: Use the **vanity-miner architecture** as the blueprint, but start with the simpler db-test approach and progressively enhance it. The vanity-miner demonstrates patterns we'll need for production deployment.

---

## Detailed Comparison

| Aspect | db-test (Our Current) | vanity-miner-wasm (Production) |
|--------|----------------------|-------------------------------|
| **Complexity** | Basic prototype | Production-grade |
| **Build Config** | Simple wasm-pack | Advanced with cargo config, build-std |
| **Optimization** | Default | Aggressive (LTO, codegen-units=1, custom allocator) |
| **Web Integration** | Static HTML + simple JS | Worker pool coordinator + multiple deployment targets |
| **Parallelism** | Single-threaded only | Multi-threaded (manual workers + optional rayon) |
| **File Size** | ~34KB (unoptimized) | ~227KB (heavily optimized for performance) |
| **Allocator** | Default | wee_alloc (smaller binaries) |
| **Feature Flags** | None | SIMD, parallel, conditional compilation |
| **Testing** | Manual HTML test | Automated wasm-bindgen-test + integration tests |
| **Deployment** | Local server only | Production Next.js app integration |

---

## Architecture Analysis

### 1. Build Configuration

#### db-test (Basic)
```toml
[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
datafrog = "2.0"
wasm-bindgen = "0.2"
# ... minimal dependencies

[profile.release]
# Uses cargo defaults
```

**Build Command:**
```bash
wasm-pack build --target web --out-dir web/pkg
```

#### vanity-miner-wasm (Advanced)
```toml
[lib]
crate-type = ["cdylib"]  # WASM-only, no rlib needed

[features]
default = ["simd", "parallel"]
simd = []
parallel = []

[dependencies]
wee_alloc = "0.4"  # Smaller allocator
# ... optimized dependencies with pinned versions

[profile.release]
opt-level = 3              # Maximum optimization
lto = "fat"                # Link-time optimization
codegen-units = 1          # Single codegen unit for better optimization
panic = "abort"            # Smaller binary (no unwinding)
strip = true               # Strip debug symbols
overflow-checks = false    # Remove runtime checks

[package.metadata.wasm-pack.profile.release]
wasm-opt = ["-O2", "--enable-simd", "--enable-bulk-memory", "--enable-reference-types"]
```

**Build Command:**
```bash
wasm-pack build --release --target web --out-dir pkg \
    --no-default-features --features parallel
```

**Key Differences:**
- **LTO (Link-Time Optimization)**: Enables cross-crate inlining and dead code elimination
- **Single codegen unit**: Slower build but better optimization
- **wasm-opt flags**: Post-processing with Binaryen optimizer
- **Custom allocator**: `wee_alloc` reduces binary size by ~20-30%
- **Feature flags**: Conditional compilation for different deployment scenarios

### 2. Web Integration Strategy

#### db-test (Simple Static)
```
db-test/
  web/
    index.html          # Single HTML file
    pkg/                # WASM output
      aura_db_test.js
      aura_db_test_bg.wasm
```

**Loading Pattern:**
```javascript
import init, { DatafrogEngine } from "./pkg/aura_db_test.js";
await init();
const engine = new DatafrogEngine();
```

**Serving:**
```bash
python3 -m http.server 8000
```

#### vanity-miner-wasm (Production Multi-Target)
```
vanity-miner-wasm/
  src/
    lib.rs              # Core WASM logic
    test.rs             # Tests
  test.html             # Local development test
  justfile              # Build automation
  pkg/                  # Build output
    vanity_miner_wasm.js
    vanity_miner_wasm_bg.wasm
    snippets/           # Rayon support (if enabled)
  
  # Deployed to:
  feels-app/public/wasm/
    vanity_miner_wasm.js
    vanity_miner_wasm_bg.wasm
    vanity-coordinator.js      # Worker pool manager
    vanity-worker.js           # Web Worker wrapper
    vanity-worker-pool.js      # Pool utilities
```

**Loading Pattern (Simple):**
```javascript
import init from './vanity_miner_wasm.js';
await init();
```

**Loading Pattern (Multi-threaded):**
```javascript
import VanityMinerCoordinator from './vanity-coordinator.js';

const coordinator = new VanityMinerCoordinator();
await coordinator.initialize();

// Spawns N workers (N = CPU cores - 2)
// Each worker runs independent WASM instance
coordinator.start(suffix, onProgress, onFound);
```

**Key Differences:**
- **Worker Pool Architecture**: Spawns multiple Web Workers for true parallelism
- **Coordinator Pattern**: Central manager distributes work across workers
- **Pre-compilation**: WASM can be pre-compiled and shared across workers
- **Production Integration**: Deployed into Next.js app's public directory
- **Automated Build**: Justfile automates build → copy → verify workflow

### 3. Parallelism Strategy

#### db-test (None)
- Single-threaded execution only
- All Datafrog queries run in main thread
- No worker support
- Suitable for lightweight queries

#### vanity-miner-wasm (Dual Strategy)

**Strategy A: Manual Worker Pool (Current)**
```javascript
// Coordinator manages N workers
class VanityMinerCoordinator {
  constructor() {
    this.workerCount = navigator.hardwareConcurrency - 2;
    this.workers = [];
  }
  
  async initialize() {
    // Create N workers in batches
    for (let i = 0; i < this.workerCount; i++) {
      const worker = new Worker('/wasm/vanity-worker.js', { type: 'module' });
      this.workers.push(worker);
    }
  }
  
  start(suffix) {
    // Distribute work across workers
    this.workers.forEach(worker => {
      worker.postMessage({ command: 'mine', suffix });
    });
  }
}
```

**Benefits:**
- Works in all browsers (no special headers)
- Independent worker failures don't crash everything
- Fine-grained control over task distribution
- No SharedArrayBuffer requirement

**Strategy B: Rayon Threading (Available, Not Used)**
```rust
// lib.rs - Built with rayon support
#[wasm_bindgen]
pub fn mine_vanity_parallel(suffix: &str) -> JsValue {
    // Uses rayon thread pool for parallel processing
    (0..batch_size).into_par_iter()
        .find_map(|_| try_generate_vanity(suffix))
}
```

```javascript
import init, { init_thread_pool, mine_vanity_parallel } from './wasm.js';

await init();
await init_thread_pool(navigator.hardwareConcurrency);
const result = mine_vanity_parallel(suffix);
```

**Requirements:**
- Server must set COOP/COEP headers:
  ```
  Cross-Origin-Opener-Policy: same-origin
  Cross-Origin-Embedder-Policy: require-corp
  ```
- SharedArrayBuffer support
- `crossOriginIsolated === true`

**Why Manual Workers Are Used:**
1. Better browser compatibility
2. Easier debugging
3. More control over task distribution
4. Can add rayon later without changing architecture

### 4. Build Optimization Techniques

#### db-test
```toml
# No special optimizations
[profile.release]
# Uses cargo defaults
```

**Binary Size:** ~34KB (unoptimized)
**Performance:** Good enough for prototype

#### vanity-miner-wasm
```toml
[profile.release]
opt-level = 3              # -O3 optimization
lto = "fat"                # Cross-crate optimization
codegen-units = 1          # Maximum inlining
panic = "abort"            # No unwinding metadata
strip = true               # Remove debug symbols
overflow-checks = false    # Remove safety checks (use with caution!)

[package.metadata.wasm-pack.profile.release]
wasm-opt = [
    "-O2",                      # Binaryen optimization level 2
    "--enable-simd",            # SIMD instructions (faster)
    "--enable-bulk-memory",     # Bulk memory operations
    "--enable-reference-types"  # Reference types support
]
```

**Additional Optimizations:**
```rust
#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

#[cfg(target_arch = "wasm32")]
use std::arch::wasm32::*;  // SIMD intrinsics
```

**Binary Size:** ~227KB (heavily optimized for performance, not size)
**Performance:** 50,000+ attempts/sec (computationally intensive)

**Optimization Impact:**

| Technique | Binary Size Impact | Performance Impact |
|-----------|-------------------|-------------------|
| LTO "fat" | -10% to -20% | +15% to +30% |
| codegen-units = 1 | -5% to -10% | +10% to +20% |
| wee_alloc | -20% to -30% | -5% to +5% (slight overhead) |
| wasm-opt -O2 | -10% to -15% | +5% to +15% |
| SIMD | No change | +50% to +200% (algorithm-dependent) |
| overflow-checks = false | -2% to -5% | +5% to +10% |

**Trade-offs:**
- `overflow-checks = false`: Faster but removes safety nets (acceptable for well-tested crypto code)
- `wee_alloc`: Smaller but slightly slower allocations (worth it for WASM)
- `codegen-units = 1`: Much slower builds but better optimization
- SIMD: Requires modern browsers but massive speedups for vectorizable code

### 5. Testing Strategy

#### db-test
```html
<!-- test.html - Manual testing only -->
<script type="module">
    import init, { DatafrogEngine } from "./pkg/aura_db_test.js";
    await init();
    const engine = new DatafrogEngine();
    const results = engine.run_transitive_closure();
    console.log(results);
</script>
```

**Testing Method:**
1. Open test.html in browser
2. Click buttons
3. Manually verify output

#### vanity-miner-wasm
```rust
// src/test.rs - Automated WASM tests
#[cfg(test)]
mod tests {
    use super::*;
    use wasm_bindgen_test::*;

    wasm_bindgen_test_configure!(run_in_browser);

    #[wasm_bindgen_test]
    fn test_vanity_generation() {
        let mut miner = VanityMiner::new("TEST".to_string());
        let result = miner.mine_batch32(1000);
        // ... assertions
    }

    #[wasm_bindgen_test]
    fn test_benchmark() {
        let rate = benchmark_single_thread(100);
        assert!(rate > 0.0);
    }
}
```

**Testing Methods:**
1. **Unit tests**: `wasm-pack test --headless --chrome`
2. **Manual HTML test**: `test.html` for visual verification
3. **Integration tests**: Test pages in Next.js app
4. **Automated verification**: `just test-integration` checks all files exist

**justfile automation:**
```bash
# Run tests
test:
    wasm-pack test --headless --chrome

# Test integration
test-integration:
    # Verify all WASM files are in place
    # Check file sizes, exports, etc.
```

### 6. Deployment Integration

#### db-test
```bash
# Build
cd crates/db-test
wasm-pack build --target web --out-dir web/pkg

# Serve locally
cd web
python3 -m http.server 8000

# Open browser
open http://localhost:8000
```

**Manual steps:**
1. Build WASM
2. Start server
3. Test in browser

#### vanity-miner-wasm
```bash
# Build and deploy to Next.js app
cd vanity-miner-wasm
just build

# Automatically:
# 1. Builds WASM with release profile
# 2. Copies to ../feels-app/public/wasm/
# 3. Copies worker scripts
# 4. Copies snippets (for rayon support)
# 5. Verifies all files are in place

# Integration test
just test-integration

# Next.js app automatically serves from public/wasm/
cd ../feels-app
npm run dev
# WASM available at http://localhost:3000/wasm/vanity_miner_wasm.js
```

**Automated Workflow:**
```bash
# justfile handles everything
build:
    wasm_out="../feels-app/public/wasm"
    wasm-pack build --release --target web --out-dir pkg
    cp pkg/vanity_miner_wasm* "$wasm_out/"
    cp -r pkg/snippets "$wasm_out/"
    echo "Build complete and integrated into feels-app"
```

**Benefits:**
- One command builds and deploys
- Automatic file verification
- Integration with Next.js build process
- No manual file copying

---

## Recommendations for Aura

### Phase 1: Prototype (Current - db-test)
**Keep the simple approach for now:**
- ✅ Basic wasm-pack build
- ✅ Simple static HTML test page
- ✅ Single-threaded Datafrog queries
- ✅ Manual testing

**Good for:**
- Validating Datafrog works in WASM
- Testing query patterns
- Experimenting with CRDT + Datalog integration

### Phase 2: Optimization (2-4 weeks)
**Adopt vanity-miner build optimizations:**

```toml
# crates/db-test/Cargo.toml
[lib]
crate-type = ["cdylib"]  # WASM-only

[dependencies]
wee_alloc = "0.4"
# ... existing dependencies

[profile.release]
opt-level = 3
lto = "fat"
codegen-units = 1
panic = "abort"
strip = true

[package.metadata.wasm-pack.profile.release]
wasm-opt = ["-O2", "--enable-bulk-memory"]
```

**Add to lib.rs:**
```rust
#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;
```

**Expected improvements:**
- 20-30% smaller binary
- 15-25% faster execution
- Better production readiness

### Phase 3: Production Integration (1-2 months)
**Adopt vanity-miner deployment patterns:**

1. **Create justfile automation:**
```bash
# justfile
build-wasm-db:
    wasm-pack build --release --target web --out-dir pkg
    cp pkg/aura_db_test* ../frontend/public/wasm/
    just verify-wasm-integration

verify-wasm-integration:
    # Check all files exist
    # Verify exports
    # Size checks
```

2. **Add automated tests:**
```rust
#[cfg(test)]
mod tests {
    use wasm_bindgen_test::*;
    
    #[wasm_bindgen_test]
    fn test_transitive_closure() {
        // ... test Datafrog queries
    }
}
```

3. **Integration with Aura frontend:**
```
aura-frontend/
  public/
    wasm/
      aura_db.js
      aura_db_bg.wasm
  components/
    DatalogProvider.tsx
```

### Phase 4: Multi-threading (3-6 months)
**When we need high-performance queries:**

1. **Implement worker pool pattern:**
```javascript
// datafrog-coordinator.js
class DatafrogCoordinator {
  constructor() {
    this.workers = [];
    this.workerCount = navigator.hardwareConcurrency - 1;
  }
  
  async runQuery(query) {
    // Distribute query across workers
    // Merge results
  }
}
```

2. **Or use rayon threading:**
```rust
#[wasm_bindgen]
pub fn query_parallel(query: &str) -> JsValue {
    // Use rayon for parallel query execution
}
```

**When to use which:**
- **Worker pool**: General-purpose, works everywhere
- **Rayon**: Maximum performance, requires COOP/COEP headers

### Phase 5: Advanced Features (6-12 months)
**Production-grade enhancements:**

1. **Query caching and incremental computation:**
```rust
pub struct DatafrogCache {
    // Cache query results
    // Incremental updates when CRDT changes
}
```

2. **SIMD optimization for specific queries:**
```rust
#[cfg(target_arch = "wasm32")]
use std::arch::wasm32::*;

// Vectorized graph traversal
```

3. **Streaming results for large queries:**
```rust
#[wasm_bindgen]
pub fn query_stream(query: &str, callback: &js_sys::Function) {
    // Stream results as they're computed
}
```

---

## Decision Matrix: Which Approach When?

| Use Case | Recommended Approach | Rationale |
|----------|---------------------|-----------|
| **Initial prototyping** | db-test (simple) | Fast iteration, easy debugging |
| **Production MVP** | vanity-miner build config | Better performance, smaller binaries |
| **High-volume queries** | Worker pool | True parallelism, good compatibility |
| **Maximum performance** | Rayon threading | Best CPU utilization, lowest overhead |
| **Browser compatibility** | Worker pool | Works everywhere, no special headers |
| **Mobile web** | Optimized single-thread | Lower memory, battery-friendly |
| **Desktop browsers** | Multi-threaded | Take advantage of available cores |

---

## Build Configuration Recommendations

### Immediate (Now)
```toml
# Keep current simple config
# Focus on functionality first
```

### Short-term (2-4 weeks)
```toml
[profile.release]
opt-level = 3
lto = "thin"  # Start with thin LTO (faster builds)
strip = true

[package.metadata.wasm-pack.profile.release]
wasm-opt = ["-O1"]  # Basic optimization
```

### Medium-term (2-3 months)
```toml
[profile.release]
opt-level = 3
lto = "fat"
codegen-units = 1
panic = "abort"
strip = true
overflow-checks = false  # Only after thorough testing

[package.metadata.wasm-pack.profile.release]
wasm-opt = ["-O2", "--enable-bulk-memory"]
```

### Long-term (6+ months)
```toml
# Full vanity-miner optimization
+ SIMD features
+ Worker pool architecture
+ Rayon threading support
+ Feature flags for different builds
```

---

## Cost-Benefit Analysis

### Build Optimization

| Optimization | Build Time Impact | Runtime Benefit | Implementation Effort |
|--------------|------------------|-----------------|----------------------|
| LTO "thin" | +20% | +10-15% | 1 line in Cargo.toml |
| LTO "fat" | +100% | +20-30% | 1 line in Cargo.toml |
| codegen-units=1 | +50% | +10-20% | 1 line in Cargo.toml |
| wee_alloc | None | -20% binary size | 3 lines of code |
| wasm-opt -O2 | +30% | +10-15% | 1 line in Cargo.toml |
| SIMD | None | +50-200% (specific ops) | Moderate (algorithm-specific) |
| Worker pool | None | +N×85% (N=cores) | High (new architecture) |

### Recommended Priority Order

1. **wee_alloc** (instant win, no downside)
2. **wasm-opt -O1** (modest build time, good benefit)
3. **LTO "thin"** (reasonable build time, good benefit)
4. **strip = true** (instant, reduces binary size)
5. **LTO "fat"** (when build time is acceptable)
6. **codegen-units = 1** (when build time is acceptable)
7. **SIMD** (when specific algorithms benefit)
8. **Worker pool** (when parallelism is needed)

---

## Conclusion

**Current state:**
- db-test is a good prototype
- Validates Datafrog works in WASM
- Simple and easy to understand

**Next steps:**
1. Add basic optimizations (wee_alloc, wasm-opt -O1)
2. Implement automated build pipeline (justfile)
3. Add wasm-bindgen tests
4. Plan for production integration

**Long-term vision:**
- Adopt vanity-miner architecture patterns
- Multi-threaded query execution
- Production-grade optimization
- Seamless frontend integration

The vanity-miner implementation is an excellent blueprint for production WASM deployment. We should evolve our db-test incrementally toward this architecture as Aura's requirements grow.
