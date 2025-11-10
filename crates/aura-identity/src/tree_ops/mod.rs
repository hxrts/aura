//! Tree operation modules

pub mod choreography;
pub mod choreography_impl;

// Re-export specific types to avoid conflicts
pub use choreography::TreeOpMessage;
pub use choreography_impl::{TreeOpChoreography, TreeOpRole};
