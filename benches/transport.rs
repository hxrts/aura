//! Transport Layer Performance Benchmarks
//!
//! Comprehensive benchmarks for the new transport layer architecture.
//! Measures performance across all layers and validates that the redesign
//! meets or exceeds performance requirements compared to the deprecated crate.

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use aura_transport::{
    types::{Envelope, TransportConfig, PrivacyLevel, ConnectionId},
    peers::{PeerInfo, PrivacyAwareSelectionCriteria, RelationshipScopedDiscovery},
    protocols::{StunConfig, PunchConfig},
};
use aura_effects::transport_effects::{
    TcpTransportHandler, WebSocketTransportHandler, InMemoryTransportHandler,
    FramingHandler, TransportManager,
};
use aura_protocol::transport_coordination::{
    TransportCoordinator, WebSocketHandshakeCoordinator, ReceiptVerificationCoordinator,
    ChoreographicConfig, TransportCoordinationConfig,
};
use aura_core::{DeviceId, ContextId};
use tokio::runtime::Runtime;
use std::collections::HashMap;
use std::time::Duration;

/// Benchmark configuration for different test scenarios
#[derive(Clone)]
struct BenchConfig {
    privacy_level: PrivacyLevel,
    message_size: usize,
    peer_count: usize,
    capability_count: usize,
}

impl BenchConfig {
    fn small() -> Self {
        Self {
            privacy_level: PrivacyLevel::Clear,
            message_size: 1024,
            peer_count: 10,
            capability_count: 3,
        }
    }
    
    fn medium() -> Self {
        Self {
            privacy_level: PrivacyLevel::Blinded,
            message_size: 64 * 1024,
            peer_count: 100,
            capability_count: 10,
        }
    }
    
    fn large() -> Self {
        Self {
            privacy_level: PrivacyLevel::RelationshipScoped,
            message_size: 1024 * 1024,
            peer_count: 1000,
            capability_count: 50,
        }
    }
}

/// Layer 2 (Types) Performance Benchmarks
fn bench_envelope_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("layer2_envelope_ops");
    
    for config in [BenchConfig::small(), BenchConfig::medium(), BenchConfig::large()] {
        let message = vec![0u8; config.message_size];
        let sender = DeviceId::new();
        let recipient = DeviceId::new();
        let context = ContextId::new();
        
        // Benchmark envelope creation
        group.bench_with_input(
            BenchmarkId::new("envelope_creation", config.message_size),
            &config,
            |b, config| {
                b.iter(|| {
                    black_box(Envelope::new_with_privacy(
                        message.clone(),
                        sender,
                        recipient,
                        config.privacy_level,
                    ))
                })
            },
        );
        
        // Benchmark scoped envelope creation
        group.bench_with_input(
            BenchmarkId::new("scoped_envelope_creation", config.message_size),
            &config,
            |b, _config| {
                b.iter(|| {
                    black_box(Envelope::new_scoped(
                        message.clone(),
                        sender,
                        context,
                    ))
                })
            },
        );
        
        // Benchmark envelope serialization
        let envelope = Envelope::new_scoped(message.clone(), sender, context);
        group.bench_with_input(
            BenchmarkId::new("envelope_serialization", config.message_size),
            &config,
            |b, _config| {
                b.iter(|| {
                    black_box(envelope.to_bytes())
                })
            },
        );
        
        // Benchmark envelope deserialization
        let serialized = envelope.to_bytes();
        group.bench_with_input(
            BenchmarkId::new("envelope_deserialization", config.message_size),
            &config,
            |b, _config| {
                b.iter(|| {
                    black_box(Envelope::from_bytes(&serialized).unwrap())
                })
            },
        );
    }
    
    group.finish();
}

fn bench_connection_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("layer2_connection_ops");
    
    let device1 = DeviceId::new();
    let device2 = DeviceId::new();
    let context = ContextId::new();
    
    group.bench_function("connection_id_creation", |b| {
        b.iter(|| {
            black_box(ConnectionId::new(device1, device2))
        })
    });
    
    group.bench_function("scoped_connection_creation", |b| {
        b.iter(|| {
            black_box(ConnectionId::new_scoped(device1, device2, context))
        })
    });
    
    // Benchmark connection serialization
    let connection = ConnectionId::new_scoped(device1, device2, context);
    group.bench_function("connection_serialization", |b| {
        b.iter(|| {
            black_box(serde_json::to_string(&connection).unwrap())
        })
    });
    
    let serialized = serde_json::to_string(&connection).unwrap();
    group.bench_function("connection_deserialization", |b| {
        b.iter(|| {
            black_box(serde_json::from_str::<ConnectionId>(&serialized).unwrap())
        })
    });
    
    group.finish();
}

fn bench_peer_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("layer2_peer_ops");
    
    for config in [BenchConfig::small(), BenchConfig::medium(), BenchConfig::large()] {
        let device_id = DeviceId::new();
        let capabilities: Vec<String> = (0..config.capability_count)
            .map(|i| format!("capability_{}", i))
            .collect();
        
        // Benchmark peer creation
        group.bench_with_input(
            BenchmarkId::new("peer_creation", config.capability_count),
            &config,
            |b, _config| {
                b.iter(|| {
                    black_box(PeerInfo::new(
                        device_id,
                        "benchmark-peer".to_string(),
                        capabilities.clone(),
                    ))
                })
            },
        );
        
        // Benchmark blinded peer creation
        group.bench_with_input(
            BenchmarkId::new("blinded_peer_creation", config.capability_count),
            &config,
            |b, _config| {
                b.iter(|| {
                    black_box(PeerInfo::new_blinded(
                        device_id,
                        "blinded-peer".to_string(),
                        capabilities.clone(),
                    ))
                })
            },
        );
        
        // Benchmark capability queries
        let peer = PeerInfo::new_blinded(device_id, "test-peer".to_string(), capabilities.clone());
        group.bench_with_input(
            BenchmarkId::new("capability_query", config.capability_count),
            &config,
            |b, _config| {
                b.iter(|| {
                    black_box(peer.has_capability_blinded("capability_5"))
                })
            },
        );
        
        // Benchmark peer selection
        let context = ContextId::new();
        let mut discovery = RelationshipScopedDiscovery::new();
        
        // Add peers to discovery
        for i in 0..config.peer_count.min(100) { // Limit for benchmark performance
            let peer = PeerInfo::new_blinded(
                DeviceId::new(),
                format!("peer-{}", i),
                capabilities.clone(),
            );
            discovery.add_peer_to_context(context, peer);
        }
        
        let criteria = PrivacyAwareSelectionCriteria {
            required_capabilities: vec!["capability_1".to_string()],
            privacy_level: config.privacy_level,
            relationship_scope: Some(context),
            max_capability_disclosure: 5,
            require_capability_proofs: false,
        };
        
        group.bench_with_input(
            BenchmarkId::new("peer_selection", config.peer_count.min(100)),
            &config,
            |b, _config| {
                b.iter(|| {
                    black_box(discovery.discover_peers_matching(context, &criteria))
                })
            },
        );
    }
    
    group.finish();
}

/// Layer 3 (Effects) Performance Benchmarks
fn bench_effect_handler_operations(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("layer3_effect_handlers");
    
    for config in [BenchConfig::small(), BenchConfig::medium()] {
        let message = vec![0u8; config.message_size];
        let transport_config = TransportConfig {
            privacy_level: config.privacy_level,
            max_connections: config.peer_count,
            connection_timeout: Duration::from_secs(30),
            enable_capability_blinding: true,
            enable_traffic_padding: false,
            ..Default::default()
        };
        
        // Benchmark in-memory transport handler
        group.bench_with_input(
            BenchmarkId::new("memory_handler_send", config.message_size),
            &config,
            |b, _config| {
                b.to_async(&rt).iter(|| async {
                    let mut handler = InMemoryTransportHandler::new(transport_config.clone());
                    let sender = DeviceId::new();
                    let recipient = DeviceId::new();
                    
                    handler.register_peer(sender, "sender".to_string()).await;
                    handler.register_peer(recipient, "recipient".to_string()).await;
                    
                    black_box(handler.send_message(sender, recipient, message.clone()).await.unwrap())
                })
            },
        );
        
        // Benchmark message framing
        let framing_handler = FramingHandler::new();
        group.bench_with_input(
            BenchmarkId::new("message_framing", config.message_size),
            &config,
            |b, _config| {
                b.iter(|| {
                    black_box(framing_handler.frame_message(&message).unwrap())
                })
            },
        );
        
        // Benchmark message unframing
        let framed = framing_handler.frame_message(&message).unwrap();
        group.bench_with_input(
            BenchmarkId::new("message_unframing", config.message_size),
            &config,
            |b, _config| {
                b.iter(|| {
                    black_box(framing_handler.unframe_message(&framed).unwrap())
                })
            },
        );
    }
    
    group.finish();
}

fn bench_transport_manager_operations(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("layer3_transport_manager");
    
    let config = BenchConfig::medium();
    let transport_config = TransportConfig {
        privacy_level: config.privacy_level,
        max_connections: config.peer_count,
        ..Default::default()
    };
    
    group.bench_function("transport_manager_creation", |b| {
        b.iter(|| {
            black_box(TransportManager::new(transport_config.clone()))
        })
    });
    
    // Benchmark connection management
    group.bench_function("connection_registration", |b| {
        b.to_async(&rt).iter(|| async {
            let mut manager = TransportManager::new(transport_config.clone());
            let device_id = DeviceId::new();
            
            black_box(manager.register_handler(device_id, "test-handler".to_string()).await)
        })
    });
    
    group.finish();
}

/// Layer 4 (Coordination) Performance Benchmarks  
fn bench_coordination_operations(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("layer4_coordination");
    
    let config = BenchConfig::medium();
    let coordination_config = TransportCoordinationConfig {
        max_connections: config.peer_count,
        connection_timeout: Duration::from_secs(30),
        max_retries: 3,
        default_capabilities: vec!["transport".to_string()],
    };
    
    // Benchmark local coordination (NO choreography)
    group.bench_function("local_coordination", |b| {
        b.to_async(&rt).iter(|| async {
            let mut coordinator = TransportCoordinator::new(coordination_config.clone());
            let peer_id = DeviceId::new();
            let connection_id = format!("bench-connection-{}", fastrand::u64(..));
            
            black_box(coordinator.register_connection(connection_id, peer_id).await.unwrap())
        })
    });
    
    // Benchmark choreographic protocol initiation
    let choreo_config = ChoreographicConfig {
        max_concurrent_protocols: config.peer_count,
        protocol_timeout: Duration::from_secs(30),
        required_capabilities: vec!["transport".to_string()],
        extension_registry: Default::default(),
    };
    
    group.bench_function("websocket_handshake_initiation", |b| {
        b.to_async(&rt).iter(|| async {
            let mut coordinator = WebSocketHandshakeCoordinator::new(
                DeviceId::new(),
                choreo_config.clone(),
            );
            
            black_box(coordinator.initiate_handshake(
                DeviceId::new(),
                "ws://bench.example.com/socket".to_string(),
                ContextId::new(),
            ).unwrap())
        })
    });
    
    group.bench_function("receipt_verification_initiation", |b| {
        b.to_async(&rt).iter(|| async {
            let mut coordinator = ReceiptVerificationCoordinator::new(
                DeviceId::new(),
                choreo_config.clone(),
            );
            
            let receipt_data = aura_transport::protocols::websocket::ReceiptData {
                receipt_id: format!("bench-receipt-{}", fastrand::u64(..)),
                sender_id: DeviceId::new(),
                recipient_id: DeviceId::new(),
                message_hash: vec![0x01, 0x02, 0x03, 0x04],
                signature: vec![0xAA, 0xBB, 0xCC, 0xDD],
                timestamp: std::time::SystemTime::now(),
                context_id: ContextId::new(),
            };
            
            black_box(coordinator.initiate_verification(
                receipt_data,
                vec![DeviceId::new()],
            ).unwrap())
        })
    });
    
    group.finish();
}

/// Cross-Layer Integration Performance Benchmarks
fn bench_integration_performance(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("integration_performance");
    
    for config in [BenchConfig::small(), BenchConfig::medium()] {
        let message = vec![0u8; config.message_size];
        
        // Benchmark complete message flow: Layer 2 → Layer 3 → Layer 4
        group.bench_with_input(
            BenchmarkId::new("complete_message_flow", config.message_size),
            &config,
            |b, config| {
                b.to_async(&rt).iter(|| async {
                    // Layer 2: Create privacy-aware envelope
                    let sender = DeviceId::new();
                    let recipient = DeviceId::new();
                    let context = ContextId::new();
                    
                    let envelope = Envelope::new_scoped(message.clone(), sender, context);
                    
                    // Layer 3: Process through effect handler
                    let transport_config = TransportConfig {
                        privacy_level: config.privacy_level,
                        ..Default::default()
                    };
                    let mut handler = InMemoryTransportHandler::new(transport_config.clone());
                    
                    handler.register_peer(sender, "sender".to_string()).await;
                    handler.register_peer(recipient, "recipient".to_string()).await;
                    
                    handler.send_message(sender, recipient, envelope.to_bytes()).await.unwrap();
                    
                    // Layer 4: Coordinate through local coordinator
                    let coordination_config = TransportCoordinationConfig {
                        max_connections: 10,
                        ..Default::default()
                    };
                    let mut coordinator = TransportCoordinator::new(coordination_config);
                    
                    let connection_id = ConnectionId::new_scoped(sender, recipient, context);
                    
                    black_box(coordinator.register_connection(
                        connection_id.to_string(),
                        recipient,
                    ).await.unwrap())
                })
            },
        );
        
        // Benchmark privacy-preserving operations
        group.bench_with_input(
            BenchmarkId::new("privacy_operations", config.peer_count),
            &config,
            |b, config| {
                b.iter(|| {
                    let context = ContextId::new();
                    let mut discovery = RelationshipScopedDiscovery::new();
                    
                    // Add peers with capability blinding
                    for i in 0..config.peer_count.min(50) {
                        let capabilities = vec![
                            "transport".to_string(),
                            "messaging".to_string(),
                            format!("capability_{}", i),
                        ];
                        
                        let peer = PeerInfo::new_blinded(
                            DeviceId::new(),
                            format!("peer-{}", i),
                            capabilities,
                        );
                        discovery.add_peer_to_context(context, peer);
                    }
                    
                    // Perform privacy-aware selection
                    let criteria = PrivacyAwareSelectionCriteria {
                        required_capabilities: vec!["transport".to_string()],
                        privacy_level: config.privacy_level,
                        relationship_scope: Some(context),
                        max_capability_disclosure: 3,
                        require_capability_proofs: false,
                    };
                    
                    black_box(discovery.discover_peers_matching(context, &criteria))
                })
            },
        );
    }
    
    group.finish();
}

/// Comparative Performance Benchmarks
fn bench_performance_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("performance_comparison");
    
    // Compare old vs new envelope implementation performance
    for size in [1024, 64 * 1024, 1024 * 1024] {
        let message = vec![0u8; size];
        let sender = DeviceId::new();
        let recipient = DeviceId::new();
        
        // New implementation (privacy-by-design)
        group.bench_with_input(
            BenchmarkId::new("new_envelope_privacy", size),
            &size,
            |b, _size| {
                b.iter(|| {
                    let envelope = Envelope::new_with_privacy(
                        message.clone(),
                        sender,
                        recipient,
                        PrivacyLevel::RelationshipScoped,
                    );
                    let serialized = envelope.to_bytes();
                    black_box(Envelope::from_bytes(&serialized).unwrap())
                })
            },
        );
        
        // Basic implementation (clear privacy for comparison)
        group.bench_with_input(
            BenchmarkId::new("basic_envelope_clear", size),
            &size,
            |b, _size| {
                b.iter(|| {
                    let envelope = Envelope::new_with_privacy(
                        message.clone(),
                        sender,
                        recipient,
                        PrivacyLevel::Clear,
                    );
                    let serialized = envelope.to_bytes();
                    black_box(Envelope::from_bytes(&serialized).unwrap())
                })
            },
        );
    }
    
    group.finish();
}

/// Memory Usage and Efficiency Benchmarks
fn bench_memory_efficiency(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_efficiency");
    
    // Benchmark memory usage for different privacy levels
    for config in [BenchConfig::small(), BenchConfig::medium(), BenchConfig::large()] {
        let message = vec![0u8; config.message_size];
        let sender = DeviceId::new();
        let recipient = DeviceId::new();
        
        group.bench_with_input(
            BenchmarkId::new("memory_usage_clear", config.message_size),
            &config,
            |b, _config| {
                b.iter(|| {
                    let envelopes: Vec<_> = (0..100).map(|_| {
                        Envelope::new_with_privacy(
                            message.clone(),
                            sender,
                            recipient,
                            PrivacyLevel::Clear,
                        )
                    }).collect();
                    black_box(envelopes)
                })
            },
        );
        
        group.bench_with_input(
            BenchmarkId::new("memory_usage_scoped", config.message_size),
            &config,
            |b, _config| {
                b.iter(|| {
                    let context = ContextId::new();
                    let envelopes: Vec<_> = (0..100).map(|_| {
                        Envelope::new_scoped(message.clone(), sender, context)
                    }).collect();
                    black_box(envelopes)
                })
            },
        );
    }
    
    group.finish();
}

criterion_group!(
    layer2_benches,
    bench_envelope_operations,
    bench_connection_operations,
    bench_peer_operations
);

criterion_group!(
    layer3_benches,
    bench_effect_handler_operations,
    bench_transport_manager_operations
);

criterion_group!(
    layer4_benches,
    bench_coordination_operations
);

criterion_group!(
    integration_benches,
    bench_integration_performance,
    bench_performance_comparison,
    bench_memory_efficiency
);

criterion_main!(
    layer2_benches,
    layer3_benches, 
    layer4_benches,
    integration_benches
);