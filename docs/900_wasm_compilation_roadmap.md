# WASM Compilation Roadmap

## Executive Summary

This document provides a comprehensive analysis of what is required to compile the Aura project to WebAssembly (WASM). Based on a thorough codebase review, approximately 30-40% of the current codebase can be compiled to WASM with minimal changes (feature flags), while the remaining 60-70% requires architectural refactoring to abstract platform-specific dependencies.

**Current Status:** Core cryptographic and protocol logic is nearly WASM-ready. Storage, networking, and platform-specific secure storage require significant architectural changes.

**Timeline Estimate:** 2-3 quarters for full WASM support with proper abstractions.

---

## 1. Crate-by-Crate WASM Compatibility Assessment

### Tier 1: Fully WASM-Compatible (No Changes Required)

#### `aura-session-types`
**Status:** ✅ Ready for WASM compilation

- Pure Rust logic implementing session type infrastructure
- No platform-specific dependencies
- No file I/O, networking, or OS interactions
- Can compile to WASM today with `--target wasm32-unknown-unknown`

**Action Required:** None

#### `aura-groups`
**Status:** ✅ Ready for WASM compilation

- Pure cryptographic protocol (BeeKEM/MLS group messaging)
- Only depends on crypto primitives (all WASM-compatible)
- No platform-specific code
- Can compile to WASM today

**Action Required:** None

#### `aura-test-utils`
**Status:** ✅ Can be excluded from WASM builds

- Testing utilities only
- Not needed in production WASM builds
- Can be feature-gated for native-only compilation

**Action Required:** Add `#[cfg(not(target_arch = "wasm32"))]` gates if needed

---

### Tier 2: WASM-Compatible with Feature Flags

#### `aura-crypto`
**Status:** ⚠️ Needs feature flag configuration (95% compatible)

**Current Issues:**
1. Random number generation uses OS entropy:
   ```rust
   // crates/crypto/src/effects.rs
   pub struct OsRandomSource;
   impl RandomSource for OsRandomSource {
       fn fill_bytes(&self, dest: &mut [u8]) {
           rand::thread_rng().fill_bytes(dest); // Uses OS entropy
       }
   }
   ```

2. Time source uses system clock:
   ```rust
   // crates/crypto/src/effects.rs  
   pub struct SystemTimeSource;
   impl TimeSource for SystemTimeSource {
       fn now(&self) -> Result<u64, CryptoError> {
           std::time::SystemTime::now() // OS syscall
               .duration_since(UNIX_EPOCH)
               .map(|d| d.as_secs())
       }
   }
   ```

**Solutions:**
- Already has effects abstraction (`TimeSource`, `RandomSource` traits)
- Add WASM-specific implementations:
  ```toml
  [target.'cfg(target_arch = "wasm32")'.dependencies]
  getrandom = { version = "0.2", features = ["js"] }
  js-sys = "0.3"
  ```

- Implement `WasmRandomSource`:
  ```rust
  #[cfg(target_arch = "wasm32")]
  pub struct WasmRandomSource;
  
  #[cfg(target_arch = "wasm32")]
  impl RandomSource for WasmRandomSource {
      fn fill_bytes(&self, dest: &mut [u8]) {
          getrandom::getrandom(dest).expect("WASM random failed");
      }
  }
  ```

- Implement `WasmTimeSource`:
  ```rust
  #[cfg(target_arch = "wasm32")]
  pub struct WasmTimeSource;
  
  #[cfg(target_arch = "wasm32")]
  impl TimeSource for WasmTimeSource {
      fn now(&self) -> Result<u64, CryptoError> {
          let ms = js_sys::Date::now();
          Ok((ms / 1000.0) as u64)
      }
  }
  ```

**Action Required:**
1. Add WASM target dependencies to `Cargo.toml`
2. Implement WASM-specific sources
3. Add feature flag `wasm` to gate implementations
4. Update `Effects::new()` to use platform-appropriate sources

**Estimated Effort:** 1-2 days

#### `aura-journal`
**Status:** ⚠️ Mostly compatible, depends on aura-crypto

**Current Issues:**
- Depends on `aura-crypto` (needs fixes above)
- Uses `automerge` 0.5 which has WASM support
- No direct platform dependencies

**Dependencies:**
- `automerge` 0.5: ✅ Has WASM builds available
- `serde`, `serde_json`, `serde_cbor`: ✅ All WASM-compatible
- `uuid`: ⚠️ Needs `getrandom` with "js" feature (already in workspace)

**Action Required:**
1. Ensure `getrandom` feature "js" is enabled for WASM target
2. Test `automerge` WASM build compatibility
3. No code changes needed

**Estimated Effort:** 1 day testing

#### `aura-coordination`
**Status:** ⚠️ Needs Tokio abstraction (80% compatible)

**Current Issues:**
1. Uses `tokio::spawn` for task spawning:
   ```rust
   // crates/coordination/src/choreography/counter.rs
   tokio::spawn(async move {
       Self::poll_messages(...).await;
   });
   ```

2. Relies on Tokio runtime features not available in WASM:
   - `tokio::spawn` (no OS threads in browser)
   - `tokio::fs` (no filesystem in browser)
   - `tokio::time` (OS timer APIs not available)

**Solutions:**
- Use `wasm-bindgen-futures` for WASM async execution
- Feature-gate Tokio usage:
  ```toml
  [features]
  default = ["native"]
  native = ["tokio/full"]
  wasm = ["wasm-bindgen-futures"]
  
  [target.'cfg(not(target_arch = "wasm32"))'.dependencies]
  tokio = { workspace = true, features = ["full"] }
  
  [target.'cfg(target_arch = "wasm32")'.dependencies]
  wasm-bindgen-futures = "0.4"
  ```

- Abstract task spawning:
  ```rust
  #[cfg(not(target_arch = "wasm32"))]
  pub fn spawn_task<F>(future: F)
  where F: Future<Output = ()> + Send + 'static {
      tokio::spawn(future);
  }
  
  #[cfg(target_arch = "wasm32")]
  pub fn spawn_task<F>(future: F)
  where F: Future<Output = ()> + 'static {
      wasm_bindgen_futures::spawn_local(future);
  }
  ```

**Action Required:**
1. Create task spawning abstraction layer
2. Feature-gate all Tokio usage
3. Replace `tokio::test` with `wasm-bindgen-test` in WASM mode
4. Remove or gate `tokio::fs` and `tokio::time` usage

**Estimated Effort:** 1 week

---

### Tier 3: NOT WASM-Compatible (Architectural Changes Required)

#### `aura-agent`
**Status:** ❌ Requires major refactoring

**Blockers:**

1. **Platform-Specific Secure Storage** (`crates/agent/src/secure_storage.rs`):
   - macOS/iOS: `security-framework`, `core-foundation` crates
   - Linux: `keyutils` crate for kernel keyring
   - Windows: `windows` crate for Win32 Credential APIs
   - Fallback: File-based storage using `std::fs`
   - **None of these work in WASM**

   Current implementation:
   ```rust
   #[cfg(any(target_os = "macos", target_os = "ios"))]
   impl SecureStorageBackend for KeychainBackend { ... }
   
   #[cfg(target_os = "linux")]
   impl SecureStorageBackend for KeyutilsBackend { ... }
   
   #[cfg(target_os = "windows")]
   impl SecureStorageBackend for WindowsCredentialBackend { ... }
   
   #[cfg(not(any(...)))]
   impl SecureStorageBackend for FileBackend { ... }
   ```

2. **File System Operations**:
   - Configuration file reading/writing using `tokio::fs`
   - Device key storage in files
   - No equivalent in browser sandbox

3. **Platform Detection**:
   - Reads `/etc/machine-id` on Linux
   - Executes `system_profiler` on macOS
   - Reads environment variables for hostnames
   - All unavailable in WASM

**Required Architectural Changes:**

1. Extract core identity logic into `aura-identity-core`:
   ```
   aura-identity-core/
   ├── identity.rs      // Pure identity logic
   ├── derivation.rs    // Key derivation (no storage)
   ├── credentials.rs   // Credential management (pure logic)
   └── types.rs         // Core types
   ```

2. Create storage abstraction:
   ```rust
   pub trait SecureStorageBackend {
       async fn store_key(&self, id: &str, data: &[u8]) -> Result<()>;
       async fn retrieve_key(&self, id: &str) -> Result<Vec<u8>>;
       async fn delete_key(&self, id: &str) -> Result<()>;
       async fn list_keys(&self) -> Result<Vec<String>>;
   }
   ```

3. Implement browser backend:
   ```rust
   #[cfg(target_arch = "wasm32")]
   pub struct IndexedDBBackend {
       db_name: String,
   }
   
   #[cfg(target_arch = "wasm32")]
   impl SecureStorageBackend for IndexedDBBackend {
       async fn store_key(&self, id: &str, data: &[u8]) -> Result<()> {
           // Use web-sys to access IndexedDB
           let window = web_sys::window().unwrap();
           let idb = window.indexed_db()?.unwrap();
           // ... IndexedDB operations
       }
   }
   ```

4. Gate device attestation:
   - Hardware UUID reading is platform-specific
   - Browser equivalent: crypto.subtle or WebAuthn
   - Will require different attestation model for web

**Action Required:**
1. Refactor secure storage into trait-based architecture
2. Extract core identity logic
3. Implement IndexedDB backend (1-2 weeks)
4. Implement localStorage fallback for simple cases
5. Redesign device attestation for browser context

**Estimated Effort:** 3-4 weeks

#### `aura-transport`
**Status:** ❌ Requires transport abstraction redesign

**Blockers:**

1. **Socket-Based Architecture**:
   ```rust
   // crates/transport/src/transport.rs
   pub trait Transport {
       async fn send(&self, addr: SocketAddr, data: &[u8]) -> Result<()>;
       async fn receive(&self) -> Result<(SocketAddr, Vec<u8>)>;
   }
   ```
   - `std::net::SocketAddr` doesn't exist in browser
   - Direct socket access not available in WASM

2. **HTTP Client Uses reqwest**:
   ```rust
   // crates/transport/src/https_relay.rs
   use reqwest::Client; // Not WASM-compatible
   ```
   - `reqwest` is OS-based HTTP client
   - Browser needs Fetch API instead

3. **Tokio Network Operations**:
   - Uses `tokio::net` for async sockets
   - Not available in WASM runtime

**Required Architectural Changes:**

1. Abstract address concept:
   ```rust
   pub enum Address {
       Socket(SocketAddr),
       Url(String),
       PeerId(String),
       Custom(Box<dyn Any + Send>),
   }
   ```

2. Redesign transport trait:
   ```rust
   pub trait Transport {
       type Address;
       async fn send(&self, addr: Self::Address, data: &[u8]) -> Result<()>;
       async fn receive(&self) -> Result<(Self::Address, Vec<u8>)>;
   }
   ```

3. Implement browser transports:
   ```rust
   #[cfg(target_arch = "wasm32")]
   pub struct WebSocketTransport {
       url: String,
   }
   
   #[cfg(target_arch = "wasm32")]
   impl Transport for WebSocketTransport {
       type Address = String;
       
       async fn send(&self, addr: String, data: &[u8]) -> Result<()> {
           // Use web-sys::WebSocket
           let ws = web_sys::WebSocket::new(&addr)?;
           ws.send_with_u8_array(data)?;
           Ok(())
       }
   }
   ```

4. Implement Fetch-based HTTP transport:
   ```rust
   #[cfg(target_arch = "wasm32")]
   pub struct FetchTransport;
   
   #[cfg(target_arch = "wasm32")]
   impl Transport for FetchTransport {
       async fn send(&self, url: String, data: &[u8]) -> Result<()> {
           let window = web_sys::window().unwrap();
           let request = web_sys::Request::new_with_str_and_init(
               &url,
               web_sys::RequestInit::new().method("POST").body(...),
           )?;
           let response = JsFuture::from(window.fetch_with_request(&request)).await?;
           // ...
       }
   }
   ```

**Action Required:**
1. Redesign transport abstraction to be address-agnostic
2. Remove `SocketAddr` dependencies
3. Implement WebSocket backend for browser (1 week)
4. Implement Fetch API backend for HTTP relay (1 week)
5. Update all transport consumers

**Estimated Effort:** 3-4 weeks

#### `aura-store`
**Status:** ❌ Requires storage backend abstraction

**Blockers:**

1. **Redb Database Dependency**:
   ```rust
   // crates/store/src/indexer.rs
   use redb::{Database, ReadableTable, TableDefinition};
   ```
   - `redb` 2.0 is file-based embedded database
   - **Does not support WASM** - requires filesystem
   - No WASM compilation target available

2. **File System Operations**:
   - Database file creation and management
   - Index persistence to disk
   - Quota tracking files

**Required Architectural Changes:**

1. Abstract storage backend:
   ```rust
   pub trait StorageBackend {
       type Transaction;
       
       async fn begin_transaction(&self) -> Result<Self::Transaction>;
       async fn commit(&self, txn: Self::Transaction) -> Result<()>;
       async fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>>;
       async fn put(&self, key: &[u8], value: &[u8]) -> Result<()>;
       async fn delete(&self, key: &[u8]) -> Result<()>;
       async fn scan(&self, prefix: &[u8]) -> Result<Vec<(Vec<u8>, Vec<u8>)>>;
   }
   ```

2. Native implementation with redb:
   ```rust
   #[cfg(not(target_arch = "wasm32"))]
   pub struct RedbBackend {
       db: Database,
   }
   ```

3. Browser implementation with IndexedDB:
   ```rust
   #[cfg(target_arch = "wasm32")]
   pub struct IndexedDBStorageBackend {
       db_name: String,
   }
   
   #[cfg(target_arch = "wasm32")]
   impl StorageBackend for IndexedDBStorageBackend {
       async fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
           // Use rexie crate or direct web-sys bindings
           let window = web_sys::window().unwrap();
           let idb_factory = window.indexed_db()?.unwrap();
           // ... IndexedDB get operation
       }
   }
   ```

4. Consider using existing WASM storage crates:
   - `rexie` - Rust IndexedDB wrapper
   - `indexed_db_futures` - Async IndexedDB bindings
   - `gloo-storage` - Simple localStorage wrapper

**Action Required:**
1. Create `StorageBackend` trait abstraction
2. Refactor existing code to use trait
3. Feature-gate redb for native builds only
4. Implement IndexedDB backend (2 weeks)
5. Add in-memory backend for testing
6. Update chunk storage and indexer to use abstraction

**Estimated Effort:** 3-4 weeks

#### `aura-cli`
**Status:** ❌ Desktop-only, should not be compiled for WASM

**Rationale:**
- Command-line interface is inherently desktop-specific
- Uses `clap` for CLI parsing (not relevant in browser)
- File I/O throughout for configuration management
- Should remain native-only

**Alternative:** Create `aura-web-ui` crate for browser interface

**Action Required:**
1. Add `#[cfg(not(target_arch = "wasm32"))]` to exclude from WASM builds
2. Create separate web UI crate if browser interface needed

**Estimated Effort:** N/A (exclude from WASM)

#### `aura-simulator`
**Status:** ❌ Testing framework, developer-only

**Rationale:**
- Integration testing framework
- Not needed in production WASM builds
- Can remain native-only

**Action Required:**
1. Exclude from WASM builds with workspace configuration

**Estimated Effort:** N/A (exclude from WASM)

---

## 2. Dependency Analysis: WASM Compatibility Matrix

### Cryptography (✅ Fully WASM-Compatible)

| Crate | Version | WASM Status | Notes |
|-------|---------|-------------|-------|
| `frost-core` | 1.0 | ✅ Compatible | Pure Rust, no syscalls |
| `frost-ed25519` | 1.0 | ✅ Compatible | Depends on frost-core |
| `ed25519-dalek` | 2.1 | ✅ Compatible | WASM support confirmed |
| `curve25519-dalek` | 4.1 | ✅ Compatible | Elliptic curve math |
| `blake3` | 1.5 | ✅ Compatible | Optional SIMD (disabled for WASM) |
| `aes-gcm` | 0.10 | ✅ Compatible | Pure crypto |
| `hpke` | 0.12 | ✅ Compatible | Depends on compatible crates |
| `chacha20poly1305` | 0.10 | ✅ Compatible | Pure crypto |
| `sha2` | 0.10 | ✅ Compatible | Pure hash functions |
| `hkdf` | 0.12 | ✅ Compatible | Pure KDF |
| `zeroize` | 1.7 | ✅ Compatible | Memory zeroing |

### Serialization (✅ Fully WASM-Compatible)

| Crate | Version | WASM Status | Notes |
|-------|---------|-------------|-------|
| `serde` | 1.0 | ✅ Compatible | Full WASM support |
| `serde_json` | 1.0 | ✅ Compatible | JSON (de)serialization |
| `serde_cbor` | 0.11 | ✅ Compatible | CBOR format |
| `bincode` | 1.3 | ✅ Compatible | Binary serialization |
| `toml` | 0.8 | ✅ Compatible | Pure Rust parser |

### Data Structures (⚠️ Mostly Compatible)

| Crate | Version | WASM Status | Notes |
|-------|---------|-------------|-------|
| `automerge` | 0.5 | ⚠️ Partial | Has WASM builds, needs testing |
| `uuid` | 1.6 | ⚠️ Conditional | Needs `getrandom` with "js" feature |
| `time` | 0.3 | ⚠️ Conditional | Needs WASM feature flags |
| `indexmap` | 2.0 | ✅ Compatible | Pure Rust hashmap |

### Async Runtime (❌ Major Issues)

| Crate | Version | WASM Status | Notes |
|-------|---------|-------------|-------|
| `tokio` | 1.35 | ❌ Not Compatible | OS-based async runtime |
| `futures` | 0.3 | ✅ Compatible | Async traits work |
| `async-trait` | 0.1 | ✅ Compatible | Proc macros work |
| `wasm-bindgen-futures` | 0.4 | ✅ WASM-specific | Required for browser async |

**Tokio Issues:**
- `tokio::spawn` requires OS thread pool (not available in browser)
- `tokio::fs` requires filesystem (not available in browser)
- `tokio::net` requires sockets (not available in browser)
- `tokio::time` requires OS timers (use `gloo-timers` instead)

**Solution:** Feature-gate Tokio, use `wasm-bindgen-futures` for WASM

### Networking (❌ Not WASM-Compatible)

| Crate | Version | WASM Status | Notes |
|-------|---------|-------------|-------|
| `reqwest` | 0.11 | ❌ Not Compatible | System HTTP client |
| `axum` | 0.7 | ❌ Not Compatible | Web framework for Tokio |
| `snow` | 0.9 | ⚠️ Partial | Noise protocol, needs WASM RNG |

**Alternatives for WASM:**
- `web-sys::fetch` for HTTP requests
- `web-sys::WebSocket` for WebSocket connections
- `gloo-net` for high-level networking abstractions

### Platform-Specific (❌ Complete Blockers)

| Crate | Version | WASM Status | Notes |
|-------|---------|-------------|-------|
| `security-framework` | 2.9 | ❌ macOS/iOS only | Apple keychain APIs |
| `core-foundation` | 0.9 | ❌ macOS/iOS only | Apple system frameworks |
| `windows` | 0.52 | ❌ Windows only | Win32 APIs |
| `keyutils` | 0.1 | ❌ Linux only | Linux kernel keyring |
| `redb` | 2.0 | ❌ Not Compatible | File-based database |
| `dirs` | 5.0 | ❌ Not Compatible | Platform paths |

**Alternatives for WASM:**
- IndexedDB for persistent storage (via `rexie` or `web-sys`)
- localStorage for simple key-value storage (via `gloo-storage`)
- WebCrypto API for key management
- No direct keychain equivalent (use encrypted IndexedDB)

### Utilities (⚠️ Mixed Compatibility)

| Crate | Version | WASM Status | Notes |
|-------|---------|-------------|-------|
| `rand` | 0.8 | ⚠️ Conditional | Needs `getrandom` with "js" |
| `getrandom` | 0.2 | ⚠️ Conditional | **Must** use `features = ["js"]` |
| `rand_chacha` | 0.3 | ✅ Compatible | Pure PRNG |
| `hex` | 0.4 | ✅ Compatible | String encoding |
| `tracing` | 0.1 | ✅ Compatible | Logging facade |
| `tracing-subscriber` | 0.3 | ❌ Not Compatible | Needs environment vars |
| `tracing-wasm` | 0.2 | ✅ WASM-specific | Browser console logging |

---

## 3. Code Pattern Analysis

### Pattern 1: File System Operations
**Locations:** 7 files (agent, store, cli)

**Example:**
```rust
// crates/agent/src/types.rs
use std::fs;
use std::path::Path;

pub fn load_config(path: &Path) -> Result<Config> {
    let contents = fs::read_to_string(path)?;
    toml::from_str(&contents)
}
```

**WASM Alternative:**
```rust
#[cfg(not(target_arch = "wasm32"))]
pub fn load_config(path: &Path) -> Result<Config> {
    let contents = std::fs::read_to_string(path)?;
    toml::from_str(&contents)
}

#[cfg(target_arch = "wasm32")]
pub async fn load_config(key: &str) -> Result<Config> {
    use gloo_storage::{LocalStorage, Storage};
    let contents: String = LocalStorage::get(key)?;
    toml::from_str(&contents)
}
```

### Pattern 2: Network Socket Operations
**Locations:** 3 files (transport crate)

**Example:**
```rust
// crates/transport/src/unified_transport.rs
use std::net::SocketAddr;

pub trait Transport {
    async fn send(&self, addr: SocketAddr, data: &[u8]) -> Result<()>;
}
```

**WASM Alternative:**
```rust
pub enum NetworkAddress {
    #[cfg(not(target_arch = "wasm32"))]
    Socket(std::net::SocketAddr),
    
    #[cfg(target_arch = "wasm32")]
    Url(String),
    
    PeerId(String),
}

pub trait Transport {
    async fn send(&self, addr: NetworkAddress, data: &[u8]) -> Result<()>;
}
```

### Pattern 3: Platform-Specific Secure Storage
**Locations:** 1 file (extensive use)

**Example:**
```rust
// crates/agent/src/secure_storage.rs
#[cfg(target_os = "macos")]
impl SecureStorageBackend for KeychainBackend {
    fn store(&self, key: &str, value: &[u8]) -> Result<()> {
        use security_framework::keychain::SecKeychain;
        // ... Apple APIs
    }
}
```

**WASM Alternative:**
```rust
#[cfg(target_arch = "wasm32")]
impl SecureStorageBackend for IndexedDBBackend {
    async fn store(&self, key: &str, value: &[u8]) -> Result<()> {
        use rexie::{Rexie, TransactionMode};
        
        let db = Rexie::builder("aura_secure_storage")
            .add_object_store("keys")
            .build()
            .await?;
            
        let transaction = db.transaction(&["keys"], TransactionMode::ReadWrite)?;
        let store = transaction.store("keys")?;
        
        // Encrypt value before storing
        let encrypted = self.encrypt(value)?;
        store.put(&key, &encrypted).await?;
        
        transaction.commit().await?;
        Ok(())
    }
}
```

### Pattern 4: Tokio Task Spawning
**Locations:** Throughout coordination crate

**Example:**
```rust
// crates/coordination/src/choreography/counter.rs
let handle = tokio::spawn(async move {
    poll_messages(receiver).await;
});
```

**WASM Alternative:**
```rust
#[cfg(not(target_arch = "wasm32"))]
fn spawn_task<F>(future: F) -> JoinHandle<()>
where
    F: Future<Output = ()> + Send + 'static,
{
    tokio::spawn(future)
}

#[cfg(target_arch = "wasm32")]
fn spawn_task<F>(future: F)
where
    F: Future<Output = ()> + 'static,
{
    wasm_bindgen_futures::spawn_local(future);
}
```

### Pattern 5: System Time Access
**Locations:** 5+ files

**Example:**
```rust
// crates/transport/src/https_relay.rs
let timestamp = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .unwrap()
    .as_secs();
```

**WASM Alternative:**
```rust
#[cfg(not(target_arch = "wasm32"))]
fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[cfg(target_arch = "wasm32")]
fn current_timestamp() -> u64 {
    (js_sys::Date::now() / 1000.0) as u64
}
```

---

## 4. Phased Implementation Roadmap

### Phase 1: Core Logic Enablement (2-3 weeks)
**Goal:** Enable core cryptographic and protocol logic for WASM compilation

**Tasks:**
1. Add WASM target dependencies to workspace:
   ```toml
   [target.'cfg(target_arch = "wasm32")'.dependencies]
   getrandom = { version = "0.2", features = ["js"] }
   wasm-bindgen = "0.2"
   wasm-bindgen-futures = "0.4"
   js-sys = "0.3"
   web-sys = { version = "0.3", features = ["console", "Window", "crypto"] }
   ```

2. Update `aura-crypto`:
   - Implement `WasmRandomSource` using `getrandom`
   - Implement `WasmTimeSource` using `js_sys::Date`
   - Feature-gate implementations with `#[cfg(target_arch = "wasm32")]`

3. Update `aura-coordination`:
   - Create task spawning abstraction
   - Replace `tokio::spawn` with platform-agnostic function
   - Feature-gate Tokio dependencies

4. Add workspace feature flags:
   ```toml
   [features]
   default = ["native"]
   native = ["tokio/full", "redb"]
   wasm = ["wasm-bindgen", "wasm-bindgen-futures"]
   ```

**Deliverables:**
- `aura-crypto` compiles to WASM
- `aura-session-types` compiles to WASM
- `aura-groups` compiles to WASM
- `aura-journal` compiles to WASM
- `aura-coordination` compiles to WASM with feature flags

### Phase 2: Storage Abstraction (3-4 weeks)
**Goal:** Abstract storage layer to support both native and WASM backends

**Tasks:**
1. Create `aura-storage-traits` crate:
   ```rust
   pub trait StorageBackend {
       async fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>>;
       async fn put(&self, key: &[u8], value: &[u8]) -> Result<()>;
       async fn delete(&self, key: &[u8]) -> Result<()>;
       async fn scan(&self, prefix: &[u8]) -> Result<Vec<(Vec<u8>, Vec<u8>)>>;
   }
   
   pub trait SecureStorageBackend {
       async fn store_key(&self, id: &str, data: &[u8]) -> Result<()>;
       async fn retrieve_key(&self, id: &str) -> Result<Vec<u8>>;
       async fn delete_key(&self, id: &str) -> Result<()>;
   }
   ```

2. Refactor `aura-store`:
   - Extract redb usage into `RedbBackend` implementation
   - Feature-gate redb for `#[cfg(not(target_arch = "wasm32"))]`
   - Update chunk storage to use trait

3. Create `aura-storage-indexeddb` crate:
   - Implement `IndexedDBStorageBackend`
   - Implement `IndexedDBSecureBackend` with encryption
   - Use `rexie` or direct `web-sys` bindings

4. Refactor `aura-agent` secure storage:
   - Extract to use `SecureStorageBackend` trait
   - Feature-gate platform-specific backends
   - Add IndexedDB backend for WASM

**Deliverables:**
- Storage backend abstraction defined
- Native redb backend implementation
- WASM IndexedDB backend implementation
- Secure storage abstraction for agent
- All storage consumers updated to use traits

### Phase 3: Network Abstraction (3-4 weeks)
**Goal:** Abstract transport layer to support both native and browser networking

**Tasks:**
1. Create `aura-transport-traits` crate:
   ```rust
   pub enum NetworkAddress {
       #[cfg(not(target_arch = "wasm32"))]
       Socket(std::net::SocketAddr),
       Url(String),
       PeerId(String),
   }
   
   pub trait Transport {
       async fn send(&self, addr: NetworkAddress, data: &[u8]) -> Result<()>;
       async fn receive(&self) -> Result<(NetworkAddress, Vec<u8>)>;
       async fn connect(&self, addr: NetworkAddress) -> Result<()>;
       async fn disconnect(&self) -> Result<()>;
   }
   ```

2. Refactor existing transports:
   - Update `HttpsRelayTransport` to use `NetworkAddress`
   - Feature-gate reqwest for native builds
   - Remove direct `SocketAddr` usage

3. Create browser transports:
   - `WebSocketTransport` using `web-sys::WebSocket`
   - `FetchTransport` using `web-sys::fetch`
   - Implement error handling and reconnection logic

4. Update transport consumers:
   - Update all code using transport to handle `NetworkAddress`
   - Add address resolution layer
   - Test both native and WASM transports

**Deliverables:**
- Transport abstraction redesigned
- Native HTTP/socket transports refactored
- WASM WebSocket transport implementation
- WASM Fetch API transport implementation
- Integration tests for both platforms

### Phase 4: Browser Integration (2-3 weeks)
**Goal:** Create browser-specific glue code and test WASM builds

**Tasks:**
1. Create `aura-wasm` crate:
   - Export WASM-compatible API using `#[wasm_bindgen]`
   - Implement browser-specific initialization
   - Create JavaScript interop layer

2. Example exports:
   ```rust
   #[wasm_bindgen]
   pub struct AuraClient {
       agent: Agent,
       storage: IndexedDBBackend,
       transport: WebSocketTransport,
   }
   
   #[wasm_bindgen]
   impl AuraClient {
       #[wasm_bindgen(constructor)]
       pub async fn new() -> Result<AuraClient, JsValue> {
           // Initialize with browser backends
       }
       
       pub async fn bootstrap_account(&mut self, threshold: u16, participants: u16) -> Result<(), JsValue> {
           // Wrap core logic
       }
       
       pub async fn derive_key(&self, app_id: String, context: String) -> Result<Vec<u8>, JsValue> {
           // Wrap DKD protocol
       }
   }
   ```

3. Build infrastructure:
   - Add `wasm-pack` build scripts to justfile
   - Configure webpack or rollup for JavaScript packaging
   - Set up TypeScript definitions generation

4. Testing:
   - `wasm-bindgen-test` for unit tests
   - Browser-based integration tests
   - Headless browser testing (Puppeteer/Playwright)

**Deliverables:**
- `aura-wasm` crate with JavaScript API
- Build scripts for WASM packaging
- TypeScript type definitions
- Browser test suite
- Example web application

### Phase 5: Web UI Development (4-6 weeks)
**Goal:** Create production-ready web application using WASM

**Tasks:**
1. Create `web-ui` directory:
   ```
   web-ui/
   ├── src/
   │   ├── index.html
   │   ├── app.ts
   │   ├── components/
   │   └── styles/
   ├── package.json
   ├── tsconfig.json
   └── webpack.config.js
   ```

2. Implement web interface:
   - Account initialization UI
   - Key derivation interface
   - Device management
   - Session credentials display
   - Network status monitoring

3. Integration:
   - Load WASM module
   - Initialize with browser storage
   - Connect to relay servers
   - Handle async operations properly

4. Production optimization:
   - Code splitting
   - Lazy loading
   - Service worker for offline support
   - Performance profiling

**Deliverables:**
- Production web application
- User documentation
- Deployment guide
- Performance benchmarks

---

## 5. Testing Strategy

### Unit Tests
```rust
#[cfg(not(target_arch = "wasm32"))]
#[tokio::test]
async fn test_native() {
    // Native async tests
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test]
async fn test_wasm() {
    // WASM async tests
}
```

### Integration Tests
- Native: Use existing test infrastructure
- WASM: Use headless browser (Puppeteer)
- Cross-platform: Share test logic, different runners

### Performance Tests
- Measure WASM overhead vs native
- Profile memory usage in browser
- Test with large datasets
- Network latency simulation

---

## 6. Migration Checklist

### Workspace Configuration
- [ ] Add `wasm32-unknown-unknown` target to flake.nix
- [ ] Add WASM tools (wasm-pack, wasm-bindgen-cli)
- [ ] Configure workspace features for native/wasm
- [ ] Update CI to test WASM builds

### aura-crypto
- [ ] Implement `WasmRandomSource`
- [ ] Implement `WasmTimeSource`
- [ ] Feature-gate implementations
- [ ] Add WASM tests
- [ ] Verify all crypto primitives work in WASM

### aura-coordination
- [ ] Create task spawning abstraction
- [ ] Replace tokio::spawn usage
- [ ] Feature-gate Tokio dependencies
- [ ] Add wasm-bindgen-futures
- [ ] Test async workflows in browser

### aura-agent
- [ ] Extract core identity logic
- [ ] Create storage backend trait
- [ ] Implement IndexedDB backend
- [ ] Feature-gate platform-specific storage
- [ ] Test key storage in browser

### aura-transport
- [ ] Redesign address abstraction
- [ ] Remove SocketAddr dependencies
- [ ] Implement WebSocket transport
- [ ] Implement Fetch transport
- [ ] Test network operations in browser

### aura-store
- [ ] Create storage backend trait
- [ ] Feature-gate redb
- [ ] Implement IndexedDB backend
- [ ] Add in-memory backend for tests
- [ ] Test chunk storage in browser

### aura-wasm
- [ ] Create new crate
- [ ] Export JavaScript API
- [ ] Generate TypeScript definitions
- [ ] Write browser integration tests
- [ ] Document JavaScript API

---

## 7. Estimated Timeline

| Phase | Duration | Dependencies |
|-------|----------|--------------|
| Phase 1: Core Logic | 2-3 weeks | None |
| Phase 2: Storage Abstraction | 3-4 weeks | Phase 1 |
| Phase 3: Network Abstraction | 3-4 weeks | Phase 1 |
| Phase 4: Browser Integration | 2-3 weeks | Phases 1-3 |
| Phase 5: Web UI | 4-6 weeks | Phase 4 |

**Total: 14-20 weeks (3.5-5 months)**

With parallel development of Phases 2 and 3: **12-16 weeks (3-4 months)**

---

## 8. Risk Assessment

### High Risk
1. **Automerge WASM compatibility** - Version 0.5 may have issues
   - Mitigation: Test early, consider alternatives if needed

2. **Performance degradation** - WASM may be slower than native
   - Mitigation: Profile and optimize hot paths

3. **Browser storage limitations** - IndexedDB quotas
   - Mitigation: Implement quota management and pruning

### Medium Risk
1. **Breaking API changes** - WASM exports different from native
   - Mitigation: Maintain API compatibility layer

2. **Browser compatibility** - Older browsers may not support features
   - Mitigation: Target modern browsers, polyfills where possible

### Low Risk
1. **Build complexity** - Multiple build targets
   - Mitigation: Use good CI/CD infrastructure

2. **Testing complexity** - Different test runners for WASM
   - Mitigation: Share test logic, automate browser testing

---

## 9. Success Criteria

### Phase 1 Success
- ✅ Core cryptographic protocols compile to WASM
- ✅ Can run FROST threshold signatures in browser
- ✅ Can run DKD protocol in browser
- ✅ Effects system works with browser APIs

### Phase 2 Success
- ✅ Can store encrypted data in IndexedDB
- ✅ Can retrieve and decrypt stored data
- ✅ Storage quota management works
- ✅ Migration from native storage possible

### Phase 3 Success
- ✅ Can send/receive messages via WebSocket
- ✅ Can communicate with relay servers
- ✅ Network state properly managed
- ✅ Reconnection logic works

### Phase 4 Success
- ✅ JavaScript API exported and functional
- ✅ TypeScript definitions generated
- ✅ Example application works in browser
- ✅ Integration tests pass in headless browser

### Phase 5 Success
- ✅ Production web UI deployed
- ✅ Can initialize accounts from browser
- ✅ Can perform all core operations
- ✅ Performance acceptable for users
- ✅ Documentation complete

---

## 10. Resources and References

### WASM Resources
- [Rust and WebAssembly Book](https://rustwasm.github.io/book/)
- [wasm-bindgen Guide](https://rustwasm.github.io/wasm-bindgen/)
- [web-sys Documentation](https://rustwasm.github.io/wasm-bindgen/api/web_sys/)

### Browser Storage APIs
- [IndexedDB API](https://developer.mozilla.org/en-US/docs/Web/API/IndexedDB_API)
- [Web Crypto API](https://developer.mozilla.org/en-US/docs/Web/API/Web_Crypto_API)
- [rexie crate](https://docs.rs/rexie/) - IndexedDB wrapper

### Browser Networking
- [Fetch API](https://developer.mozilla.org/en-US/docs/Web/API/Fetch_API)
- [WebSocket API](https://developer.mozilla.org/en-US/docs/Web/API/WebSocket)
- [gloo-net crate](https://docs.rs/gloo-net/) - Browser networking

### Testing
- [wasm-bindgen-test](https://rustwasm.github.io/wasm-bindgen/wasm-bindgen-test/)
- [Puppeteer](https://pptr.dev/) - Headless browser automation
- [Playwright](https://playwright.dev/) - Cross-browser testing

---

## 11. Conclusion

The Aura project can be compiled to WASM, but it requires a phased architectural refactoring approach:

1. **30-40% of the codebase** (core logic) is already WASM-compatible or nearly so
2. **60-70% requires architectural changes** to abstract platform-specific dependencies
3. **Estimated timeline: 3-5 months** of focused development
4. **Main challenges:** Storage abstraction, network abstraction, secure storage in browser

The good news is that the core cryptographic and protocol logic is well-structured and can be made WASM-compatible with minimal changes. The effects system in `aura-crypto` provides an excellent foundation for platform abstraction.

The recommended approach is to tackle this incrementally, starting with core logic (Phase 1) which provides immediate value by enabling protocol testing in browsers, then progressively adding storage and networking abstractions to reach full WASM support.
