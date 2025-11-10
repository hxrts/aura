use aura_core::{AuraError, DeviceId};
use aura_transport::{WebSocketConfig, WebSocketConnection, WebSocketTransport};
use std::time::Duration;
use tokio::time::timeout;

/// Integration test for WebSocket transport
#[tokio::test]
async fn test_websocket_transport_basic() {
    let device1 = DeviceId("device1".to_string());
    let device2 = DeviceId("device2".to_string());

    let config1 = WebSocketConfig {
        port: 8082, // Use different port to avoid conflicts
        ..WebSocketConfig::default()
    };
    let config2 = WebSocketConfig {
        port: 8083,
        ..WebSocketConfig::default()
    };

    let transport1 = WebSocketTransport::new(device1.clone(), config1);
    let transport2 = WebSocketTransport::new(device2.clone(), config2);

    // Start server on transport1
    let mut server1 = transport1
        .start_server()
        .await
        .expect("Failed to start server");

    // Connect transport2 to transport1 as client
    let url = "ws://127.0.0.1:8082";
    let addr = "127.0.0.1:8082".parse().expect("Invalid address");

    // Add small delay to ensure server is ready
    tokio::time::sleep(Duration::from_millis(100)).await;

    let mut client_conn = transport2
        .connect_client(url, addr)
        .await
        .expect("Failed to connect client");

    // Accept the connection on server side
    let mut server_conn = timeout(Duration::from_secs(5), server1.accept())
        .await
        .expect("Timeout waiting for connection")
        .expect("Failed to accept connection");

    // Test sending message from client to server
    let test_message = b"Hello from client";
    client_conn
        .send(device1.clone(), test_message)
        .await
        .expect("Failed to send message");

    // Receive message on server side
    let received = timeout(Duration::from_secs(5), server_conn.receive())
        .await
        .expect("Timeout waiting for message")
        .expect("Failed to receive message");

    assert_eq!(received.payload, test_message);
    assert_eq!(received.from, device2);
    assert_eq!(received.to, device1);

    // Test sending message from server to client
    let response_message = b"Hello from server";
    server_conn
        .send(device2.clone(), response_message)
        .await
        .expect("Failed to send response");

    // Receive response on client side
    let response = timeout(Duration::from_secs(5), client_conn.receive())
        .await
        .expect("Timeout waiting for response")
        .expect("Failed to receive response");

    assert_eq!(response.payload, response_message);
    assert_eq!(response.from, device1);
    assert_eq!(response.to, device2);

    // Clean up connections
    let _ = client_conn.close().await;
    let _ = server_conn.close().await;
}

#[tokio::test]
async fn test_websocket_message_size_limit() {
    let device_id = DeviceId("test_device".to_string());
    let config = WebSocketConfig {
        port: 8084,
        max_message_size: 100, // Small limit for testing
        ..WebSocketConfig::default()
    };

    let transport = WebSocketTransport::new(device_id.clone(), config);
    let mut server = transport
        .start_server()
        .await
        .expect("Failed to start server");

    let url = "ws://127.0.0.1:8084";
    let addr = "127.0.0.1:8084".parse().expect("Invalid address");

    tokio::time::sleep(Duration::from_millis(100)).await;

    let mut client_conn = transport
        .connect_client(url, addr)
        .await
        .expect("Failed to connect client");
    let mut server_conn = timeout(Duration::from_secs(5), server.accept())
        .await
        .expect("Timeout waiting for connection")
        .expect("Failed to accept connection");

    // Try to send a message that exceeds the size limit
    let large_message = vec![0u8; 200]; // Larger than the 100 byte limit
    let result = client_conn.send(device_id.clone(), &large_message).await;

    assert!(result.is_err(), "Should fail to send oversized message");

    // Send a message within the size limit
    let small_message = vec![0u8; 50];
    client_conn
        .send(device_id.clone(), &small_message)
        .await
        .expect("Should succeed with small message");

    let received = timeout(Duration::from_secs(5), server_conn.receive())
        .await
        .expect("Timeout waiting for message")
        .expect("Failed to receive message");

    assert_eq!(received.payload.len(), 50);

    // Clean up
    let _ = client_conn.close().await;
    let _ = server_conn.close().await;
}

#[tokio::test]
async fn test_websocket_connection_lifecycle() {
    let device_id = DeviceId("lifecycle_test".to_string());
    let config = WebSocketConfig {
        port: 8085,
        ..WebSocketConfig::default()
    };

    let transport = WebSocketTransport::new(device_id.clone(), config);

    // Test that transport is created successfully
    assert_eq!(transport.device_id(), device_id);
    assert_eq!(transport.config().port, 8085);

    // Test server startup
    let server = transport
        .start_server()
        .await
        .expect("Failed to start server");
    drop(server); // Server should handle being dropped gracefully

    // Test client connection failure to non-existent server
    let url = "ws://127.0.0.1:9999"; // Non-existent port
    let addr = "127.0.0.1:9999".parse().expect("Invalid address");

    let result = transport.connect_client(url, addr).await;
    assert!(
        result.is_err(),
        "Should fail to connect to non-existent server"
    );
}

#[tokio::test]
async fn test_websocket_envelope_format() {
    use aura_transport::WebSocketEnvelope;
    use serde_cbor;

    let device1 = DeviceId("sender".to_string());
    let device2 = DeviceId("receiver".to_string());

    let envelope = WebSocketEnvelope {
        from: device1.clone(),
        to: device2.clone(),
        payload: b"test payload".to_vec(),
        sequence: 42,
        timestamp: 1234567890,
    };

    // Test CBOR serialization/deserialization
    let serialized = serde_cbor::to_vec(&envelope).expect("Failed to serialize envelope");
    let deserialized: WebSocketEnvelope =
        serde_cbor::from_slice(&serialized).expect("Failed to deserialize envelope");

    assert_eq!(envelope.from, deserialized.from);
    assert_eq!(envelope.to, deserialized.to);
    assert_eq!(envelope.payload, deserialized.payload);
    assert_eq!(envelope.sequence, deserialized.sequence);
    assert_eq!(envelope.timestamp, deserialized.timestamp);

    // Test that serialized size is reasonable (for fixed-size envelope padding)
    assert!(
        serialized.len() < 1000,
        "Envelope should be relatively small"
    );
}

#[tokio::test]
async fn test_websocket_concurrent_connections() {
    let server_device = DeviceId("server".to_string());
    let config = WebSocketConfig {
        port: 8086,
        ..WebSocketConfig::default()
    };

    let transport = WebSocketTransport::new(server_device.clone(), config);
    let mut server = transport
        .start_server()
        .await
        .expect("Failed to start server");

    tokio::time::sleep(Duration::from_millis(100)).await;

    // Create multiple client connections concurrently
    let url = "ws://127.0.0.1:8086";
    let addr = "127.0.0.1:8086".parse().expect("Invalid address");

    let client_tasks = (0..3)
        .map(|i| {
            let device_id = DeviceId(format!("client_{}", i));
            let transport = WebSocketTransport::new(device_id.clone(), WebSocketConfig::default());
            let url = url.to_string();

            tokio::spawn(async move {
                let mut conn = transport.connect_client(&url, addr).await?;
                let message = format!("Hello from client {}", i).into_bytes();
                conn.send(DeviceId("server".to_string()), &message).await?;
                conn.close().await?;
                Ok::<(), AuraError>(())
            })
        })
        .collect::<Vec<_>>();

    // Accept connections and receive messages
    for _ in 0..3 {
        let mut conn = timeout(Duration::from_secs(5), server.accept())
            .await
            .expect("Timeout waiting for connection")
            .expect("Failed to accept connection");

        let envelope = timeout(Duration::from_secs(5), conn.receive())
            .await
            .expect("Timeout waiting for message")
            .expect("Failed to receive message");

        let message_text = String::from_utf8(envelope.payload).expect("Invalid UTF-8");
        assert!(message_text.starts_with("Hello from client"));

        let _ = conn.close().await;
    }

    // Wait for all client tasks to complete
    for task in client_tasks {
        task.await
            .expect("Client task panicked")
            .expect("Client task failed");
    }
}
