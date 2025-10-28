//! Production implementations of Transport and Storage traits

use crate::{AgentError, Result};
use crate::{Storage, StorageStats, Transport};
use async_trait::async_trait;
use aura_journal::Serializable;
use aura_types::{AccountId, DeviceId};
use quinn::{ClientConfig, Connection, Endpoint, ServerConfig};
use redb::{Database, ReadTransaction, ReadableTable, TableDefinition, WriteTransaction};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use serde::{Deserialize, Serialize};
use snow::{Builder, HandshakeState, TransportState};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, Mutex, RwLock};
use tracing::{debug, error, info, warn};

/// Production transport implementation using QUIC networking
#[derive(Debug)]
pub struct ProductionTransport {
    device_id: DeviceId,
    endpoint: Arc<Mutex<Option<Endpoint>>>,
    connections: Arc<RwLock<HashMap<DeviceId, QuicConnection>>>,
    message_queue: Arc<RwLock<Vec<(DeviceId, Vec<u8>)>>>,
    bind_address: String,
    noise_pattern: &'static str,
}

#[derive(Debug, Clone)]
struct QuicConnection {
    connection: Connection,
    endpoint: String,
    last_seen: Instant,
    noise_state: Option<Arc<Mutex<TransportState>>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct NetworkMessage {
    from: DeviceId,
    to: DeviceId,
    payload: Vec<u8>,
    timestamp: u64,
    message_id: String,
}

impl ProductionTransport {
    /// Create a new production transport
    pub fn new(device_id: DeviceId, bind_address: String) -> Self {
        Self {
            device_id,
            endpoint: Arc::new(Mutex::new(None)),
            connections: Arc::new(RwLock::new(HashMap::new())),
            message_queue: Arc::new(RwLock::new(Vec::new())),
            bind_address,
            noise_pattern: "Noise_XX_25519_ChaChaPoly_BLAKE2s",
        }
    }

    /// Generate self-signed certificate for QUIC
    fn generate_self_signed_cert() -> Result<(CertificateDer<'static>, PrivateKeyDer<'static>)> {
        let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()]).map_err(|e| {
            AgentError::agent_invalid_state(format!("Failed to generate certificate: {}", e))
        })?;

        let cert_der = CertificateDer::from(cert.cert.der().clone());
        let private_key = PrivateKeyDer::try_from(cert.key_pair.serialize_der()).map_err(|e| {
            AgentError::agent_invalid_state(format!("Failed to serialize private key: {}", e))
        })?;

        Ok((cert_der, private_key))
    }

    /// Create QUIC server configuration
    fn create_server_config() -> Result<ServerConfig> {
        let (cert, key) = Self::generate_self_signed_cert()?;

        let mut server_config = ServerConfig::with_single_cert(vec![cert], key).map_err(|e| {
            AgentError::agent_invalid_state(format!("Failed to create server config: {}", e))
        })?;

        let transport_config = Arc::get_mut(&mut server_config.transport).unwrap();
        transport_config.max_concurrent_uni_streams(0_u8.into());
        transport_config.max_concurrent_bidi_streams(100_u8.into());
        transport_config.max_idle_timeout(Some(Duration::from_secs(30).try_into().unwrap()));

        Ok(server_config)
    }

    /// Create QUIC client configuration
    fn create_client_config() -> Result<ClientConfig> {
        use quinn::crypto::rustls::QuicClientConfig;

        let crypto = rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(SkipServerVerification::new())
            .with_no_client_auth();

        let mut client_config = ClientConfig::new(Arc::new(
            QuicClientConfig::try_from(crypto).map_err(|e| {
                AgentError::agent_invalid_state(format!(
                    "Failed to create QUIC client config: {}",
                    e
                ))
            })?,
        ));

        let mut transport_config = quinn::TransportConfig::default();
        transport_config.max_concurrent_uni_streams(0_u8.into());
        transport_config.max_concurrent_bidi_streams(100_u8.into());
        transport_config.max_idle_timeout(Some(Duration::from_secs(30).try_into().unwrap()));
        client_config.transport_config(Arc::new(transport_config));

        Ok(client_config)
    }

    /// Initialize the transport (start listening, etc.)
    pub async fn initialize(&self) -> Result<()> {
        info!(
            "Initializing QUIC transport for device {} on {}",
            self.device_id, self.bind_address
        );

        let server_config = Self::create_server_config()?;
        let bind_addr: SocketAddr = self
            .bind_address
            .parse()
            .map_err(|e| AgentError::agent_invalid_state(format!("Invalid bind address: {}", e)))?;

        let endpoint = Endpoint::server(server_config, bind_addr).map_err(|e| {
            AgentError::agent_invalid_state(format!("Failed to create QUIC endpoint: {}", e))
        })?;

        info!(
            "QUIC endpoint listening on {}",
            endpoint.local_addr().unwrap()
        );

        // Store endpoint
        {
            let mut ep = self.endpoint.lock().await;
            *ep = Some(endpoint.clone());
        }

        // Spawn task to handle incoming connections
        let connections = self.connections.clone();
        let device_id = self.device_id;

        tokio::spawn(async move {
            while let Some(incoming) = endpoint.accept().await {
                match incoming.await {
                    Ok(connection) => {
                        info!("Accepted connection from {}", connection.remote_address());
                        // TODO: Handle connection in separate task
                    }
                    Err(e) => {
                        warn!("Failed to accept connection: {}", e);
                    }
                }
            }
        });

        Ok(())
    }

    /// Shutdown the transport gracefully
    pub async fn shutdown(&self) -> Result<()> {
        info!("Shutting down QUIC transport for device {}", self.device_id);

        // Close all connections
        {
            let mut connections = self.connections.write().await;
            for (peer_id, conn) in connections.drain() {
                info!("Closing connection to peer {}", peer_id);
                conn.connection.close(0u32.into(), b"shutdown");
            }
        }

        // Close endpoint
        {
            let mut endpoint = self.endpoint.lock().await;
            if let Some(ep) = endpoint.take() {
                ep.close(0u32.into(), b"shutdown");
                info!("QUIC endpoint closed");
            }
        }

        Ok(())
    }
}

/// Skip server certificate verification for development
#[derive(Debug)]
struct SkipServerVerification;

impl SkipServerVerification {
    fn new() -> Arc<Self> {
        Arc::new(Self)
    }
}

impl rustls::client::danger::ServerCertVerifier for SkipServerVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> std::result::Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> std::result::Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> std::result::Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::RSA_PKCS1_SHA1,
            rustls::SignatureScheme::ECDSA_SHA1_Legacy,
            rustls::SignatureScheme::RSA_PKCS1_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::RSA_PKCS1_SHA384,
            rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
            rustls::SignatureScheme::RSA_PKCS1_SHA512,
            rustls::SignatureScheme::ECDSA_NISTP521_SHA512,
            rustls::SignatureScheme::RSA_PSS_SHA256,
            rustls::SignatureScheme::RSA_PSS_SHA384,
            rustls::SignatureScheme::RSA_PSS_SHA512,
            rustls::SignatureScheme::ED25519,
            rustls::SignatureScheme::ED448,
        ]
    }
}

#[async_trait]
impl Transport for ProductionTransport {
    fn device_id(&self) -> DeviceId {
        self.device_id
    }

    async fn send_message(&self, peer_id: DeviceId, message: &[u8]) -> Result<()> {
        let connections = self.connections.read().await;

        if let Some(quic_conn) = connections.get(&peer_id) {
            debug!(
                "Sending {} bytes to peer {} via QUIC",
                message.len(),
                peer_id
            );

            // Open bidirectional stream
            let (mut send, _recv) = quic_conn.connection.open_bi().await.map_err(|e| {
                AgentError::agent_invalid_state(format!("Failed to open stream: {}", e))
            })?;

            // Create network message envelope
            let network_msg = NetworkMessage {
                from: self.device_id,
                to: peer_id,
                payload: message.to_vec(),
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
                message_id: uuid::Uuid::new_v4().to_string(),
            };

            // Serialize and send
            let serialized = bincode::serialize(&network_msg).map_err(|e| {
                AgentError::agent_invalid_state(format!("Failed to serialize message: {}", e))
            })?;

            send.write_all(&serialized).await.map_err(|e| {
                AgentError::agent_invalid_state(format!("Failed to send message: {}", e))
            })?;

            send.finish().map_err(|e| {
                AgentError::agent_invalid_state(format!("Failed to finish send: {}", e))
            })?;

            debug!("Successfully sent message to peer {}", peer_id);
            Ok(())
        } else {
            Err(AgentError::agent_invalid_state(format!(
                "Not connected to peer {}",
                peer_id
            )))
        }
    }

    async fn receive_messages(&self) -> Result<Vec<(DeviceId, Vec<u8>)>> {
        // Return queued messages (populated by background tasks)
        let mut queue = self.message_queue.write().await;
        let messages: Vec<(DeviceId, Vec<u8>)> = queue.drain(..).collect();
        debug!("Retrieved {} queued messages", messages.len());
        Ok(messages)
    }

    async fn connect(&self, peer_id: DeviceId) -> Result<()> {
        let endpoint_guard = self.endpoint.lock().await;
        let endpoint = endpoint_guard.as_ref().ok_or_else(|| {
            AgentError::agent_invalid_state("Transport not initialized".to_string())
        })?;

        // Construct peer address (placeholder - should come from peer discovery)
        let peer_bytes = peer_id.to_bytes().map_err(|e| {
            AgentError::agent_invalid_state(format!("Failed to serialize peer ID: {}", e))
        })?;
        let peer_addr = format!("127.0.0.1:{}", 8080u16 + (peer_bytes[0] as u16 % 100));
        let peer_socket_addr: SocketAddr = peer_addr
            .parse()
            .map_err(|e| AgentError::agent_invalid_state(format!("Invalid peer address: {}", e)))?;

        info!("Connecting to peer {} at {}", peer_id, peer_socket_addr);

        // Create client config
        let client_config = Self::create_client_config()?;

        // Connect to peer
        let connection = endpoint
            .connect_with(client_config, peer_socket_addr, "localhost")
            .map_err(|e| {
                AgentError::agent_invalid_state(format!("Failed to initiate connection: {}", e))
            })?
            .await
            .map_err(|e| AgentError::agent_invalid_state(format!("Failed to connect: {}", e)))?;

        // Store connection
        let quic_conn = QuicConnection {
            connection,
            endpoint: peer_addr,
            last_seen: Instant::now(),
            noise_state: None, // Simplified without Noise for now
        };

        let mut connections = self.connections.write().await;
        connections.insert(peer_id, quic_conn);

        info!("Successfully connected to peer {}", peer_id);
        Ok(())
    }

    async fn disconnect(&self, peer_id: DeviceId) -> Result<()> {
        let mut connections = self.connections.write().await;

        if let Some(quic_conn) = connections.remove(&peer_id) {
            info!("Disconnecting from peer {}", peer_id);
            quic_conn.connection.close(0u32.into(), b"disconnect");
            info!("Disconnected from peer {}", peer_id);
        }

        Ok(())
    }

    async fn connected_peers(&self) -> Result<Vec<DeviceId>> {
        let connections = self.connections.read().await;
        Ok(connections.keys().cloned().collect())
    }

    async fn is_connected(&self, peer_id: DeviceId) -> Result<bool> {
        let connections = self.connections.read().await;
        Ok(connections.contains_key(&peer_id))
    }
}

/// Production storage implementation using persistent storage (redb)
#[derive(Debug)]
pub struct ProductionStorage {
    account_id: AccountId,
    storage_path: std::path::PathBuf,
    database: Arc<Mutex<Database>>,
}

// Define table for key-value storage
const DATA_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("data");
const METADATA_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("metadata");

#[derive(Debug, Serialize, Deserialize)]
struct StorageMetadata {
    created_at: u64,
    updated_at: u64,
    size_bytes: u64,
    checksum: String,
}

impl ProductionStorage {
    /// Create a new production storage
    pub fn new(account_id: AccountId, storage_path: impl Into<std::path::PathBuf>) -> Result<Self> {
        let storage_path = storage_path.into();

        // Create parent directories if they don't exist
        if let Some(parent) = storage_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                AgentError::agent_invalid_state(format!(
                    "Failed to create storage directory: {}",
                    e
                ))
            })?
        }

        // Open or create redb database
        let database = Database::create(&storage_path).map_err(|e| {
            AgentError::agent_invalid_state(format!(
                "Failed to create database at {:?}: {}",
                storage_path, e
            ))
        })?;

        // Initialize tables
        {
            let write_txn = database.begin_write().map_err(|e| {
                AgentError::agent_invalid_state(format!("Failed to begin write transaction: {}", e))
            })?;

            write_txn.open_table(DATA_TABLE).map_err(|e| {
                AgentError::agent_invalid_state(format!("Failed to open data table: {}", e))
            })?;

            write_txn.open_table(METADATA_TABLE).map_err(|e| {
                AgentError::agent_invalid_state(format!("Failed to open metadata table: {}", e))
            })?;

            write_txn.commit().map_err(|e| {
                AgentError::agent_invalid_state(format!("Failed to commit table creation: {}", e))
            })?;
        }

        Ok(Self {
            account_id,
            storage_path,
            database: Arc::new(Mutex::new(database)),
        })
    }

    /// Compute blake3 checksum of data
    fn compute_checksum(data: &[u8]) -> String {
        hex::encode(blake3::hash(data).as_bytes())
    }

    /// Initialize the storage (already done in constructor)
    pub async fn initialize(&self) -> Result<()> {
        info!(
            "Production storage already initialized at {:?}",
            self.storage_path
        );
        Ok(())
    }

    /// Cleanup and close storage
    pub async fn cleanup(&self) -> Result<()> {
        info!("Cleaning up production storage");
        // redb automatically handles cleanup when dropped
        Ok(())
    }

    /// Backup storage to a specified location
    pub async fn backup(&self, backup_path: impl Into<std::path::PathBuf>) -> Result<()> {
        let backup_path = backup_path.into();
        info!("Creating storage backup to {:?}", backup_path);

        // Create backup directory
        if let Some(parent) = backup_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                AgentError::agent_invalid_state(format!("Failed to create backup directory: {}", e))
            })?
        }

        // Copy database file
        std::fs::copy(&self.storage_path, &backup_path).map_err(|e| {
            AgentError::agent_invalid_state(format!("Failed to copy database: {}", e))
        })?;

        info!("Storage backup completed");
        Ok(())
    }
}

#[async_trait]
impl Storage for ProductionStorage {
    fn account_id(&self) -> AccountId {
        self.account_id
    }

    async fn store(&self, key: &str, data: &[u8]) -> Result<()> {
        let database = self.database.lock().await;

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let metadata = StorageMetadata {
            created_at: timestamp,
            updated_at: timestamp,
            size_bytes: data.len() as u64,
            checksum: Self::compute_checksum(data),
        };

        let metadata_bytes = bincode::serialize(&metadata).map_err(|e| {
            AgentError::agent_invalid_state(format!("Failed to serialize metadata: {}", e))
        })?;

        let write_txn = database.begin_write().map_err(|e| {
            AgentError::agent_invalid_state(format!("Failed to begin write transaction: {}", e))
        })?;

        {
            let mut data_table = write_txn.open_table(DATA_TABLE).map_err(|e| {
                AgentError::agent_invalid_state(format!("Failed to open data table: {}", e))
            })?;

            data_table.insert(key, data).map_err(|e| {
                AgentError::agent_invalid_state(format!("Failed to insert data: {}", e))
            })?;
        }

        {
            let mut metadata_table = write_txn.open_table(METADATA_TABLE).map_err(|e| {
                AgentError::agent_invalid_state(format!("Failed to open metadata table: {}", e))
            })?;

            metadata_table
                .insert(key, metadata_bytes.as_slice())
                .map_err(|e| {
                    AgentError::agent_invalid_state(format!("Failed to insert metadata: {}", e))
                })?;
        }

        write_txn.commit().map_err(|e| {
            AgentError::agent_invalid_state(format!("Failed to commit transaction: {}", e))
        })?;

        debug!(
            "Stored {} bytes at key '{}' for account {} (checksum: {})",
            data.len(),
            key,
            self.account_id,
            metadata.checksum
        );
        Ok(())
    }

    async fn retrieve(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let database = self.database.lock().await;

        let read_txn = database.begin_read().map_err(|e| {
            AgentError::agent_invalid_state(format!("Failed to begin read transaction: {}", e))
        })?;

        let data_table = read_txn.open_table(DATA_TABLE).map_err(|e| {
            AgentError::agent_invalid_state(format!("Failed to open data table: {}", e))
        })?;

        let result = match data_table.get(key) {
            Ok(Some(data)) => {
                let bytes = data.value().to_vec();
                debug!(
                    "Retrieved {} bytes from key '{}' for account {}",
                    bytes.len(),
                    key,
                    self.account_id
                );
                Some(bytes)
            }
            Ok(None) => {
                debug!("Key '{}' not found for account {}", key, self.account_id);
                None
            }
            Err(e) => {
                return Err(AgentError::agent_invalid_state(format!(
                    "Failed to retrieve data: {}",
                    e
                )));
            }
        };

        Ok(result)
    }

    async fn delete(&self, key: &str) -> Result<()> {
        let database = self.database.lock().await;

        let write_txn = database.begin_write().map_err(|e| {
            AgentError::agent_invalid_state(format!("Failed to begin write transaction: {}", e))
        })?;

        {
            let mut data_table = write_txn.open_table(DATA_TABLE).map_err(|e| {
                AgentError::agent_invalid_state(format!("Failed to open data table: {}", e))
            })?;

            data_table.remove(key).map_err(|e| {
                AgentError::agent_invalid_state(format!("Failed to delete data: {}", e))
            })?;
        }

        {
            let mut metadata_table = write_txn.open_table(METADATA_TABLE).map_err(|e| {
                AgentError::agent_invalid_state(format!("Failed to open metadata table: {}", e))
            })?;

            metadata_table.remove(key).map_err(|e| {
                AgentError::agent_invalid_state(format!("Failed to delete metadata: {}", e))
            })?;
        }

        write_txn.commit().map_err(|e| {
            AgentError::agent_invalid_state(format!("Failed to commit transaction: {}", e))
        })?;

        debug!("Deleted key '{}' for account {}", key, self.account_id);
        Ok(())
    }

    async fn list_keys(&self) -> Result<Vec<String>> {
        let database = self.database.lock().await;

        let read_txn = database.begin_read().map_err(|e| {
            AgentError::agent_invalid_state(format!("Failed to begin read transaction: {}", e))
        })?;

        let data_table = read_txn.open_table(DATA_TABLE).map_err(|e| {
            AgentError::agent_invalid_state(format!("Failed to open data table: {}", e))
        })?;

        let mut keys = Vec::new();
        let mut iter = data_table.iter().map_err(|e| {
            AgentError::agent_invalid_state(format!("Failed to create iterator: {}", e))
        })?;

        while let Some(result) = iter.next() {
            let (key, _) = result.map_err(|e| {
                AgentError::agent_invalid_state(format!("Failed to read key: {}", e))
            })?;
            keys.push(key.value().to_string());
        }

        debug!("Listed {} keys for account {}", keys.len(), self.account_id);
        Ok(keys)
    }

    async fn exists(&self, key: &str) -> Result<bool> {
        let database = self.database.lock().await;

        let read_txn = database.begin_read().map_err(|e| {
            AgentError::agent_invalid_state(format!("Failed to begin read transaction: {}", e))
        })?;

        let data_table = read_txn.open_table(DATA_TABLE).map_err(|e| {
            AgentError::agent_invalid_state(format!("Failed to open data table: {}", e))
        })?;

        let exists = data_table
            .get(key)
            .map_err(|e| {
                AgentError::agent_invalid_state(format!("Failed to check key existence: {}", e))
            })?
            .is_some();

        Ok(exists)
    }

    async fn stats(&self) -> Result<StorageStats> {
        let database = self.database.lock().await;

        let read_txn = database.begin_read().map_err(|e| {
            AgentError::agent_invalid_state(format!("Failed to begin read transaction: {}", e))
        })?;

        let metadata_table = read_txn.open_table(METADATA_TABLE).map_err(|e| {
            AgentError::agent_invalid_state(format!("Failed to open metadata table: {}", e))
        })?;

        let mut total_keys = 0u64;
        let mut total_size_bytes = 0u64;

        let mut iter = metadata_table.iter().map_err(|e| {
            AgentError::agent_invalid_state(format!("Failed to create iterator: {}", e))
        })?;

        while let Some(result) = iter.next() {
            let (_, metadata_bytes) = result.map_err(|e| {
                AgentError::agent_invalid_state(format!("Failed to read metadata: {}", e))
            })?;

            if let Ok(metadata) = bincode::deserialize::<StorageMetadata>(metadata_bytes.value()) {
                total_keys += 1;
                total_size_bytes += metadata.size_bytes;
            }
        }

        // Get filesystem stats for available space
        let available_space_bytes = if self.storage_path.exists() {
            // For simplicity, use a placeholder value
            // In production, you'd use platform-specific APIs
            Some(1_000_000_000) // 1GB placeholder
        } else {
            None
        };

        debug!(
            "Storage stats for account {}: {} keys, {} bytes",
            self.account_id, total_keys, total_size_bytes
        );

        Ok(StorageStats {
            total_keys,
            total_size_bytes,
            available_space_bytes,
        })
    }
}

/// Factory for creating production transport and storage
pub struct ProductionFactory;

impl ProductionFactory {
    /// Create a production transport instance
    pub async fn create_transport(
        device_id: DeviceId,
        bind_address: String,
    ) -> Result<ProductionTransport> {
        let transport = ProductionTransport::new(device_id, bind_address);
        transport.initialize().await?;
        Ok(transport)
    }

    /// Create a production storage instance
    pub async fn create_storage(
        account_id: AccountId,
        storage_path: impl Into<std::path::PathBuf>,
    ) -> Result<ProductionStorage> {
        let storage = ProductionStorage::new(account_id, storage_path)?;
        storage.initialize().await?;
        Ok(storage)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_production_transport() {
        let device_id = DeviceId::new();
        let bind_address = "127.0.0.1:0".to_string();

        let transport = ProductionFactory::create_transport(device_id, bind_address)
            .await
            .unwrap();

        // Test basic functionality
        assert_eq!(transport.device_id(), device_id);

        let peer_id = DeviceId::new();
        assert!(!transport.is_connected(peer_id).await.unwrap());

        // Note: Actual connection would require a running peer
        // transport.connect(peer_id).await.unwrap();
        // assert!(transport.is_connected(peer_id).await.unwrap());

        let peers = transport.connected_peers().await.unwrap();
        assert_eq!(peers.len(), 0);

        transport.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn test_production_storage() {
        let account_id = AccountId::new();
        let temp_dir = TempDir::new().unwrap();
        let storage_path = temp_dir.path().join("storage.db");

        let storage = ProductionFactory::create_storage(account_id, storage_path)
            .await
            .unwrap();

        // Test basic functionality
        assert_eq!(storage.account_id(), account_id);

        let key = "test_key";
        let data = b"test data";

        assert!(!storage.exists(key).await.unwrap());
        storage.store(key, data).await.unwrap();
        assert!(storage.exists(key).await.unwrap());

        let retrieved = storage.retrieve(key).await.unwrap().unwrap();
        assert_eq!(retrieved, data);

        let keys = storage.list_keys().await.unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0], key);

        let stats = storage.stats().await.unwrap();
        assert_eq!(stats.total_keys, 1);
        assert_eq!(stats.total_size_bytes, data.len() as u64);

        storage.delete(key).await.unwrap();
        assert!(!storage.exists(key).await.unwrap());

        storage.cleanup().await.unwrap();
    }
}
