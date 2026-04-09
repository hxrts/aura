//! Query type definitions
//!
//! Each query type implements the `aura_core::Query` trait for typed Datalog compilation.

#![allow(missing_docs)]

pub mod channels;
pub mod common;
pub mod contacts;
pub mod messages;
pub mod recovery;

pub use channels::*;
pub use common::*;
pub use contacts::*;
pub use messages::*;
pub use recovery::*;
