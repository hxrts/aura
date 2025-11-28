//! Leakage tracking handlers for testing
//!
//! **Note**: The production `ProductionLeakageHandler` now lives in `aura-effects`
//! and composes `StorageEffects` for persistence. For testing, use:
//!
//! ```rust,ignore
//! use aura_effects::leakage_handler::ProductionLeakageHandler;
//! use aura_testkit::stateful_effects::MemoryStorageHandler;
//! use std::sync::Arc;
//!
//! let storage = Arc::new(MemoryStorageHandler::new());
//! let handler = ProductionLeakageHandler::with_storage(storage);
//! ```
//!
//! This module is kept for backwards compatibility but the handlers have been
//! removed as they were dead code that didn't implement `LeakageEffects`.

// Legacy types removed - use aura_effects::ProductionLeakageHandler<S> with
// MemoryStorageHandler from this crate for testing purposes.
