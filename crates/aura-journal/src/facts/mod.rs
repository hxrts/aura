//! Domain fact modules for aura-journal.
//!
//! This module contains domain-specific fact types that implement the `DomainFact`
//! trait. Facts are stored as `RelationalFact::Generic` in the journal and reduced
//! using registered `FactReducer` implementations.
//!
//! # Architecture
//!
//! Following the Open/Closed Principle:
//! - `aura-journal` provides the generic fact infrastructure
//! - Domain-specific fact types are defined in submodules
//! - Runtime registers reducers with the `FactRegistry`
//!
//! # Fact Type IDs
//!
//! Each domain fact type has a unique string identifier:
//! - `"device_naming"` - Device nickname suggestion updates

pub mod device_naming;

pub use device_naming::{
    derive_device_naming_context, DeviceNamingFact, DeviceNamingFactKey, DeviceNamingFactReducer,
    DEVICE_NAMING_FACT_TYPE_ID, DEVICE_NAMING_SCHEMA_VERSION, NICKNAME_SUGGESTION_BYTES_MAX,
};
