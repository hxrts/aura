//! Reactive and callback signal surfaces for `AppCore`.
//!
//! The authoritative implementation remains on `AppCore`; this module exists so
//! signal-facing responsibilities have a dedicated structural home.

#![allow(unused_imports)]

pub use super::legacy::AppCore;
#[cfg(feature = "callbacks")]
pub use super::legacy::SubscriptionId;
