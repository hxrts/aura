# Deployment Guide

Aura applications require careful deployment planning to ensure security, reliability, and performance across distributed environments. This guide covers production deployment patterns, security best practices, monitoring approaches, and cross-platform considerations.

The deployment process involves effect system configuration, infrastructure provisioning, security hardening, and operational monitoring. You will learn to deploy applications that maintain Aura's security and consistency guarantees.

See [Getting Started Guide](800_getting_started_guide.md) for development basics. See [Effect System Guide](801_effect_system_guide.md) for production handler configuration.

---

## Production Deployment Patterns

**Effect System Configuration** adapts applications to production infrastructure using environment-specific handlers. Production configurations replace development mocks with real infrastructure integration.

```rust
use aura_protocol::effects::AuraEffectSystem;
use aura_protocol::handlers::{
    FileSystemStorageHandler,
    PostgreSQLJournalHandler, 
    TlsNetworkHandler,
    SystemTimeHandler,
};

pub fn create_production_effects(config: &DeploymentConfig) -> AuraEffectSystem {
    let storage = FileSystemStorageHandler::encrypted(
        &config.storage_path,
        &config.encryption_key,
    );
    
    let journal = PostgreSQLJournalHandler::new(
        &config.database_url,
        config.connection_pool_size,
    );
    
    let network = TlsNetworkHandler::new(
        config.listen_port,
        &config.tls_certificate,
        &config.tls_private_key,
    );
    
    let time = SystemTimeHandler::with_ntp_sync(&config.ntp_servers);
    
    AuraEffectSystem::new(storage, journal, network, time)
}
```

Production effect systems use real infrastructure handlers with proper security and reliability features. Configuration drives handler selection without application code changes.

**Container Orchestration** enables scalable deployment using Kubernetes or Docker Swarm. Container orchestration provides service discovery, load balancing, and failure recovery.

```yaml
# aura-app-deployment.yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: aura-application
spec:
  replicas: 3
  selector:
    matchLabels:
      app: aura-application
  template:
    metadata:
      labels:
        app: aura-application
    spec:
      containers:
      - name: aura-app
        image: aura-application:latest
        ports:
        - containerPort: 8080
        env:
        - name: DATABASE_URL
          valueFrom:
            secretKeyRef:
              name: aura-secrets
              key: database-url
        - name: STORAGE_PATH
          value: "/data/storage"
        volumeMounts:
        - name: storage-volume
          mountPath: /data
        - name: tls-certs
          mountPath: /certs
      volumes:
      - name: storage-volume
        persistentVolumeClaim:
          claimName: aura-storage-pvc
      - name: tls-certs
        secret:
          secretName: aura-tls-certs
```

Container deployment provides isolation, reproducibility, and scalability. Persistent volumes ensure data survives container restarts while secrets management protects sensitive configuration.

**Service Mesh Integration** enables secure communication between Aura applications and external services. Service mesh provides encryption, authentication, and traffic management.

```yaml
# aura-service-mesh.yaml
apiVersion: networking.istio.io/v1alpha3
kind: VirtualService
metadata:
  name: aura-application
spec:
  http:
  - match:
    - uri:
        prefix: /api/
    route:
    - destination:
        host: aura-application
        port:
          number: 8080
    timeout: 30s
    retries:
      attempts: 3
      perTryTimeout: 10s
---
apiVersion: security.istio.io/v1beta1
kind: PeerAuthentication
metadata:
  name: aura-application
spec:
  selector:
    matchLabels:
      app: aura-application
  mtls:
    mode: STRICT
```

Service mesh configuration enables mutual TLS authentication and automatic retry handling. This provides defense-in-depth security for distributed Aura applications.

## Security Best Practices

**Key Management** protects cryptographic material using hardware security modules and key derivation functions. Proper key management prevents unauthorized access to Aura accounts.

```rust
use aura_crypto::{KeyDerivationParams, SecureRandom};
use std::path::Path;

pub struct ProductionKeyManager {
    hsm_client: HsmClient,
    key_derivation: KeyDerivationParams,
}

impl ProductionKeyManager {
    pub fn new(hsm_config: HsmConfig) -> Result<Self, KeyManagerError> {
        let hsm_client = HsmClient::connect(hsm_config)?;
        
        let key_derivation = KeyDerivationParams {
            iterations: 100_000,
            memory_cost: 64 * 1024, // 64 MB
            parallelism: 4,
            salt_length: 32,
        };
        
        Ok(Self { hsm_client, key_derivation })
    }

    pub async fn derive_device_key(
        &self,
        device_id: DeviceId,
        master_key_id: &str,
    ) -> Result<DeviceKey, KeyManagerError> {
        let salt = SecureRandom::generate_bytes(32);
        
        let master_key = self.hsm_client
            .get_key(master_key_id)
            .await?;
            
        let derived_key = self.key_derivation
            .derive_key(&master_key, &device_id.as_bytes(), &salt)?;
            
        Ok(DeviceKey::new(derived_key, salt))
    }
}
```

Production key management uses hardware security modules for key storage and cryptographically secure key derivation. This prevents key compromise even with system access.

**Network Security** protects communication using Transport Layer Security and certificate pinning. Network security prevents eavesdropping and man-in-the-middle attacks.

```rust
use rustls::{Certificate, ClientConfig, ServerConfig, PrivateKey};
use webpki_roots;

pub fn create_tls_client_config(
    ca_certificates: &[Certificate],
    client_cert: Certificate,
    client_key: PrivateKey,
) -> Result<ClientConfig, TlsError> {
    let mut config = ClientConfig::builder()
        .with_safe_defaults()
        .with_root_certificates(RootCertStore::empty())
        .with_single_cert(vec![client_cert], client_key)?;
    
    // Add custom CA certificates for Aura network
    for cert in ca_certificates {
        config.root_store.add(cert)?;
    }
    
    // Enable certificate pinning
    config.dangerous().set_certificate_verifier(
        Arc::new(AuraCertificateVerifier::new(ca_certificates))
    );
    
    Ok(config)
}
```

TLS configuration uses strong cipher suites and certificate validation. Certificate pinning prevents attacks using compromised certificate authorities.

**Access Control** implements capability-based authorization using Aura's Web of Trust system. Access control ensures only authorized devices can perform operations.

```rust
use aura_wot::{CapabilitySet, TrustPolicy, WotEffects};

pub struct ProductionAccessControl {
    wot_effects: Arc<dyn WotEffects>,
    policies: HashMap<OperationType, TrustPolicy>,
}

impl ProductionAccessControl {
    pub async fn authorize_operation(
        &self,
        device_id: DeviceId,
        operation: &Operation,
    ) -> Result<bool, AuthorizationError> {
        let required_policy = self.policies
            .get(&operation.operation_type())
            .ok_or(AuthorizationError::UnknownOperation)?;
        
        let device_capabilities = self.wot_effects
            .get_device_capabilities(device_id)
            .await?;
        
        let trust_level = self.wot_effects
            .evaluate_trust_level(device_id)
            .await?;
        
        Ok(required_policy.evaluate(&device_capabilities, trust_level))
    }
}
```

Access control integrates with Web of Trust to enforce capability-based authorization. This ensures operations execute only with appropriate trust levels and capabilities.

## Monitoring and Observability

**Metrics Collection** tracks application performance and resource usage using Prometheus and OpenTelemetry. Metrics enable proactive monitoring and capacity planning.

```rust
use opentelemetry::{global, metrics::{Counter, Histogram, Meter}};
use prometheus::{Counter as PrometheusCounter, Histogram as PrometheusHistogram};

pub struct AuraMetrics {
    operation_counter: Counter<u64>,
    operation_duration: Histogram<f64>,
    journal_size: PrometheusCounter,
    network_latency: PrometheusHistogram,
}

impl AuraMetrics {
    pub fn new() -> Self {
        let meter = global::meter("aura_application");
        
        Self {
            operation_counter: meter
                .u64_counter("aura_operations_total")
                .with_description("Total number of operations")
                .init(),
            
            operation_duration: meter
                .f64_histogram("aura_operation_duration_seconds")
                .with_description("Operation execution duration")
                .init(),
            
            journal_size: prometheus::register_counter!(
                "aura_journal_entries_total",
                "Total number of journal entries"
            ).unwrap(),
            
            network_latency: prometheus::register_histogram!(
                "aura_network_latency_seconds",
                "Network operation latency"
            ).unwrap(),
        }
    }

    pub fn record_operation(&self, operation_type: &str, duration: f64) {
        self.operation_counter
            .add(1, &[KeyValue::new("type", operation_type.to_string())]);
        
        self.operation_duration
            .record(duration, &[KeyValue::new("type", operation_type.to_string())]);
    }
}
```

Metrics collection provides visibility into application behavior and performance characteristics. Standardized metrics enable integration with monitoring infrastructure.

**Distributed Tracing** tracks requests across multiple services and devices. Distributed tracing helps debug performance issues and understand system behavior.

```rust
use opentelemetry::{global, trace::{Span, Tracer}};
use tracing::{info_span, Instrument};

pub async fn execute_distributed_operation(
    operation: Operation,
    devices: Vec<DeviceId>,
    effects: &AuraEffectSystem,
) -> Result<OperationResult, OperationError> {
    let tracer = global::tracer("aura_application");
    let span = tracer
        .span_builder("distributed_operation")
        .with_attributes(vec![
            KeyValue::new("operation.type", operation.operation_type().to_string()),
            KeyValue::new("devices.count", devices.len() as i64),
        ])
        .start(&tracer);
    
    async move {
        let mut results = Vec::new();
        
        for device_id in devices {
            let device_span = tracer
                .span_builder("device_operation")
                .with_attributes(vec![
                    KeyValue::new("device.id", device_id.to_string()),
                ])
                .start(&tracer);
            
            let result = async move {
                effects.execute_on_device(device_id, &operation).await
            }
            .instrument(info_span!("device_execution"))
            .await?;
            
            results.push(result);
            device_span.end();
        }
        
        Ok(OperationResult::merge(results))
    }
    .instrument(info_span!("distributed_operation"))
    .await
}
```

Distributed tracing correlates operations across multiple devices and services. Trace correlation enables understanding complex distributed workflows.

**Log Aggregation** centralizes log collection for analysis and alerting. Structured logging enables efficient searching and automated analysis.

```rust
use serde_json::json;
use tracing::{error, info, warn};

pub struct StructuredLogger {
    device_id: DeviceId,
    service_name: String,
}

impl StructuredLogger {
    pub fn log_operation_start(&self, operation_id: &str, operation_type: &str) {
        info!(
            operation_id = operation_id,
            operation_type = operation_type,
            device_id = %self.device_id,
            service = %self.service_name,
            "Operation started"
        );
    }

    pub fn log_operation_error(&self, operation_id: &str, error: &dyn std::error::Error) {
        error!(
            operation_id = operation_id,
            error = %error,
            device_id = %self.device_id,
            service = %self.service_name,
            "Operation failed"
        );
    }

    pub fn log_security_event(&self, event_type: &str, details: serde_json::Value) {
        warn!(
            event_type = event_type,
            details = %details,
            device_id = %self.device_id,
            service = %self.service_name,
            "Security event detected"
        );
    }
}
```

Structured logging includes contextual information for correlation and analysis. Security events receive special attention for threat detection and response.

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
pub fn create_mobile_effects(config: &MobileConfig) -> AuraEffectSystem {
    let storage = KeychainStorageHandler::new(&config.app_identifier);
    
    let journal = SqliteJournalHandler::new(&config.database_path)
        .with_wal_mode()
        .with_vacuum_on_close();
    
    let network = NsurlSessionHandler::new()
        .with_cellular_access(config.allow_cellular)
        .with_background_tasks(config.enable_background_sync);
    
    let time = CoreLocationTimeHandler::new();
    
    AuraEffectSystem::new(storage, journal, network, time)
}

#[cfg(target_os = "android")]
pub fn create_mobile_effects(config: &MobileConfig) -> AuraEffectSystem {
    let storage = AndroidKeystoreHandler::new(&config.keystore_alias);
    
    let journal = SqliteJournalHandler::new(&config.database_path)
        .with_wal_mode()
        .with_room_integration();
    
    let network = OkHttpHandler::new()
        .with_network_security_policy()
        .with_certificate_pinning(&config.certificate_pins);
    
    let time = SystemTimeHandler::with_ntp_sync(&config.ntp_servers);
    
    AuraEffectSystem::new(storage, journal, network, time)
}
```

Mobile effect systems integrate with platform-specific security and storage mechanisms. This ensures optimal performance and security on mobile devices.

**WebAssembly Deployment** enables browser-based Aura applications with appropriate security and performance adaptations. WebAssembly deployment handles browser limitations and capabilities.

```rust
#[cfg(target_arch = "wasm32")]
pub fn create_wasm_effects() -> AuraEffectSystem {
    use wasm_bindgen_futures::spawn_local;
    use web_sys::{window, IndexedDb, WebSocket};
    
    let storage = IndexedDbStorageHandler::new("aura_storage")
        .with_encryption(WebCryptoHandler::new());
    
    let journal = IndexedDbJournalHandler::new("aura_journal")
        .with_persistence_guarantee(false); // Best effort in browser
    
    let network = WebSocketNetworkHandler::new()
        .with_auto_reconnect(true)
        .with_heartbeat(Duration::from_secs(30));
    
    let time = PerformanceTimeHandler::new(); // performance.now() based
    
    AuraEffectSystem::new(storage, journal, network, time)
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