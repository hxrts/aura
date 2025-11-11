# Deployment Guide

Aura applications require careful deployment planning to ensure security, reliability, and performance across distributed environments. This guide covers production deployment patterns, security best practices, monitoring approaches, and cross-platform considerations.

The deployment process involves effect system configuration, infrastructure provisioning, security hardening, and operational monitoring. You will learn to deploy applications that maintain Aura's security and consistency guarantees.

See [Getting Started Guide](800_getting_started_guide.md) for development basics. See [Effect System Guide](801_effect_system_guide.md) for production handler configuration.

---

## Production Deployment Patterns

**Effect System Configuration** adapts applications to production infrastructure using environment-specific handlers. Production configurations replace development mocks with real infrastructure integration.

```rust
// Import runtime composition from aura-agent
use aura_agent::AuraAgent;

// Import handler implementations from aura-effects
use aura_effects::storage::FilesystemStorageHandler;
use aura_effects::network::TcpNetworkHandler;
use aura_effects::time::RealTimeHandler;
use aura_effects::journal::MemoryJournalHandler;

pub fn create_production_agent(
    device_id: DeviceId,
    config: &DeploymentConfig,
) -> Result<AuraAgent, DeploymentError> {
    // Configure handlers with production settings
    let storage = FilesystemStorageHandler::encrypted(
        &config.storage_path,
        &config.encryption_key,
    )?;

    let network = TcpNetworkHandler::with_tls(
        config.listen_port,
        &config.tls_certificate,
        &config.tls_private_key,
    )?;

    let time = RealTimeHandler::with_ntp_sync(&config.ntp_servers)?;

    // Use aura-agent to compose complete runtime
    let agent = AuraAgent::builder(device_id)
        .with_storage(Arc::new(storage))
        .with_network(Arc::new(network))
        .with_time(Arc::new(time))
        .build()?;

    Ok(agent)
}
```

Production effect systems use real infrastructure handlers with proper security and reliability features. Configuration drives handler selection without application code changes.

## Cross-Platform Considerations

**Platform Adaptation** handles differences in operating system capabilities and security features. Platform adaptation enables consistent behavior across deployment environments.

```rust
use std::path::Path;

#[cfg(target_os = "linux")]
pub fn create_secure_storage(path: &Path) -> Result<SecureStorage, StorageError> {
    use std::os::unix::fs::PermissionsExt;

    std::fs::create_dir_all(path)?;

    // Set restrictive permissions (owner only)
    let metadata = std::fs::metadata(path)?;
    let mut permissions = metadata.permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(path, permissions)?;

    Ok(SecureStorage::new(path, SecurityLevel::High))
}

#[cfg(target_os = "macos")]
pub fn create_secure_storage(path: &Path) -> Result<SecureStorage, StorageError> {
    std::fs::create_dir_all(path)?;

    // Use macOS keychain integration
    let keychain_service = KeychainService::new("aura_application")?;

    Ok(SecureStorage::with_keychain(path, keychain_service))
}

#[cfg(target_os = "windows")]
pub fn create_secure_storage(path: &Path) -> Result<SecureStorage, StorageError> {
    use winapi::um::fileapi::CreateDirectoryW;

    std::fs::create_dir_all(path)?;

    // Use Windows DPAPI for encryption
    let dpapi_provider = DpapiProvider::new()?;

    Ok(SecureStorage::with_dpapi(path, dpapi_provider))
}
```

Platform-specific implementations use operating system security features optimally. Conditional compilation enables platform optimization without runtime overhead.

**Mobile Deployment** adapts applications for iOS and Android platforms using appropriate security and performance optimizations. Mobile deployment handles platform constraints and capabilities.

```rust
#[cfg(target_os = "ios")]
pub fn create_mobile_agent(
    device_id: DeviceId,
    config: &MobileConfig,
) -> Result<AuraAgent, MobileError> {
    // iOS uses platform-specific handlers (these would be in aura-effects with platform features)
    let storage = KeychainStorageHandler::new(&config.app_identifier)?;

    let journal = SqliteJournalHandler::new(&config.database_path)
        .with_wal_mode()
        .with_vacuum_on_close()?;

    let network = NsurlSessionHandler::new()
        .with_cellular_access(config.allow_cellular)
        .with_background_tasks(config.enable_background_sync)?;

    let time = CoreLocationTimeHandler::new()?;

    // Use aura-agent to compose mobile runtime
    AuraAgent::builder(device_id)
        .with_storage(Arc::new(storage))
        .with_journal(Arc::new(journal))
        .with_network(Arc::new(network))
        .with_time(Arc::new(time))
        .build()
}

#[cfg(target_os = "android")]
pub fn create_mobile_agent(
    device_id: DeviceId,
    config: &MobileConfig,
) -> Result<AuraAgent, MobileError> {
    // Android uses platform-specific handlers (these would be in aura-effects with platform features)
    let storage = AndroidKeystoreHandler::new(&config.keystore_alias)?;

    let journal = SqliteJournalHandler::new(&config.database_path)
        .with_wal_mode()
        .with_room_integration()?;

    let network = OkHttpHandler::new()
        .with_network_security_policy()
        .with_certificate_pinning(&config.certificate_pins)?;

    let time = SystemTimeHandler::with_ntp_sync(&config.ntp_servers)?;

    // Use aura-agent to compose mobile runtime
    AuraAgent::builder(device_id)
        .with_storage(Arc::new(storage))
        .with_journal(Arc::new(journal))
        .with_network(Arc::new(network))
        .with_time(Arc::new(time))
        .build()
}
```

Mobile effect systems integrate with platform-specific security and storage mechanisms. This ensures optimal performance and security on mobile devices.

**WebAssembly Deployment** enables browser-based Aura applications with appropriate security and performance adaptations. WebAssembly deployment handles browser limitations and capabilities.

```rust
#[cfg(target_arch = "wasm32")]
pub fn create_wasm_agent(device_id: DeviceId) -> Result<AuraAgent, WasmError> {
    use wasm_bindgen_futures::spawn_local;
    use web_sys::{window, IndexedDb, WebSocket};

    // WASM uses browser-specific handlers (these would be in aura-effects with wasm feature)
    let storage = IndexedDbStorageHandler::new("aura_storage")
        .with_encryption(WebCryptoHandler::new())?;

    let journal = IndexedDbJournalHandler::new("aura_journal")
        .with_persistence_guarantee(false)?; // Best effort in browser

    let network = WebSocketNetworkHandler::new()
        .with_auto_reconnect(true)
        .with_heartbeat(Duration::from_secs(30))?;

    let time = PerformanceTimeHandler::new()?; // performance.now() based

    // Use aura-agent to compose WASM runtime
    AuraAgent::builder(device_id)
        .with_storage(Arc::new(storage))
        .with_journal(Arc::new(journal))
        .with_network(Arc::new(network))
        .with_time(Arc::new(time))
        .build()
}
```

WebAssembly effect systems use browser APIs while maintaining security boundaries. Browser limitations require adaptive implementation strategies.

**Resource Management** optimizes memory and CPU usage for different deployment environments. Resource management ensures applications perform well across various hardware configurations.

```rust
pub struct ResourceManager {
    memory_limit: usize,
    cpu_quota: f64,
    io_quota: u64,
}

impl ResourceManager {
    pub fn for_environment(env: DeploymentEnvironment) -> Self {
        match env {
            DeploymentEnvironment::Server => Self {
                memory_limit: 2 * 1024 * 1024 * 1024, // 2 GB
                cpu_quota: 1.0, // Full CPU
                io_quota: 1000 * 1024 * 1024, // 1 GB/s
            },
            DeploymentEnvironment::Mobile => Self {
                memory_limit: 256 * 1024 * 1024, // 256 MB
                cpu_quota: 0.3, // 30% CPU to preserve battery
                io_quota: 10 * 1024 * 1024, // 10 MB/s
            },
            DeploymentEnvironment::Embedded => Self {
                memory_limit: 64 * 1024 * 1024, // 64 MB
                cpu_quota: 0.1, // 10% CPU
                io_quota: 1024 * 1024, // 1 MB/s
            },
        }
    }

    pub fn configure_journal(&self, journal: &mut JournalHandler) {
        journal.set_cache_size(self.memory_limit / 10);
        journal.set_write_batch_size(self.io_quota / 100);
        journal.set_background_compaction(self.cpu_quota > 0.5);
    }
}
```

Resource management adapts application behavior to available resources. This ensures good performance across different deployment environments while respecting system constraints.
