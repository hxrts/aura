//! # Runtime Builder System
//!
//! Ergonomic builder API for constructing Aura agents with platform presets
//! and compile-time safety for required effects.
//!
//! ## Design Principles
//!
//! 1. **Extend existing patterns** - Build on `AgentBuilder` and `EffectSystemBuilder`
//! 2. **Compile-time safety** - Required effects enforced by Rust's type system
//! 3. **Progressive disclosure** - Simple cases are simple, complex cases are possible
//! 4. **Discoverability** - IDE autocomplete guides developers
//! 5. **Granular override** - Presets can be customized piece by piece
//!
//! ## Available Presets
//!
//! | Preset | Platform | Feature Flag |
//! |--------|----------|--------------|
//! | `AgentBuilder::cli()` | Desktop/Server | (default) |
//! | `AgentBuilder::ios()` | iOS | `ios` |
//! | `AgentBuilder::android()` | Android | `android` |
//! | `AgentBuilder::web()` | Browser/WASM | `web` |
//! | `AgentBuilder::custom()` | Any | (default) |
//!
//! ## Usage
//!
//! ```rust,ignore
//! use aura_agent::AgentBuilder;
//!
//! // Platform preset - simplest path
//! let agent = AgentBuilder::cli()
//!     .data_dir(PathBuf::from("~/.aura"))
//!     .build()
//!     .await?;
//!
//! // Custom platform - explicit but guided
//! let agent = AgentBuilder::custom()
//!     .with_crypto(MyHsmCrypto::new())
//!     .with_storage(PostgresStorage::new(pool))
//!     .with_time(PhysicalTimeHandler::new())
//!     .with_random(RealRandomHandler::new())
//!     .with_console(RealConsoleHandler::new(false))
//!     .build()
//!     .await?;
//! ```

// Core presets (always available)
mod cli;
mod custom;
mod error;

// Platform-specific presets (always compiled, but require feature flags to build)
mod android;
mod ios;
mod web;

// Core exports
pub use cli::CliPresetBuilder;
pub use custom::CustomPresetBuilder;
pub use error::BuildError;

// Re-export marker types for typestate pattern
pub use custom::{Missing, Provided};

// Platform preset exports
pub use android::AndroidPresetBuilder;
pub use ios::{DataProtectionClass, IosPresetBuilder};
pub use web::WebPresetBuilder;
