//! Runtime-backed query and operation access for `AppCore`.
//!
//! The authoritative implementation remains on `AppCore`; this module exists so
//! runtime access can be reviewed separately from signal and hook surfaces.

#![allow(unused_imports)]

pub use super::legacy::{AppConfig, AppCore};
