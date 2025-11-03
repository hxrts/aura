//! Capability middleware (placeholder)

use crate::middleware::Middleware;

pub struct CapabilityMiddleware<H> {
    inner: H,
}

impl<H> CapabilityMiddleware<H> {
    pub fn new(handler: H) -> Self {
        Self { inner: handler }
    }
}

impl<H> Middleware<H> for CapabilityMiddleware<H> {
    type Decorated = CapabilityMiddleware<H>;

    fn apply(self, handler: H) -> Self::Decorated {
        CapabilityMiddleware::new(handler)
    }
}