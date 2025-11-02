//! Rumpsteak-Aura integration layer for choreographic protocols
//!
//! This module provides the integration between Aura's protocol execution framework
//! and the Rumpsteak choreographic programming library. It enables type-safe,
//! deadlock-free distributed protocol implementations with comprehensive middleware
//! support, error handling, and Byzantine fault tolerance.
//!
//! # Architecture
//!
//! The integration consists of several key components:
//!
//! - **Handler Adapter**: Bridges between Aura's protocol handlers and Rumpsteak's
//!   choreographic handlers, enabling seamless protocol execution.
//! - **Middleware Integration**: Connects choreographic protocols with Aura's
//!   middleware stack (tracing, metrics, capabilities, error recovery).
//! - **Error Handling**: Comprehensive error handling that integrates with
//!   aura-types error system, ensuring zero panics in production.
//! - **Timeout Management**: Configurable timeouts for all protocol operations
//!   with adaptive behavior based on network conditions.
//! - **Byzantine Tolerance**: Built-in support for handling up to 33% Byzantine
//!   participants with detection and recovery mechanisms.
//!
//! # Examples
//!
//! ## Basic Choreographic Protocol
//!
//! ```rust,ignore
//! use aura_protocol::protocols::choreographic::{
//!     ChoreographicHandlerBuilder, ChoreographyMiddlewareConfig,
//! };
//!
//! // Create a handler with full middleware stack
//! let config = ChoreographyMiddlewareConfig::default();
//! let handler = ChoreographicHandlerBuilder::new(effects)
//!     .with_config(config)
//!     .build_in_memory(device_id, context);
//!
//! // Execute choreographic protocols with automatic error handling,
//! // timeout management, and Byzantine fault tolerance
//! ```
//!
//! ## Error-Safe Protocol Execution
//!
//! ```rust,ignore
//! use aura_protocol::protocols::choreographic::error_handling::{
//!     SafeChoreography, choreo_assert,
//! };
//!
//! // Wrap protocol in error-safe handler
//! let mut safe_protocol = SafeChoreography::new(protocol);
//!
//! // Execute with automatic error conversion
//! let result = safe_protocol.execute(|p| {
//!     choreo_assert!(participants.len() >= 3, "Need at least 3 participants");
//!     p.run_protocol()
//! }).await?;
//! ```
//!
//! ## Timeout Management
//!
//! ```rust,ignore
//! use aura_protocol::protocols::choreographic::timeout_management::{
//!     TimeoutManager, OperationType,
//! };
//!
//! let timeout_mgr = TimeoutManager::new();
//!
//! // Execute with appropriate timeout for operation type
//! let result = timeout_mgr.with_timeout(
//!     OperationType::Frost,
//!     frost_signing_protocol()
//! ).await?;
//! ```
//!
//! # Byzantine Fault Tolerance
//!
//! All choreographic protocols automatically handle Byzantine participants:
//!
//! - Detection of malicious behavior (corrupted messages, equivocation)
//! - Automatic timeout handling for unresponsive participants
//! - Protocol completion with up to 33% Byzantine participants
//! - Comprehensive reporting of Byzantine behaviors
//!
//! # Production Hardening
//!
//! This module implements Phase 5 production hardening:
//!
//! - ✓ Zero panics - all errors properly handled
//! - ✓ Configurable timeouts for all operations
//! - ✓ Byzantine fault tolerance with 33% threshold
//! - ✓ Comprehensive API documentation
//! - ✓ Integration with aura-types error system

pub mod byzantine_tests;
pub mod error_handling;
pub mod handler_adapter;
pub mod middleware_integration;
pub mod production_example;
pub mod simulation_handler;
pub mod time_travel;
pub mod timeout_management;
pub mod trace;

pub use error_handling::{
    choreo_assertion_failed, ByzantineDetector, ByzantineReport, ChoreographyErrorExt,
    ChoreographyResult, SafeChoreography,
};
pub use handler_adapter::{BridgedEndpoint, BridgedRole, RumpsteakAdapter};
pub use middleware_integration::{
    ChoreographicHandlerBuilder, ChoreographicMiddlewareExt, ChoreographyMiddlewareConfig,
};
pub use simulation_handler::{
    create_simulation_handler, ChoreoEvent, SimulationChoreoHandler, SimulationConfig,
};
pub use time_travel::{
    export_time_travel_session, ChoreoCheckpoint, TimeTravelDebugInfo, TimeTravelDebugger,
    TimeTravelSimulationHandler,
};
pub use timeout_management::{
    timebox, DeadlineTracker, OperationType, TimeoutConfig, TimeoutManager,
};
pub use trace::{ChoreoTraceContext, ChoreoTraceExport, ChoreoTraceRecorder};
