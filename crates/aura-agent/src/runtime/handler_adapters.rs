//! Handler adapter stubs
//!
//! Placeholder for handler adapters that bridge between different
//! effect handler interfaces in the authority-centric architecture.

/// Stub handler adapter
#[derive(Debug)]
pub struct HandlerAdapter;

impl HandlerAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl Default for HandlerAdapter {
    fn default() -> Self {
        Self::new()
    }
}