//! Handler Adapters
//!
//! Adapters that bridge between effect traits and concrete handler implementations.

use aura_core::{AuraResult};
use async_trait::async_trait;
use std::sync::Arc;

/// Stub handler adapter types until we refactor to new architecture
pub struct ConsoleHandlerAdapter;
pub struct CryptoHandlerAdapter;
pub struct JournalHandlerAdapter;
pub struct NetworkHandlerAdapter;
pub struct RandomHandlerAdapter;
pub struct StorageHandlerAdapter;
pub struct TimeHandlerAdapter;
pub struct LedgerHandlerAdapter;
pub struct SystemHandlerAdapter;
pub struct TreeHandlerAdapter;
pub struct ChoreographicHandlerAdapter;

impl ConsoleHandlerAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl CryptoHandlerAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl JournalHandlerAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl NetworkHandlerAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl RandomHandlerAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl StorageHandlerAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl TimeHandlerAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl LedgerHandlerAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl SystemHandlerAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl TreeHandlerAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl ChoreographicHandlerAdapter {
    pub fn new() -> Self {
        Self
    }
}
