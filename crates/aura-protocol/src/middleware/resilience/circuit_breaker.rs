//! Circuit breaker middleware for effect handlers (placeholder)

use crate::middleware::Middleware;

/// Circuit breaker middleware that prevents cascade failures
pub struct CircuitBreakerMiddleware<H> {
    inner: H,
}

impl<H> CircuitBreakerMiddleware<H> {
    pub fn new(handler: H) -> Self {
        Self { inner: handler }
    }
}

impl<H> Middleware<H> for CircuitBreakerMiddleware<H> {
    type Decorated = CircuitBreakerMiddleware<H>;

    fn apply(self, handler: H) -> Self::Decorated {
        CircuitBreakerMiddleware::new(handler)
    }
}