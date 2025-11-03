//! Discovery Middleware

use super::stack::TransportMiddleware;
use super::handler::{TransportHandler, TransportOperation, TransportResult, NetworkAddress, PeerInfo};
use aura_types::effects::AuraEffects;
use aura_types::{MiddlewareContext, MiddlewareResult};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct DiscoveryConfig {
    pub discovery_interval_ms: u64,
    pub peer_timeout_ms: u64,
    pub max_peers: usize,
    pub enable_mdns: bool,
    pub enable_dht: bool,
    pub bootstrap_peers: Vec<NetworkAddress>,
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        Self {
            discovery_interval_ms: 30000, // 30 seconds
            peer_timeout_ms: 300000, // 5 minutes
            max_peers: 100,
            enable_mdns: true,
            enable_dht: true,
            bootstrap_peers: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
struct CachedPeer {
    info: PeerInfo,
    last_seen: u64,
    connection_attempts: u32,
    last_attempt: u64,
}

impl CachedPeer {
    fn new(info: PeerInfo, current_time: u64) -> Self {
        Self {
            info,
            last_seen: current_time,
            connection_attempts: 0,
            last_attempt: 0,
        }
    }
    
    fn is_expired(&self, current_time: u64, timeout_ms: u64) -> bool {
        current_time.saturating_sub(self.last_seen) > timeout_ms
    }
    
    fn should_retry_connection(&self, current_time: u64, retry_interval_ms: u64) -> bool {
        if self.connection_attempts == 0 {
            return true;
        }
        
        // Exponential backoff
        let backoff_ms = retry_interval_ms * (1 << self.connection_attempts.min(10));
        current_time.saturating_sub(self.last_attempt) > backoff_ms
    }
}

pub struct DiscoveryMiddleware {
    config: DiscoveryConfig,
    discovered_peers: HashMap<NetworkAddress, CachedPeer>,
    last_discovery: u64,
    local_capabilities: Vec<String>,
    stats: DiscoveryStats,
}

#[derive(Debug, Default)]
struct DiscoveryStats {
    discovery_attempts: u64,
    peers_discovered: u64,
    peers_expired: u64,
    successful_connections: u64,
    failed_connections: u64,
}

impl DiscoveryMiddleware {
    pub fn new() -> Self {
        Self {
            config: DiscoveryConfig::default(),
            discovered_peers: HashMap::new(),
            last_discovery: 0,
            local_capabilities: vec!["transport".to_string(), "storage".to_string()],
            stats: DiscoveryStats::default(),
        }
    }
    
    pub fn with_config(config: DiscoveryConfig) -> Self {
        Self {
            config,
            discovered_peers: HashMap::new(),
            last_discovery: 0,
            local_capabilities: vec!["transport".to_string(), "storage".to_string()],
            stats: DiscoveryStats::default(),
        }
    }
    
    pub fn with_capabilities(mut self, capabilities: Vec<String>) -> Self {
        self.local_capabilities = capabilities;
        self
    }
    
    fn should_discover(&self, current_time: u64) -> bool {
        current_time.saturating_sub(self.last_discovery) >= self.config.discovery_interval_ms
    }
    
    fn cleanup_expired_peers(&mut self, current_time: u64) {
        let initial_count = self.discovered_peers.len();
        self.discovered_peers.retain(|_, peer| {
            !peer.is_expired(current_time, self.config.peer_timeout_ms)
        });
        let removed = initial_count - self.discovered_peers.len();
        if removed > 0 {
            self.stats.peers_expired += removed as u64;
        }
    }
    
    fn simulate_mdns_discovery(&self, effects: &dyn AuraEffects) -> Vec<PeerInfo> {
        // Simulate mDNS discovery
        let current_time = effects.current_timestamp();
        let _device_id = effects.device_id();
        
        // Generate some fake local network peers
        vec![
            PeerInfo {
                address: NetworkAddress::Tcp("192.168.1.10:8080".parse().unwrap()),
                capabilities: vec!["storage".to_string(), "relay".to_string()],
                metadata: {
                    let mut meta = HashMap::new();
                    meta.insert("discovery_method".to_string(), "mdns".to_string());
                    meta.insert("device_type".to_string(), "peer".to_string());
                    meta
                },
                last_seen: current_time,
            },
            PeerInfo {
                address: NetworkAddress::Tcp("192.168.1.20:8080".parse().unwrap()),
                capabilities: vec!["transport".to_string(), "communication".to_string()],
                metadata: {
                    let mut meta = HashMap::new();
                    meta.insert("discovery_method".to_string(), "mdns".to_string());
                    meta.insert("device_type".to_string(), "relay".to_string());
                    meta
                },
                last_seen: current_time,
            },
        ]
    }
    
    fn simulate_dht_discovery(&self, effects: &dyn AuraEffects) -> Vec<PeerInfo> {
        // Simulate DHT discovery
        let current_time = effects.current_timestamp();
        
        // Generate some fake DHT peers
        vec![
            PeerInfo {
                address: NetworkAddress::Peer("12D3KooWPjceQrSwdWXPyLLeABRXmuqt69Rg3sBYbU1Nft9HyQ6X".to_string()),
                capabilities: vec!["dht".to_string(), "storage".to_string()],
                metadata: {
                    let mut meta = HashMap::new();
                    meta.insert("discovery_method".to_string(), "dht".to_string());
                    meta.insert("protocol".to_string(), "libp2p".to_string());
                    meta
                },
                last_seen: current_time,
            },
            PeerInfo {
                address: NetworkAddress::Peer("12D3KooWQYz3w8nJ1MkXbGW2UJVz8U5QeH6Y9B3K9L7M6N8P9Q0R".to_string()),
                capabilities: vec!["communication".to_string(), "relay".to_string()],
                metadata: {
                    let mut meta = HashMap::new();
                    meta.insert("discovery_method".to_string(), "dht".to_string());
                    meta.insert("protocol".to_string(), "kad-dht".to_string());
                    meta
                },
                last_seen: current_time,
            },
        ]
    }
    
    fn perform_discovery(&mut self, effects: &dyn AuraEffects) -> Vec<PeerInfo> {
        let current_time = effects.current_timestamp();
        self.last_discovery = current_time;
        self.stats.discovery_attempts += 1;
        
        let mut discovered = Vec::new();
        
        // mDNS discovery
        if self.config.enable_mdns {
            let mdns_peers = self.simulate_mdns_discovery(effects);
            discovered.extend(mdns_peers);
        }
        
        // DHT discovery
        if self.config.enable_dht {
            let dht_peers = self.simulate_dht_discovery(effects);
            discovered.extend(dht_peers);
        }
        
        // Add bootstrap peers if we don't have enough peers
        if self.discovered_peers.len() < 5 {
            for bootstrap_addr in &self.config.bootstrap_peers {
                discovered.push(PeerInfo {
                    address: bootstrap_addr.clone(),
                    capabilities: vec!["bootstrap".to_string()],
                    metadata: {
                        let mut meta = HashMap::new();
                        meta.insert("discovery_method".to_string(), "bootstrap".to_string());
                        meta
                    },
                    last_seen: current_time,
                });
            }
        }
        
        // Cache discovered peers
        for peer in &discovered {
            if self.discovered_peers.len() < self.config.max_peers {
                let cached_peer = CachedPeer::new(peer.clone(), current_time);
                self.discovered_peers.insert(peer.address.clone(), cached_peer);
                self.stats.peers_discovered += 1;
            }
        }
        
        effects.log_info(
            &format!("Discovered {} peers via mDNS/DHT", discovered.len()),
            &[]
        );
        
        discovered
    }
}

impl Default for DiscoveryMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

impl TransportMiddleware for DiscoveryMiddleware {
    fn process(
        &mut self,
        operation: TransportOperation,
        _context: &MiddlewareContext,
        effects: &dyn AuraEffects,
        next: &mut dyn TransportHandler,
    ) -> MiddlewareResult<TransportResult> {
        let current_time = effects.current_timestamp();
        
        // Cleanup expired peers periodically
        if current_time % 60000 == 0 { // Every minute
            self.cleanup_expired_peers(current_time);
        }
        
        match operation {
            TransportOperation::Discover { criteria } => {
                // Always perform fresh discovery for explicit discover requests
                let discovered = self.perform_discovery(effects);
                
                // Filter based on criteria
                let filtered_peers = if criteria.protocol.is_some() || !criteria.capabilities.is_empty() {
                    discovered.into_iter().filter(|peer| {
                        // Check protocol match
                        let protocol_match = if let Some(ref protocol) = criteria.protocol {
                            peer.metadata.get("protocol")
                                .map(|p| p == protocol)
                                .unwrap_or(false)
                        } else {
                            true
                        };
                        
                        // Check capabilities match
                        let capabilities_match = if criteria.capabilities.is_empty() {
                            true
                        } else {
                            criteria.capabilities.iter().any(|required| {
                                peer.capabilities.contains(required)
                            })
                        };
                        
                        protocol_match && capabilities_match
                    }).collect()
                } else {
                    discovered
                };
                
                // Limit results if requested
                let final_peers = if let Some(max_results) = criteria.max_results {
                    filtered_peers.into_iter()
                        .take(max_results as usize)
                        .collect()
                } else {
                    filtered_peers
                };
                
                Ok(TransportResult::Discovered {
                    peers: final_peers,
                })
            }
            
            TransportOperation::Connect { address, options } => {
                // Track connection attempts for discovered peers
                if let Some(cached_peer) = self.discovered_peers.get_mut(&address) {
                    cached_peer.connection_attempts += 1;
                    cached_peer.last_attempt = current_time;
                }
                
                let result = next.execute(TransportOperation::Connect { address: address.clone(), options }, effects);
                
                // Update stats based on result
                match &result {
                    Ok(TransportResult::Connected { .. }) => {
                        self.stats.successful_connections += 1;
                        if let Some(cached_peer) = self.discovered_peers.get_mut(&address) {
                            cached_peer.last_seen = current_time;
                        }
                    }
                    Err(_) => {
                        self.stats.failed_connections += 1;
                    }
                    _ => {}
                }
                
                result
            }
            
            _ => {
                // Perform background discovery if needed
                if self.should_discover(current_time) {
                    self.perform_discovery(effects);
                }
                
                next.execute(operation, effects)
            }
        }
    }
    
    fn middleware_name(&self) -> &'static str {
        "DiscoveryMiddleware"
    }
    
    fn middleware_info(&self) -> HashMap<String, String> {
        let mut info = HashMap::new();
        info.insert("discovery_interval_ms".to_string(), self.config.discovery_interval_ms.to_string());
        info.insert("peer_timeout_ms".to_string(), self.config.peer_timeout_ms.to_string());
        info.insert("max_peers".to_string(), self.config.max_peers.to_string());
        info.insert("enable_mdns".to_string(), self.config.enable_mdns.to_string());
        info.insert("enable_dht".to_string(), self.config.enable_dht.to_string());
        info.insert("cached_peers".to_string(), self.discovered_peers.len().to_string());
        info.insert("discovery_attempts".to_string(), self.stats.discovery_attempts.to_string());
        info.insert("peers_discovered".to_string(), self.stats.peers_discovered.to_string());
        info.insert("peers_expired".to_string(), self.stats.peers_expired.to_string());
        info.insert("successful_connections".to_string(), self.stats.successful_connections.to_string());
        info.insert("failed_connections".to_string(), self.stats.failed_connections.to_string());
        info
    }
}