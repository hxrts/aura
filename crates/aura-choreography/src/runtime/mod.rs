//! Runtime infrastructure for executing rumpsteak-generated choreographies
//!
//! This module contains the bridge between rumpsteak-aura generated session types
//! and aura's effect system.

pub mod aura_handler_adapter;

pub use aura_handler_adapter::*;
