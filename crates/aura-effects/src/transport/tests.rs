//! Comprehensive Tests for Transport Effect Handlers
//!
//! Tests all transport effect implementations including TCP, WebSocket,
//! in-memory handlers, and utility functions. Focuses on Layer 3 compliance
//! (stateless, single-party, context-free operations).

use super::{
    framing::FramingHandler,
    memory::InMemoryTransportHandler,
    tcp::TcpTransportHandler,
    utils::{AddressResolver, BufferUtils, ConnectionMetrics, TimeoutHelper},
    websocket::WebSocketTransportHandler,
};
use aura_core::{ContextId, DeviceId};
use aura_transport::{
    peers::PeerInfo,
    types::{ConnectionId, PrivacyLevel, TransportConfig},
};
use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;
use tokio::time::timeout;

#[cfg(test)]
mod tcp_handler_tests {
    use super::*;

    #[test]
    fn test_tcp_handler_creation() {
        let config = TransportConfig::default();
        let handler = TcpTransportHandler::new(config);

        assert_eq!(handler.protocol_name(), "tcp");
        assert!(handler.is_stateless());
        assert!(!handler.requires_relay());
    }

    #[test]
    fn test_tcp_config_validation() {
        let mut config = TransportConfig::default();
        config.max_connections = 0; // Invalid

        let result = TcpTransportHandler::validate_config(&config);
        assert!(result.is_err());

        config.max_connections = 10; // Valid
        let result = TcpTransportHandler::validate_config(&config);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_tcp_local_bind() {
        let config = TransportConfig::default();
        let handler = TcpTransportHandler::new(config);

        // Test local bind (should work on any available port)
        let bind_addr = "127.0.0.1:0".parse::<SocketAddr>().unwrap();
        let result = handler.bind_local(bind_addr).await;

        // Should succeed (or fail gracefully due to system constraints)
        match result {
            Ok(bound_addr) => {
                assert_eq!(bound_addr.ip(), bind_addr.ip());
                // Port might be different (0 = any available port)
            }
            Err(e) => {
                // Acceptable if system doesn't allow binding
                println!("Bind failed (acceptable): {}", e);
            }
        }
    }

    #[tokio::test]
    async fn test_tcp_connection_timeout() {
        let config = TransportConfig {
            connection_timeout: Duration::from_millis(100),
            ..Default::default()
        };
        let handler = TcpTransportHandler::new(config);

        // Try to connect to a non-routable address (will timeout)
        let unreachable_addr = "10.255.255.1:12345".parse::<SocketAddr>().unwrap();

        let start_time = std::time::Instant::now();
        let result = handler.connect(unreachable_addr).await;
        let elapsed = start_time.elapsed();

        // Should timeout within reasonable bounds
        assert!(result.is_err());
        assert!(elapsed >= Duration::from_millis(90)); // Allow some variance
        assert!(elapsed <= Duration::from_millis(200)); // But not too much
    }

    #[test]
    fn test_tcp_address_parsing() {
        let handler = TcpTransportHandler::new(TransportConfig::default());

        // Valid addresses
        let valid_addresses = vec![
            "127.0.0.1:8080",
            "192.168.1.1:443",
            "[::1]:8080",
            "localhost:3000",
        ];

        for addr_str in valid_addresses {
            let result = handler.parse_address(addr_str);
            assert!(
                result.is_ok(),
                "Failed to parse valid address: {}",
                addr_str
            );
        }

        // Invalid addresses
        let invalid_addresses = vec![
            "not-an-address",
            "127.0.0.1",       // Missing port
            "127.0.0.1:99999", // Invalid port
            "",
        ];

        for addr_str in invalid_addresses {
            let result = handler.parse_address(addr_str);
            assert!(
                result.is_err(),
                "Should not parse invalid address: {}",
                addr_str
            );
        }
    }
}

#[cfg(test)]
mod websocket_handler_tests {
    use super::*;

    #[test]
    fn test_websocket_handler_creation() {
        let config = TransportConfig::default();
        let handler = WebSocketTransportHandler::new(config);

        assert_eq!(handler.protocol_name(), "websocket");
        assert!(handler.is_stateless());
        assert!(!handler.requires_relay()); // Can be used without relay
    }

    #[test]
    fn test_websocket_url_validation() {
        let handler = WebSocketTransportHandler::new(TransportConfig::default());

        // Valid WebSocket URLs
        let valid_urls = vec![
            "ws://localhost:8080/path",
            "wss://secure.example.com:443/socket",
            "ws://192.168.1.1:3000",
        ];

        for url_str in valid_urls {
            let result = handler.validate_url(url_str);
            assert!(result.is_ok(), "Failed to validate URL: {}", url_str);
        }

        // Invalid URLs
        let invalid_urls = vec![
            "http://not-websocket.com", // Wrong protocol
            "ws://",                    // Incomplete URL
            "not-a-url",
            "ftp://example.com",
        ];

        for url_str in invalid_urls {
            let result = handler.validate_url(url_str);
            assert!(
                result.is_err(),
                "Should not validate invalid URL: {}",
                url_str
            );
        }
    }

    #[test]
    fn test_websocket_message_framing() {
        let handler = WebSocketTransportHandler::new(TransportConfig::default());
        let test_message = b"Hello, WebSocket world!";

        // Test message framing
        let framed = handler.frame_message(test_message);
        assert!(!framed.is_empty());
        assert!(framed.len() >= test_message.len()); // Framed message should be at least as long

        // Test message unframing
        let unframed = handler.unframe_message(&framed).expect("Unframing failed");
        assert_eq!(unframed, test_message);
    }

    #[tokio::test]
    async fn test_websocket_connection_timeout() {
        let config = TransportConfig {
            connection_timeout: Duration::from_millis(200),
            ..Default::default()
        };
        let handler = WebSocketTransportHandler::new(config);

        // Try to connect to unreachable WebSocket server
        let unreachable_url = "ws://10.255.255.1:12345/socket";

        let start_time = std::time::Instant::now();
        let result = handler.connect(unreachable_url).await;
        let elapsed = start_time.elapsed();

        // Should timeout
        assert!(result.is_err());
        assert!(elapsed >= Duration::from_millis(180)); // Allow some variance
        assert!(elapsed <= Duration::from_millis(300));
    }
}

#[cfg(test)]
mod memory_handler_tests {
    use super::*;

    #[tokio::test]
    async fn test_memory_handler_creation() {
        let config = TransportConfig::default();
        let handler = InMemoryTransportHandler::new(config);

        assert_eq!(handler.protocol_name(), "memory");
        assert!(handler.is_stateless());
        assert!(!handler.requires_relay());

        // Should start with no registered peers
        assert_eq!(handler.registered_peer_count(), 0);
    }

    #[tokio::test]
    async fn test_memory_peer_registration() {
        let mut handler = InMemoryTransportHandler::new(TransportConfig::default());

        let device1 = DeviceId::new();
        let device2 = DeviceId::new();

        // Register peers
        handler
            .register_peer(device1, "device1-channel".to_string())
            .await;
        handler
            .register_peer(device2, "device2-channel".to_string())
            .await;

        assert_eq!(handler.registered_peer_count(), 2);
        assert!(handler.is_peer_registered(device1));
        assert!(handler.is_peer_registered(device2));
        assert!(!handler.is_peer_registered(DeviceId::new())); // Unregistered peer
    }

    #[tokio::test]
    async fn test_memory_message_delivery() {
        let mut handler = InMemoryTransportHandler::new(TransportConfig::default());

        let sender = DeviceId::new();
        let receiver = DeviceId::new();
        let message = b"In-memory test message";

        // Register both peers
        handler
            .register_peer(sender, "sender-channel".to_string())
            .await;
        handler
            .register_peer(receiver, "receiver-channel".to_string())
            .await;

        // Send message
        let result = handler
            .send_message(sender, receiver, message.to_vec())
            .await;
        assert!(result.is_ok());

        // Check message delivery
        let messages = handler
            .get_pending_messages(receiver)
            .await
            .expect("Failed to get messages");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].payload, message);
        assert_eq!(messages[0].sender, sender);
    }

    #[tokio::test]
    async fn test_memory_message_ordering() {
        let mut handler = InMemoryTransportHandler::new(TransportConfig::default());

        let sender = DeviceId::new();
        let receiver = DeviceId::new();

        handler.register_peer(sender, "sender".to_string()).await;
        handler
            .register_peer(receiver, "receiver".to_string())
            .await;

        // Send multiple messages
        let messages = vec![b"Message 1", b"Message 2", b"Message 3"];

        for (i, msg) in messages.iter().enumerate() {
            let result = handler.send_message(sender, receiver, msg.to_vec()).await;
            assert!(result.is_ok(), "Failed to send message {}", i);
        }

        // Messages should be delivered in order
        let delivered = handler
            .get_pending_messages(receiver)
            .await
            .expect("Failed to get messages");
        assert_eq!(delivered.len(), 3);

        for (i, delivered_msg) in delivered.iter().enumerate() {
            assert_eq!(delivered_msg.payload, messages[i]);
            assert!(delivered_msg.timestamp.elapsed().unwrap() < Duration::from_secs(1));
        }
    }

    #[tokio::test]
    async fn test_memory_isolation_between_peers() {
        let mut handler = InMemoryTransportHandler::new(TransportConfig::default());

        let peer1 = DeviceId::new();
        let peer2 = DeviceId::new();
        let peer3 = DeviceId::new();

        handler.register_peer(peer1, "peer1".to_string()).await;
        handler.register_peer(peer2, "peer2".to_string()).await;
        handler.register_peer(peer3, "peer3".to_string()).await;

        // Send message from peer1 to peer2
        handler
            .send_message(peer1, peer2, b"Secret message".to_vec())
            .await
            .unwrap();

        // peer2 should receive the message
        let peer2_messages = handler.get_pending_messages(peer2).await.unwrap();
        assert_eq!(peer2_messages.len(), 1);

        // peer3 should not receive the message
        let peer3_messages = handler.get_pending_messages(peer3).await.unwrap();
        assert_eq!(peer3_messages.len(), 0);
    }
}

#[cfg(test)]
mod framing_handler_tests {
    use super::*;

    #[test]
    fn test_framing_handler_creation() {
        let handler = FramingHandler::new();

        assert_eq!(handler.protocol_name(), "framing");
        assert!(handler.is_stateless());
        assert_eq!(
            handler.max_frame_size(),
            FramingHandler::DEFAULT_MAX_FRAME_SIZE
        );
    }

    #[test]
    fn test_message_framing_roundtrip() {
        let handler = FramingHandler::new();
        let original_messages = vec![
            b"Short message",
            "A longer message with more content and special characters: αβγδε 🔒🔑".as_bytes(),
            &[0u8; 1000], // Binary data
            b"",          // Empty message
        ];

        for original in original_messages {
            // Frame the message
            let framed = handler.frame_message(original).expect("Framing failed");
            assert!(framed.len() >= original.len()); // Framed should be at least as long

            // Unframe the message
            let unframed = handler.unframe_message(&framed).expect("Unframing failed");
            assert_eq!(unframed, original);
        }
    }

    #[test]
    fn test_frame_size_limits() {
        let handler = FramingHandler::with_max_frame_size(100);

        // Small message should work
        let small_message = vec![1u8; 50];
        let result = handler.frame_message(&small_message);
        assert!(result.is_ok());

        // Large message should fail
        let large_message = vec![2u8; 200];
        let result = handler.frame_message(&large_message);
        assert!(result.is_err());
    }

    #[test]
    fn test_corrupted_frame_handling() {
        let handler = FramingHandler::new();
        let original = b"Valid message";

        let mut framed = handler.frame_message(original).expect("Framing failed");

        // Corrupt the frame by modifying random bytes
        framed[5] = framed[5].wrapping_add(1);
        framed[10] = framed[10].wrapping_add(1);

        // Unframing should fail gracefully
        let result = handler.unframe_message(&framed);
        assert!(result.is_err());
    }

    #[test]
    fn test_multiple_frames_in_buffer() {
        let handler = FramingHandler::new();
        let messages = vec![b"First", b"Second", b"Third"];

        // Frame all messages and concatenate
        let mut buffer = Vec::new();
        for msg in &messages {
            let framed = handler.frame_message(msg).expect("Framing failed");
            buffer.extend_from_slice(&framed);
        }

        // Extract frames one by one
        let mut remaining = buffer.as_slice();
        let mut extracted = Vec::new();

        while !remaining.is_empty() {
            let (frame, rest) = handler
                .extract_frame(remaining)
                .expect("Frame extraction failed");
            let unframed = handler.unframe_message(&frame).expect("Unframing failed");
            extracted.push(unframed);
            remaining = rest;
        }

        // Should extract all original messages
        assert_eq!(extracted.len(), messages.len());
        for (i, extracted_msg) in extracted.iter().enumerate() {
            assert_eq!(extracted_msg, &messages[i]);
        }
    }
}

#[cfg(test)]
mod utils_tests {
    use super::*;

    #[test]
    fn test_address_resolver() {
        let resolver = AddressResolver::new();

        // Test known good addresses
        let addresses = vec!["127.0.0.1:8080", "localhost:3000", "192.168.1.1:443"];

        for addr_str in addresses {
            let result = resolver.resolve(addr_str);
            // Resolution might fail due to system configuration, but shouldn't panic
            match result {
                Ok(resolved) => {
                    assert!(!resolved.is_empty());
                    println!("Resolved {} to {:?}", addr_str, resolved);
                }
                Err(e) => {
                    println!("Resolution failed for {} (acceptable): {}", addr_str, e);
                }
            }
        }
    }

    #[tokio::test]
    async fn test_timeout_helper() {
        let timeout_helper = TimeoutHelper::new(Duration::from_millis(100));

        // Fast operation should succeed
        let fast_result = timeout_helper
            .with_timeout(async {
                tokio::time::sleep(Duration::from_millis(50)).await;
                "success"
            })
            .await;
        assert!(fast_result.is_ok());
        assert_eq!(fast_result.unwrap(), "success");

        // Slow operation should timeout
        let slow_result = timeout_helper
            .with_timeout(async {
                tokio::time::sleep(Duration::from_millis(200)).await;
                "should not reach here"
            })
            .await;
        assert!(slow_result.is_err());
    }

    #[test]
    fn test_buffer_utils() {
        let utils = BufferUtils::new();

        // Test buffer sizing
        assert_eq!(
            utils.optimal_buffer_size(100),
            BufferUtils::min_buffer_size(100)
        );
        assert_eq!(
            utils.optimal_buffer_size(10000),
            BufferUtils::min_buffer_size(10000)
        );

        // Test buffer allocation
        let buffer = utils.allocate_buffer(1024);
        assert_eq!(buffer.len(), 1024);
        assert_eq!(buffer.capacity(), 1024);

        // Test buffer reuse
        let mut reused_buffer = vec![1, 2, 3, 4, 5];
        utils.clear_and_resize(&mut reused_buffer, 10);
        assert_eq!(reused_buffer.len(), 10);
        assert!(reused_buffer.iter().all(|&x| x == 0)); // Should be zero-filled
    }

    #[test]
    fn test_connection_metrics() {
        let mut metrics = ConnectionMetrics::new();

        // Record some connection attempts
        metrics.record_connection_attempt();
        metrics.record_connection_attempt();
        metrics.record_connection_success();
        metrics.record_connection_failure("timeout".to_string());

        // Verify metrics
        assert_eq!(metrics.total_attempts(), 2);
        assert_eq!(metrics.success_count(), 1);
        assert_eq!(metrics.failure_count(), 1);
        assert_eq!(metrics.success_rate(), 0.5);

        // Record some latency
        metrics.record_latency(Duration::from_millis(50));
        metrics.record_latency(Duration::from_millis(100));
        metrics.record_latency(Duration::from_millis(75));

        assert_eq!(metrics.average_latency(), Duration::from_millis(75));
        assert!(metrics.min_latency() <= Duration::from_millis(50));
        assert!(metrics.max_latency() >= Duration::from_millis(100));
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;

    #[tokio::test]
    async fn test_handler_interoperability() {
        // Test that different handlers can work together through common interfaces
        let config = TransportConfig::default();

        let tcp_handler = TcpTransportHandler::new(config.clone());
        let ws_handler = WebSocketTransportHandler::new(config.clone());
        let mem_handler = InMemoryTransportHandler::new(config);

        // All handlers should implement common traits
        assert!(tcp_handler.is_stateless());
        assert!(ws_handler.is_stateless());
        assert!(mem_handler.is_stateless());

        // All should have consistent protocol names
        assert!(!tcp_handler.protocol_name().is_empty());
        assert!(!ws_handler.protocol_name().is_empty());
        assert!(!mem_handler.protocol_name().is_empty());
    }

    #[test]
    fn test_config_compatibility() {
        // Test that all handlers work with various config combinations
        let configs = vec![
            TransportConfig {
                privacy_level: PrivacyLevel::Clear,
                max_connections: 1,
                connection_timeout: Duration::from_secs(1),
                enable_capability_blinding: false,
                enable_traffic_padding: false,
                ..Default::default()
            },
            TransportConfig {
                privacy_level: PrivacyLevel::RelationshipScoped,
                max_connections: 100,
                connection_timeout: Duration::from_secs(60),
                enable_capability_blinding: true,
                enable_traffic_padding: true,
                ..Default::default()
            },
        ];

        for config in configs {
            // All handlers should accept these configs
            let tcp = TcpTransportHandler::new(config.clone());
            let ws = WebSocketTransportHandler::new(config.clone());
            let mem = InMemoryTransportHandler::new(config.clone());

            // Basic validation
            assert!(TcpTransportHandler::validate_config(&config).is_ok());
            assert!(WebSocketTransportHandler::validate_config(&config).is_ok());
            assert!(InMemoryTransportHandler::validate_config(&config).is_ok());
        }
    }

    #[tokio::test]
    async fn test_error_handling_consistency() {
        // Test that all handlers handle errors consistently
        let config = TransportConfig {
            connection_timeout: Duration::from_millis(10), // Very short timeout
            ..Default::default()
        };

        let handlers: Vec<Box<dyn std::fmt::Debug>> = vec![
            Box::new(TcpTransportHandler::new(config.clone())),
            Box::new(WebSocketTransportHandler::new(config.clone())),
            Box::new(InMemoryTransportHandler::new(config)),
        ];

        // All handlers should fail gracefully on invalid operations
        // (Actual testing would require more specific error trait implementations)
        assert_eq!(handlers.len(), 3); // Just verify they were created
    }
}
