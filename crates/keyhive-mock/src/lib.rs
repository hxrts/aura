//! Mock implementation of Keyhive capabilities and BeeKEM protocol
//! 
//! This is a placeholder implementation that provides the API surface
//! needed for Aura's Keyhive integration without the actual logic.
//! 
//! TODO: Replace this with the real keyhive_core crate when available.

#![allow(missing_docs)]

pub mod capability;
pub mod cgka;

pub use capability::*;
pub use cgka::*;

#[derive(Debug, thiserror::Error)]
pub enum KeyhiveError {
    #[error("Not implemented: {0}")]
    NotImplemented(String),
    #[error("Invalid capability: {0}")]
    InvalidCapability(String),
    #[error("BeeKEM error: {0}")]
    BeeKemError(String),
    #[error("Serialization error: {0}")]
    SerializationError(String),
}

pub type Result<T> = std::result::Result<T, KeyhiveError>;