use serde::{Deserialize, Serialize};

/// Opaque endpoint address representation used in Layer 2 specs.
///
/// Addresses remain strings here to avoid std::net coupling in the
/// specification layer. Production layers parse/validate as needed via
/// NetworkEffects implementations in aura-effects (Layer 3).
///
/// # Architecture Note
///
/// This type intentionally does NOT depend on `std::net` types. Address
/// parsing and validation should happen in effect implementations, not
/// in the specification layer. Use `EndpointAddress::as_str()` to extract
/// the raw address string for effect-layer processing.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EndpointAddress(pub String);

impl EndpointAddress {
    /// Create a new endpoint address from any string-like type.
    pub fn new<S: Into<String>>(addr: S) -> Self {
        Self(addr.into())
    }

    /// Get the address as a string slice.
    ///
    /// Use this to pass the address to NetworkEffects implementations
    /// which handle the actual parsing and validation.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Extract the port portion of the address string.
    ///
    /// Performs simple string parsing without std::net dependency.
    /// Returns 0 if no valid port can be extracted.
    pub fn port(&self) -> u16 {
        self.extract_port().unwrap_or(0)
    }

    /// Extract the host portion of the address string.
    ///
    /// Returns the IP/hostname part without the port.
    /// Returns empty string if parsing fails.
    pub fn host(&self) -> &str {
        self.extract_host().unwrap_or("")
    }

    /// Check if this appears to be an IPv4 address (contains dots, no colons except for port).
    pub fn is_ipv4(&self) -> bool {
        let host = self.host();
        host.contains('.') && !host.contains(':')
    }

    /// Check if this appears to be an IPv6 address (contains colons or brackets).
    pub fn is_ipv6(&self) -> bool {
        let host = self.host();
        host.starts_with('[') || (host.contains(':') && !host.contains('.'))
    }

    /// Create from host string and port.
    pub fn from_host_port(host: &str, port: u16) -> Self {
        // If host looks like IPv6, wrap in brackets
        if host.contains(':') && !host.starts_with('[') {
            Self(format!("[{host}]:{port}"))
        } else {
            Self(format!("{host}:{port}"))
        }
    }

    /// Extract port from "host:port" or "[ipv6]:port" format.
    fn extract_port(&self) -> Option<u16> {
        // Handle "[ipv6]:port" format
        if let Some(bracket_end) = self.0.rfind(']') {
            if self.0.len() > bracket_end + 2 && self.0.as_bytes()[bracket_end + 1] == b':' {
                return self.0[bracket_end + 2..].parse().ok();
            }
        }

        // Handle "host:port" format (take last colon)
        if let Some(colon_pos) = self.0.rfind(':') {
            // Make sure this isn't part of an IPv6 address without brackets
            let before_colon = &self.0[..colon_pos];
            if !before_colon.contains(':') {
                return self.0[colon_pos + 1..].parse().ok();
            }
        }

        None
    }

    /// Extract host from "host:port" or "[ipv6]:port" format.
    fn extract_host(&self) -> Option<&str> {
        // Handle "[ipv6]:port" format
        if self.0.starts_with('[') {
            if let Some(bracket_end) = self.0.find(']') {
                return Some(&self.0[1..bracket_end]);
            }
        }

        // Handle "host:port" format
        if let Some(colon_pos) = self.0.rfind(':') {
            let before_colon = &self.0[..colon_pos];
            // Make sure this isn't part of an IPv6 address
            if !before_colon.contains(':') {
                return Some(before_colon);
            }
        }

        // No port separator found, return entire string as host
        Some(&self.0)
    }
}

impl From<&str> for EndpointAddress {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for EndpointAddress {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl std::fmt::Display for EndpointAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
