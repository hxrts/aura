//! Authorization decision logic
//!
//! This module contains the high-level decision functions that integrate
//! authentication, capability checking, and policy evaluation to make
//! final authorization decisions.

pub mod access_control;
pub mod event_auth;

pub use access_control::{make_access_decision, AccessDecision, AccessRequest};
pub use event_auth::{authorize_event, EventAuthorizationResult};
