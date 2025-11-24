//! Mock authorization effect handlers for testing

/// Mock authorization handler for testing
#[derive(Debug)]
pub struct MockAuthorizationHandler;

impl Default for MockAuthorizationHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl MockAuthorizationHandler {
    pub fn new() -> Self {
        Self
    }
}
