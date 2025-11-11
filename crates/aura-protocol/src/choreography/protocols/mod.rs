//! Choreographic Protocol Implementations
//!
//! This module contains concrete choreographic protocol implementations for various
//! distributed operations in Aura.

pub mod anti_entropy;
pub mod consensus;
pub mod frost;
pub mod snapshot;
pub mod threshold_ceremony;
pub mod tree_coordination;

// Re-export protocol types for convenience
pub use anti_entropy::*;
pub use consensus::*;
pub use frost::*;
pub use snapshot::*;
pub use threshold_ceremony::*;
pub use tree_coordination::*;
