//! Tree handler implementations
//!
//! The production tree handlers were removed during the refactor, but a dummy
//! handler remains useful for tests and composite handler wiring. This module
//! provides that placeholder implementation.

pub mod dummy;

pub use dummy::DummyTreeHandler;
