use super::TransportError;
use std::net::SocketAddr;

const AURA_TCP_LISTEN_ADDR: &str = "AURA_TCP_LISTEN_ADDR";
const DEFAULT_TCP_LISTEN_ADDR: &str = "127.0.0.1:0";

pub(super) fn tcp_listen_addr() -> Result<SocketAddr, TransportError> {
    std::env::var(AURA_TCP_LISTEN_ADDR)
        .unwrap_or_else(|_| DEFAULT_TCP_LISTEN_ADDR.to_string())
        .parse()
        .map_err(|error| {
            TransportError::ConnectionFailed(format!("Invalid listen address: {error}"))
        })
}
