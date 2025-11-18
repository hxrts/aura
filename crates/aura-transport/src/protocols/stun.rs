//! STUN Protocol Message Types
//!
//! Essential STUN protocol messages for NAT traversal and connectivity.
//! Target: <150 lines (focused implementation).

use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use uuid::Uuid;

/// Essential STUN protocol messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StunMessage {
    /// Message type (method + class)
    pub message_type: StunMessageType,

    /// Transaction ID for request/response matching
    pub transaction_id: Uuid,

    /// STUN attributes
    pub attributes: Vec<StunAttribute>,
}

/// STUN message type combining method and class
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StunMessageType {
    /// STUN method
    pub method: StunMethod,

    /// STUN class
    pub class: StunClass,
}

/// Core STUN method types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StunMethod {
    /// Binding method for connectivity checks
    Binding,
    /// Allocate method for relay allocation
    Allocate,
    /// Refresh method for allocation refresh
    Refresh,
    /// Send method for relayed data
    Send,
    /// Data method for relayed data indication
    Data,
    /// CreatePermission for relay permissions
    CreatePermission,
}

/// Core STUN class types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StunClass {
    /// Request message
    Request,
    /// Success response
    SuccessResponse,
    /// Error response
    ErrorResponse,
    /// Indication (no response expected)
    Indication,
}

/// Essential STUN attribute definitions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StunAttribute {
    /// Mapped address from server perspective
    MappedAddress(SocketAddr),

    /// Username for authentication
    Username(String),

    /// Message integrity for authentication
    MessageIntegrity(Vec<u8>),

    /// Error code for error responses
    ErrorCode {
        /// Numeric error code
        code: u16,
        /// Human-readable error description
        reason: String,
    },

    /// Unknown comprehension required attribute
    UnknownAttributes(Vec<u16>),

    /// Realm for authentication
    Realm(String),

    /// Nonce for authentication
    Nonce(String),

    /// XOR mapped address (RFC 5389)
    XorMappedAddress(SocketAddr),

    /// Software identification
    Software(String),

    /// Alternate server
    AlternateServer(SocketAddr),

    /// Unknown attribute
    Unknown {
        /// Attribute type identifier
        attribute_type: u16,
        /// Raw attribute data
        data: Vec<u8>,
    },
}

impl StunMessage {
    /// Create new STUN binding request
    pub fn binding_request() -> Self {
        Self::binding_request_with_id(Self::generate_transaction_id())
    }

    /// Create STUN binding request with specific transaction ID
    pub fn binding_request_with_id(transaction_id: Uuid) -> Self {
        Self {
            message_type: StunMessageType {
                method: StunMethod::Binding,
                class: StunClass::Request,
            },
            transaction_id,
            attributes: Vec::new(),
        }
    }

    /// Generate deterministic transaction ID
    fn generate_transaction_id() -> Uuid {
        // Use deterministic approach for transaction IDs
        // In production this would use a proper deterministic algorithm
        Uuid::nil() // Placeholder
    }

    /// Create STUN binding success response
    pub fn binding_response(transaction_id: Uuid, mapped_addr: SocketAddr) -> Self {
        Self {
            message_type: StunMessageType {
                method: StunMethod::Binding,
                class: StunClass::SuccessResponse,
            },
            transaction_id,
            attributes: vec![StunAttribute::XorMappedAddress(mapped_addr)],
        }
    }

    /// Create STUN error response
    pub fn error_response(transaction_id: Uuid, error_code: u16, reason: String) -> Self {
        Self {
            message_type: StunMessageType {
                method: StunMethod::Binding,
                class: StunClass::ErrorResponse,
            },
            transaction_id,
            attributes: vec![StunAttribute::ErrorCode {
                code: error_code,
                reason,
            }],
        }
    }

    /// Add attribute to message
    pub fn add_attribute(&mut self, attribute: StunAttribute) {
        self.attributes.push(attribute);
    }

    /// Find attribute by type
    pub fn find_attribute<T>(&self, predicate: impl Fn(&StunAttribute) -> Option<T>) -> Option<T> {
        for attr in &self.attributes {
            if let Some(result) = predicate(attr) {
                return Some(result);
            }
        }
        None
    }

    /// Get mapped address from response
    pub fn mapped_address(&self) -> Option<SocketAddr> {
        self.find_attribute(|attr| match attr {
            StunAttribute::MappedAddress(addr) => Some(*addr),
            StunAttribute::XorMappedAddress(addr) => Some(*addr),
            _ => None,
        })
    }

    /// Get error code from error response
    pub fn error_code(&self) -> Option<(u16, String)> {
        self.find_attribute(|attr| match attr {
            StunAttribute::ErrorCode { code, reason } => Some((*code, reason.clone())),
            _ => None,
        })
    }

    /// Check if this is a request message
    pub fn is_request(&self) -> bool {
        matches!(self.message_type.class, StunClass::Request)
    }

    /// Check if this is a response message
    pub fn is_response(&self) -> bool {
        matches!(
            self.message_type.class,
            StunClass::SuccessResponse | StunClass::ErrorResponse
        )
    }

    /// Check if this is an error response
    pub fn is_error(&self) -> bool {
        matches!(self.message_type.class, StunClass::ErrorResponse)
    }
}

/// Configuration for STUN operations
#[derive(Debug, Clone)]
pub struct StunConfig {
    /// STUN server addresses
    pub servers: Vec<SocketAddr>,
    /// Request timeout
    pub request_timeout: std::time::Duration,
    /// Maximum number of retries
    pub max_retries: u32,
    /// Retry interval
    pub retry_interval: std::time::Duration,
    /// Enable ICE support
    pub enable_ice: bool,
}

impl Default for StunConfig {
    fn default() -> Self {
        Self {
            servers: Vec::new(),
            request_timeout: std::time::Duration::from_secs(3),
            max_retries: 3,
            retry_interval: std::time::Duration::from_millis(500),
            enable_ice: false,
        }
    }
}
