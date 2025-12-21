//! # Subscription Registry
//!
//! Centralized signal subscriptions for TUI screens.
//!
//! This module provides a `SubscriptionRegistry` that lazily initializes
//! and caches signal subscriptions, preventing duplicate subscriptions
//! and reducing boilerplate in screen components.
//!
//! ## Usage
//!
//! ```rust,ignore
//! // In a screen component
//! let registry = hooks.use_ref(|| SubscriptionRegistry::new(app_ctx.clone()));
//! let contacts = registry.read().contacts(hooks, &app_ctx);
//! let messages = registry.read().messages(hooks, &app_ctx);
//! ```
//!
//! ## Architecture
//!
//! - `SubscriptionRegistry` uses `OnceCell` for lazy initialization
//! - Each subscription is created at most once per registry instance
//! - Subscriptions use `Arc<RwLock<T>>` for thread-safe, non-blocking access
//! - The subscription functions from `screens/app/subscriptions.rs` are reused

mod registry;

pub use registry::SubscriptionRegistry;

// Re-export shared types from the original subscriptions module for convenience
pub use crate::tui::screens::app::subscriptions::{
    use_channels_subscription, use_contacts_subscription, use_invitations_subscription,
    use_messages_subscription, use_nav_status_signals, use_neighborhood_blocks_subscription,
    use_pending_requests_subscription, use_residents_subscription, NavStatusSignals,
    SharedChannels, SharedContacts, SharedInvitations, SharedMessages, SharedNeighborhoodBlocks,
    SharedPendingRequests, SharedResidents,
};
