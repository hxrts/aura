//! # Aura Composition - Layer 3: Implementation (Handler Composition)
//!
//! **Purpose**: Assemble individual handlers into cohesive effect systems.
//!
//! This crate provides effect handler composition, registry, and builder infrastructure
//! for assembling stateless handlers from the effects crate into runnable effect systems.
//!
//! # Architecture Constraints
//!
//! **Layer 3 depends only on aura-core and the effects implementations** (foundation + implementation).
//! - MUST handle composition of effect handlers (not implementation)
//! - MUST provide registry and builder patterns for handler assembly
//! - MUST manage effect system lifecycle (initialization, shutdown)
//! - MUST NOT implement effect handlers (that's the effects crate)
//! - MUST NOT do multi-party coordination (that's aura-protocol)
//! - MUST NOT depend on domain crates or higher layers
//!
//! # Key Components
//!
//! - EffectRegistry: Type-indexed storage of handler instances
//! - EffectSystemBuilder: Builder pattern for composing handlers
//! - EffectContainer: Runtime container managing handler lifecycle
//! - Handler lifecycle management (start/stop/configure)
//!
//! # What Belongs Here
//!
//! Handler composition and assembly infrastructure:
//! - Effect registry and lookup by type
//! - Builder patterns for effect system construction
//! - Handler composition utilities
//! - Lifecycle management (initialization, shutdown)
//!
//! # What Does NOT Belong Here
//!
//! - Individual handler implementations (that's the effects crate)
//! - Multi-party protocol logic (that's aura-protocol)
//! - Runtime-specific concerns like signal handling (that's aura-agent)
//! - Application lifecycle management (that's aura-agent)
//!
//! # Usage Pattern
//!
//! Feature crates compose handlers without pulling in full runtime infrastructure:
//!
//! ```rust,ignore
//! use aura_composition::{EffectSystemBuilder};
//! use aura_effects::RealCryptoHandler;
//!
//! let effects = EffectSystemBuilder::new()
//!     .with_handler(Arc::new(RealCryptoHandler::new()))
//!     .build()
//!     .await?;
//! ```

#![allow(missing_docs)]

pub mod adapters;
pub mod composite;
pub mod registry;

// Re-export core types for convenience
pub use registry::{
    EffectCapability, EffectRegistry, Handler, HandlerContext, HandlerError, RegistrableHandler,
    RegistryCapabilities, RegistryError,
};

pub use composite::{
    CompositeError, CompositeHandler, CompositeHandlerAdapter, CompositeHandlerBuilder,
};

pub use adapters::{
    ConsoleHandlerAdapter, CryptoHandlerAdapter, LoggingSystemHandlerAdapter, RandomHandlerAdapter,
    StorageHandlerAdapter, TimeHandlerAdapter, TransportHandlerAdapter,
};
