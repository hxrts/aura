//! Core Handler Infrastructure
//!
//! This module contains the foundational components for the Aura handler system,
//! providing the building blocks for handler composition, registration, and orchestration.
//!
//! ## Components
//!
//! - **Composite**: Multi-handler composition and delegation patterns
//! - **Erased**: Type-erased handler traits for dynamic dispatch
//! - **Factory**: Handler construction and configuration patterns
//! - **Registry**: Handler registration and lookup mechanisms
//!
//! These components work together to provide the core infrastructure that enables
//! the flexible, composable handler architecture used throughout the protocol layer.

pub mod composite;
pub mod erased;
pub mod factory;
pub mod registry;

pub use composite::CompositeHandler;
pub use erased::{AuraHandler, BoxedHandler, HandlerUtils};
pub use factory::{AuraHandlerBuilder, AuraHandlerConfig, AuraHandlerFactory, FactoryError};
pub use registry::{EffectRegistry, RegistrableHandler, RegistryError};