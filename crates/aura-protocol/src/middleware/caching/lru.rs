//! LRU cache middleware (placeholder)

use crate::middleware::Middleware;

pub struct LruCacheMiddleware<H> {
    inner: H,
}

impl<H> LruCacheMiddleware<H> {
    pub fn new(handler: H) -> Self {
        Self { inner: handler }
    }
}

impl<H> Middleware<H> for LruCacheMiddleware<H> {
    type Decorated = LruCacheMiddleware<H>;

    fn apply(self, handler: H) -> Self::Decorated {
        LruCacheMiddleware::new(handler)
    }
}