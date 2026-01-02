//! Leakage tracking handlers for testing
//!
//! The production `ProductionLeakageHandler` lives in `aura-effects` and composes
//! `StorageEffects` for persistence. For testing, use:
//!
//! ```rust,ignore
//! use aura_effects::leakage_handler::ProductionLeakageHandler;
//! use aura_testkit::stateful_effects::MemoryStorageHandler;
//! use std::sync::Arc;
//!
//! let storage = Arc::new(MemoryStorageHandler::new());
//! let handler = ProductionLeakageHandler::with_storage(storage);
//! ```
