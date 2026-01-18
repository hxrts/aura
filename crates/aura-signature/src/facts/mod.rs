//! Domain fact types for identity and device lifecycle.
//!
//! This module contains fact types that capture identity verification and
//! device lifecycle events. These facts are stored in journals and reduced
//! to derive authority and device state.
//!
//! # Architecture
//!
//! Layer 2 fact types use `aura-core::types::facts` infrastructure:
//! - `aura-signature` provides identity/device lifecycle fact types
//! - Facts use `FactTypeId` and `try_encode`/`try_decode` APIs
//! - No dependency on `aura-journal` (Layer 2 → Layer 2 would be circular)
//!
//! # Modules
//!
//! - `verification`: Authority lifecycle and verification facts
//! - `device_naming`: Device nickname suggestion updates

pub mod device_naming;
pub mod verification;

// Re-export commonly used types
pub use device_naming::{
    derive_device_naming_context, device_naming_fact_type_id, DeviceNamingFact,
    DEVICE_NAMING_FACT_TYPE_ID, DEVICE_NAMING_SCHEMA_VERSION, NICKNAME_SUGGESTION_BYTES_MAX,
};
pub use verification::{
    verify_fact_type_id, Confidence, ConfidenceError, PublicKeyBytes, PublicKeyBytesError,
    RevocationReason, VerificationType, VerifyFact, VerifyFactDelta, VerifyFactReducer,
    VERIFY_FACT_SCHEMA_VERSION, VERIFY_FACT_TYPE_ID,
};
