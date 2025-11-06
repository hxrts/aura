//! Simulation Middleware Components
//!
//! This module contains simulation-specific middleware implementations for
//! fault injection, time control, state inspection, property checking,
//! and chaos coordination using the unified Aura middleware architecture.

pub mod fault_injection;
pub mod time_control;
pub mod state_inspection;
pub mod property_checking;
pub mod chaos_coordination;

// Re-export middleware implementations
pub use fault_injection::FaultInjectionMiddleware;
pub use time_control::TimeControlMiddleware;
pub use state_inspection::StateInspectionMiddleware;
pub use property_checking::PropertyCheckingMiddleware;
pub use chaos_coordination::ChaosCoordinationMiddleware;