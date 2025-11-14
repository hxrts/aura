//! Simulation Middleware Components
//!
//! This module contains simulation-specific middleware implementations for
//! fault injection, time control, state inspection, property checking,
//! and chaos coordination using the unified Aura middleware architecture.

pub mod chaos_coordination;
pub mod fault_injection;
pub mod property_checking;
pub mod state_inspection;
pub mod time_control;

// Re-export middleware implementations
pub use chaos_coordination::ChaosCoordinationMiddleware;
pub use fault_injection::FaultInjectionMiddleware;
pub use property_checking::PropertyCheckingMiddleware;
pub use state_inspection::StateInspectionMiddleware;
pub use time_control::TimeControlMiddleware;
