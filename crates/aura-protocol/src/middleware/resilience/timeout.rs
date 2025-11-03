//! Timeout middleware for effect handlers (placeholder)

use crate::middleware::Middleware;

/// Timeout middleware that adds operation timeouts
pub struct TimeoutMiddleware<H> {
    inner: H,
    timeout_ms: u64,
}

impl<H> TimeoutMiddleware<H> {
    pub fn new(handler: H, timeout_ms: u64) -> Self {
        Self { inner: handler, timeout_ms }
    }
}

impl<H> Middleware<H> for TimeoutMiddleware<H> {
    type Decorated = TimeoutMiddleware<H>;

    fn apply(self, handler: H) -> Self::Decorated {
        TimeoutMiddleware::new(handler, self.timeout_ms)
    }
}