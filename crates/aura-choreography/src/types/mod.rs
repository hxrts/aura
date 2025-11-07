//! Rumpsteak-compatible type definitions for choreography generation
//!
//! This module contains role and message types that work with the `choreography!` macro
//! from rumpsteak-aura to generate session types.

pub mod messages;
pub mod roles;

pub use messages::*;
pub use roles::*;
