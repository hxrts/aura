//! Adversary Simulation Framework
//!
//! Provides adversarial components for security testing:
//! - Byzantine devices that deviate from protocol
//! - Network adversaries (MITM, DoS, eclipse attacks)
//! - Message schedulers for worst-case delivery orders
//! - Fault injection for cryptographic and network operations
//!
//! All adversaries operate deterministically using the simulation's Effects.

pub mod byzantine;
pub mod network;
pub mod scheduler;

pub use byzantine::{ByzantineDevice, ByzantineStrategy};
pub use network::{NetworkAdversary, NetworkAttack};
pub use scheduler::{AdversarialScheduler, DeliveryStrategy, MessageReordering};
