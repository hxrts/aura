//! # Subscription Registry
//!
//! Centralized management of signal subscriptions for TUI screens.
//!
//! The registry lazily initializes subscriptions on first access and caches
//! them for subsequent use. This prevents duplicate subscriptions and ensures
//! consistent access to shared state across components.

use std::cell::OnceCell;

use iocraft::prelude::*;

use crate::tui::hooks::AppCoreContext;
use crate::tui::screens::app::subscriptions::{
    use_channels_subscription, use_contacts_subscription, use_invitations_subscription,
    use_messages_subscription, use_neighborhood_blocks_subscription,
    use_pending_requests_subscription, use_residents_subscription, SharedChannels, SharedContacts,
    SharedInvitations, SharedMessages, SharedNeighborhoodBlocks, SharedPendingRequests,
    SharedResidents,
};

/// Centralized registry for signal subscriptions.
///
/// The registry uses `OnceCell` for lazy initialization of each subscription.
/// Once a subscription is created, it is cached and reused for subsequent
/// access, preventing duplicate subscriptions and ensuring consistent state.
///
/// # Example
///
/// ```rust,ignore
/// // Create registry once per component
/// let registry = hooks.use_ref(|| SubscriptionRegistry::new());
///
/// // Access subscriptions - initialized lazily on first use
/// let contacts = registry.write().contacts(hooks, &app_ctx);
/// let messages = registry.write().messages(hooks, &app_ctx);
///
/// // Subsequent accesses return the cached subscription
/// let contacts_again = registry.read().get_contacts().unwrap();
/// ```
///
/// # Thread Safety
///
/// All shared state uses `Arc<RwLock<T>>` for thread-safe access without
/// triggering re-renders on every update. This allows dispatch handlers
/// to read current state synchronously.
#[derive(Default)]
pub struct SubscriptionRegistry {
    contacts: OnceCell<SharedContacts>,
    residents: OnceCell<SharedResidents>,
    messages: OnceCell<SharedMessages>,
    channels: OnceCell<SharedChannels>,
    invitations: OnceCell<SharedInvitations>,
    neighborhood_blocks: OnceCell<SharedNeighborhoodBlocks>,
    pending_requests: OnceCell<SharedPendingRequests>,
}

impl SubscriptionRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    // =========================================================================
    // Lazy-initializing accessors
    //
    // These methods initialize the subscription on first access and cache it.
    // =========================================================================

    /// Get or create the contacts subscription.
    ///
    /// Initializes the subscription on first access using the provided hooks.
    pub fn contacts(&mut self, hooks: &mut Hooks, app_ctx: &AppCoreContext) -> SharedContacts {
        self.contacts
            .get_or_init(|| use_contacts_subscription(hooks, app_ctx))
            .clone()
    }

    /// Get or create the residents subscription.
    ///
    /// Initializes the subscription on first access using the provided hooks.
    pub fn residents(&mut self, hooks: &mut Hooks, app_ctx: &AppCoreContext) -> SharedResidents {
        self.residents
            .get_or_init(|| use_residents_subscription(hooks, app_ctx))
            .clone()
    }

    /// Get or create the messages subscription.
    ///
    /// Initializes the subscription on first access using the provided hooks.
    pub fn messages(&mut self, hooks: &mut Hooks, app_ctx: &AppCoreContext) -> SharedMessages {
        self.messages
            .get_or_init(|| use_messages_subscription(hooks, app_ctx))
            .clone()
    }

    /// Get or create the channels subscription.
    ///
    /// Initializes the subscription on first access using the provided hooks.
    pub fn channels(&mut self, hooks: &mut Hooks, app_ctx: &AppCoreContext) -> SharedChannels {
        self.channels
            .get_or_init(|| use_channels_subscription(hooks, app_ctx))
            .clone()
    }

    /// Get or create the invitations subscription.
    ///
    /// Initializes the subscription on first access using the provided hooks.
    pub fn invitations(
        &mut self,
        hooks: &mut Hooks,
        app_ctx: &AppCoreContext,
    ) -> SharedInvitations {
        self.invitations
            .get_or_init(|| use_invitations_subscription(hooks, app_ctx))
            .clone()
    }

    /// Get or create the neighborhood blocks subscription.
    ///
    /// Initializes the subscription on first access using the provided hooks.
    pub fn neighborhood_blocks(
        &mut self,
        hooks: &mut Hooks,
        app_ctx: &AppCoreContext,
    ) -> SharedNeighborhoodBlocks {
        self.neighborhood_blocks
            .get_or_init(|| use_neighborhood_blocks_subscription(hooks, app_ctx))
            .clone()
    }

    /// Get or create the pending requests subscription.
    ///
    /// Initializes the subscription on first access using the provided hooks.
    pub fn pending_requests(
        &mut self,
        hooks: &mut Hooks,
        app_ctx: &AppCoreContext,
    ) -> SharedPendingRequests {
        self.pending_requests
            .get_or_init(|| use_pending_requests_subscription(hooks, app_ctx))
            .clone()
    }

    // =========================================================================
    // Non-initializing getters
    //
    // These methods return cached subscriptions without initialization.
    // =========================================================================

    /// Get the contacts subscription if already initialized.
    pub fn get_contacts(&self) -> Option<SharedContacts> {
        self.contacts.get().cloned()
    }

    /// Get the residents subscription if already initialized.
    pub fn get_residents(&self) -> Option<SharedResidents> {
        self.residents.get().cloned()
    }

    /// Get the messages subscription if already initialized.
    pub fn get_messages(&self) -> Option<SharedMessages> {
        self.messages.get().cloned()
    }

    /// Get the channels subscription if already initialized.
    pub fn get_channels(&self) -> Option<SharedChannels> {
        self.channels.get().cloned()
    }

    /// Get the invitations subscription if already initialized.
    pub fn get_invitations(&self) -> Option<SharedInvitations> {
        self.invitations.get().cloned()
    }

    /// Get the neighborhood blocks subscription if already initialized.
    pub fn get_neighborhood_blocks(&self) -> Option<SharedNeighborhoodBlocks> {
        self.neighborhood_blocks.get().cloned()
    }

    /// Get the pending requests subscription if already initialized.
    pub fn get_pending_requests(&self) -> Option<SharedPendingRequests> {
        self.pending_requests.get().cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_default() {
        let registry = SubscriptionRegistry::new();

        // All subscriptions should be uninitialized
        assert!(registry.get_contacts().is_none());
        assert!(registry.get_residents().is_none());
        assert!(registry.get_messages().is_none());
        assert!(registry.get_channels().is_none());
        assert!(registry.get_invitations().is_none());
        assert!(registry.get_neighborhood_blocks().is_none());
        assert!(registry.get_pending_requests().is_none());
    }
}
