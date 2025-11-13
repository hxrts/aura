//! Device Registry and Lifecycle Management
//!
//! This module provides device lifecycle management and organizational identity
//! tracking. Maintains registry of devices and their status (Active, Suspended, Revoked).

mod verifier;

pub use verifier::{DeviceInfo, DeviceStatus, IdentityVerifier, VerificationResult};
