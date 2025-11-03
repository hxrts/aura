//! Authorization middleware (placeholder)

use crate::middleware::Middleware;

pub struct AuthorizationMiddleware<H> {
    inner: H,
}

impl<H> AuthorizationMiddleware<H> {
    pub fn new(handler: H) -> Self {
        Self { inner: handler }
    }
}

impl<H> Middleware<H> for AuthorizationMiddleware<H> {
    type Decorated = AuthorizationMiddleware<H>;

    fn apply(self, handler: H) -> Self::Decorated {
        AuthorizationMiddleware::new(handler)
    }
}