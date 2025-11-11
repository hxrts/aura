//! Tree operation modules

pub mod choreography;
pub mod choreography_impl;
pub mod coordinator;

// Re-export specific types to avoid conflicts
pub use choreography::TreeOpMessage;
pub use choreography_impl::{TreeOpChoreography, TreeOpRole};
pub use coordinator::TreeOperationCoordinator;
