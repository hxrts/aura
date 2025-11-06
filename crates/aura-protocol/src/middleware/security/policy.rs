//! Policy middleware

use aura_macros::AuraMiddleware;

/// Middleware that enforces policy policies
#[derive(AuraMiddleware)]
#[middleware(
    effects = "[NetworkEffects, CryptoEffects, TimeEffects, StorageEffects, LedgerEffects, ConsoleEffects, ChoreographicEffects, RandomEffects]"
)]
pub struct PolicyMiddleware<H> {
    inner: H,
}

impl<H> PolicyMiddleware<H> {
    /// Create a new policy middleware wrapping the given handler
    ///
    /// # Arguments
    /// * `handler` - The inner handler to wrap with policy checking
    pub fn new(handler: H) -> Self {
        Self { inner: handler }
    }
}
