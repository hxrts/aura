//! UDP effect handlers (Layer 3)
//!
//! Stateless UDP socket implementation backing `UdpEffects` from aura-core.

use async_trait::async_trait;
use aura_core::effects::network::{NetworkError, UdpEffects, UdpSocketEffects};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;

/// Production UDP effects handler (stateless).
#[derive(Debug, Clone, Default)]
pub struct RealUdpEffectsHandler;

impl RealUdpEffectsHandler {
    /// Create a new UDP effects handler.
    pub fn new() -> Self {
        Self
    }
}

/// UDP socket wrapper implementing the effect surface.
#[derive(Debug)]
struct RealUdpSocket {
    socket: UdpSocket,
}

#[async_trait]
impl UdpSocketEffects for RealUdpSocket {
    async fn set_broadcast(&self, enabled: bool) -> Result<(), NetworkError> {
        self.socket
            .set_broadcast(enabled)
            .map_err(|e| NetworkError::ConnectionFailed(format!("set_broadcast failed: {e}")))?;
        Ok(())
    }

    async fn send_to(&self, payload: &[u8], addr: SocketAddr) -> Result<usize, NetworkError> {
        self.socket
            .send_to(payload, addr)
            .await
            .map_err(|e| NetworkError::SendFailed {
                peer_id: None,
                reason: e.to_string(),
            })
    }

    async fn recv_from(&self, buffer: &mut [u8]) -> Result<(usize, SocketAddr), NetworkError> {
        self.socket
            .recv_from(buffer)
            .await
            .map_err(|e| NetworkError::ReceiveFailed {
                reason: e.to_string(),
            })
    }
}

#[async_trait]
impl UdpEffects for RealUdpEffectsHandler {
    async fn udp_bind(&self, addr: SocketAddr) -> Result<Arc<dyn UdpSocketEffects>, NetworkError> {
        let socket = UdpSocket::bind(addr).await.map_err(|e| {
            NetworkError::ConnectionFailed(format!("UDP bind failed ({addr}): {e}"))
        })?;
        Ok(Arc::new(RealUdpSocket { socket }))
    }
}
