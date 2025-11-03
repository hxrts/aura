//! Effect Handlers Module
//!
//! This module contains concrete implementations of the effect traits defined in the effects module.
//! Following the algebraic effects pattern, handlers interpret effect operations and provide
//! different implementations (real, mock, simulation) for different execution contexts.
//!
//! ## Architecture Principles
//!
//! 1. **Multiple Implementations**: Each effect has multiple handlers (real, mock, simulation)
//! 2. **Handler Selection**: Choose appropriate handler based on execution context
//! 3. **Composability**: Handlers can be combined into composite effect providers
//! 4. **Middleware Integration**: Handlers can be wrapped with middleware decorators
//!
//! ## Handler Categories
//!
//! - **Network Handlers**: Memory, real network, simulated network
//! - **Storage Handlers**: Memory, filesystem, distributed storage
//! - **Crypto Handlers**: Real crypto, mock crypto for testing
//! - **Time Handlers**: System time, simulated time for deterministic testing
//! - **Console Handlers**: Stdout, structured logging, silent for tests
//! - **Ledger Handlers**: Memory ledger, persistent ledger
//! - **Choreographic Handlers**: Rumpsteak integration, local execution
//!
//! ## Usage Pattern
//!
//! ```rust,ignore
//! use crate::handlers::{
//!     network::MemoryNetworkHandler,
//!     crypto::RealCryptoHandler,
//!     time::SimulatedTimeHandler,
//!     composite::CompositeHandler,
//! };
//!
//! // Create individual handlers
//! let network = MemoryNetworkHandler::new();
//! let crypto = RealCryptoHandler::new();
//! let time = SimulatedTimeHandler::new();
//!
//! // Combine into composite handler
//! let effects = CompositeHandler::builder()
//!     .with_network(network)
//!     .with_crypto(crypto)
//!     .with_time(time)
//!     .build();
//! ```

pub mod choreographic;
pub mod composite;
pub mod console;
pub mod crypto;
pub mod ledger;
pub mod network;
pub mod storage;
pub mod time;

// Re-export commonly used handlers
pub use composite::CompositeHandler;
pub use console::{SilentConsoleHandler, StdoutConsoleHandler, StructuredConsoleHandler};
pub use crypto::{MockCryptoHandler, RealCryptoHandler};
pub use network::{MemoryNetworkHandler, RealNetworkHandler, SimulatedNetworkHandler};
pub use storage::{MemoryStorageHandler, FilesystemStorageHandler};
pub use time::{RealTimeHandler, SimulatedTimeHandler};

/// Builder for creating composite effect handlers
pub struct HandlerBuilder {
    device_id: uuid::Uuid,
    is_simulation: bool,
}

impl HandlerBuilder {
    /// Create a new handler builder
    pub fn new(device_id: uuid::Uuid) -> Self {
        Self {
            device_id,
            is_simulation: false,
        }
    }

    /// Enable simulation mode
    pub fn simulation(mut self) -> Self {
        self.is_simulation = true;
        self
    }

    /// Build handlers optimized for testing
    pub fn for_testing(self) -> CompositeHandler {
        CompositeHandler::for_testing(self.device_id)
    }

    /// Build handlers for production use
    pub fn for_production(self) -> CompositeHandler {
        CompositeHandler::for_production(self.device_id)
    }

    /// Build handlers for simulation/deterministic testing
    pub fn for_simulation(self) -> CompositeHandler {
        CompositeHandler::for_simulation(self.device_id)
    }
}