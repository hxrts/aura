// Lock poisoning is fatal for this module - we prefer to panic than continue with corrupted state
#![allow(clippy::expect_used)]

//! Materialized Views over CRDTs
//!
//! This module provides `MaterializedView<T>`, a thin wrapper over `CvHandler`
//! that integrates with the subscription system and provides reactive updates
//! via `Dynamic<T>`.
//!
//! # Design Philosophy
//!
//! Materialized views in Aura leverage the existing CRDT infrastructure:
//! - Uses `CvHandler<T>` for join-semilattice state management
//! - Integrates with fact subscriptions for automatic updates
//! - Exposes state via `Dynamic<T>` for reactive UI binding
//!
//! # Example
//!
//! ```rust,ignore
//! use aura_effects::database::views::MaterializedView;
//! use aura_core::semilattice::{CvState, JoinSemilattice, Bottom};
//!
//! // Define a CRDT for aggregated metrics
//! #[derive(Clone, Debug)]
//! struct UserCount(u64);
//!
//! impl JoinSemilattice for UserCount {
//!     fn join(&self, other: &Self) -> Self {
//!         UserCount(self.0.max(other.0))
//!     }
//! }
//!
//! impl Bottom for UserCount {
//!     fn bottom() -> Self { UserCount(0) }
//! }
//!
//! impl CvState for UserCount {}
//!
//! // Create a materialized view
//! let view = MaterializedView::new("user_count".to_string(), auth_id);
//!
//! // Subscribe to reactive updates
//! let dynamic = view.subscribe();
//!
//! // Apply facts to update the view
//! view.apply_facts(&new_facts);
//!
//! // Current value automatically updates
//! println!("User count: {:?}", dynamic.get());
//! ```

use aura_core::{reactive::Dynamic, semilattice::CvState, types::identifiers::AuthorityId};
use std::sync::RwLock;

use super::subscription::SubscriptionFact;

/// A materialized view that maintains aggregated state from journal facts.
///
/// `MaterializedView<T>` combines:
/// - A CRDT state type `T` that implements `CvState`
/// - A `Dynamic<T>` for reactive subscriptions
/// - Integration with fact subscriptions
///
/// The view automatically updates its state when `apply_facts()` is called,
/// and all subscribers to the `Dynamic<T>` are notified of changes.
///
/// # Type Parameters
///
/// - `T`: The CRDT state type. Must implement `CvState`, which requires
///   `JoinSemilattice`, `Bottom`, `Clone`, `Send`, and `Sync`.
///
/// # Thread Safety
///
/// `MaterializedView<T>` is thread-safe when `T` is thread-safe.
/// Multiple threads can call `apply_facts()` concurrently, and the
/// join-semilattice property ensures deterministic results.
pub struct MaterializedView<T: CvState> {
    /// The query or predicate this view materializes
    query: String,
    /// Authority context for the view
    authority: AuthorityId,
    /// Current CRDT state
    state: RwLock<T>,
    /// Reactive value for subscribers
    dynamic: Dynamic<T>,
}

impl<T: CvState + Send + Sync + 'static> MaterializedView<T> {
    /// Create a new materialized view with the given query and authority.
    ///
    /// The view is initialized with the bottom element of the CRDT.
    ///
    /// # Parameters
    ///
    /// - `query`: The Datalog query or predicate prefix this view materializes
    /// - `authority`: The authority context for fact filtering
    pub fn new(query: String, authority: AuthorityId) -> Self {
        let initial = T::bottom();
        Self {
            query,
            authority,
            state: RwLock::new(initial.clone()),
            dynamic: Dynamic::new(initial),
        }
    }

    /// Create a materialized view with an initial state.
    ///
    /// # Parameters
    ///
    /// - `query`: The Datalog query or predicate prefix this view materializes
    /// - `authority`: The authority context for fact filtering
    /// - `initial`: Initial state value
    pub fn with_state(query: String, authority: AuthorityId, initial: T) -> Self {
        Self {
            query,
            authority,
            state: RwLock::new(initial.clone()),
            dynamic: Dynamic::new(initial),
        }
    }

    /// Subscribe to state changes.
    ///
    /// Returns a `Dynamic<T>` that will be updated whenever the view's
    /// state changes. The returned `Dynamic` shares state with the view,
    /// so updates are reflected in all subscribers.
    ///
    /// # Returns
    ///
    /// A `Dynamic<T>` for reactive access to the current state
    pub fn subscribe(&self) -> Dynamic<T> {
        self.dynamic.clone()
    }

    /// Get the current state.
    ///
    /// Returns a clone of the current CRDT state.
    pub fn get_state(&self) -> T {
        self.state
            .read()
            .expect("MaterializedView lock poisoned")
            .clone()
    }

    /// Get the query string this view materializes.
    pub fn query(&self) -> &str {
        &self.query
    }

    /// Get the authority context for this view.
    pub fn authority(&self) -> &AuthorityId {
        &self.authority
    }
}

impl<T: CvState + FactReducible + Send + Sync + 'static> MaterializedView<T> {
    /// Apply new facts to update the view state.
    ///
    /// This method:
    /// 1. Reduces the facts to a new CRDT state using `T::reduce_facts()`
    /// 2. Joins the new state with the current state
    /// 3. Updates the `Dynamic<T>` to notify subscribers
    ///
    /// # Parameters
    ///
    /// - `facts`: New facts to incorporate into the view
    ///
    /// # Returns
    ///
    /// `true` if the state changed, `false` if the join was idempotent
    pub fn apply_facts(&self, facts: &[SubscriptionFact]) -> bool {
        // Reduce facts to CRDT state
        let delta = T::reduce_facts(facts);

        // Join with current state
        let new_state = {
            let current = self.state.read().expect("MaterializedView lock poisoned");
            current.join(&delta)
        };

        // Check if state changed
        let changed = {
            let current = self.state.read().expect("MaterializedView lock poisoned");
            new_state != *current
        };

        if changed {
            // Update state
            {
                let mut state = self.state.write().expect("MaterializedView lock poisoned");
                *state = new_state.clone();
            }

            // Notify subscribers
            self.dynamic.set(new_state);
        }

        changed
    }

    /// Reset the view to the bottom element.
    ///
    /// Use this for testing or when you need to clear accumulated state.
    pub fn reset(&self) {
        let bottom = T::bottom();
        {
            let mut state = self.state.write().expect("MaterializedView lock poisoned");
            *state = bottom.clone();
        }
        self.dynamic.set(bottom);
    }
}

/// Trait for CRDT types that can be reduced from facts.
///
/// Implement this trait to enable `apply_facts()` on `MaterializedView<T>`.
///
/// # Example
///
/// ```rust,ignore
/// use aura_effects::database::views::FactReducible;
/// use aura_effects::database::subscription::SubscriptionFact;
///
/// struct MessageCount(u64);
///
/// impl FactReducible for MessageCount {
///     fn reduce_facts(facts: &[SubscriptionFact]) -> Self {
///         MessageCount(facts.len() as u64)
///     }
/// }
/// ```
pub trait FactReducible: CvState + PartialEq {
    /// Reduce a set of facts to a CRDT state.
    ///
    /// This function should be deterministic: the same facts should
    /// always produce the same result.
    fn reduce_facts(facts: &[SubscriptionFact]) -> Self;
}

impl<T: CvState + std::fmt::Debug + Send + Sync + 'static> std::fmt::Debug for MaterializedView<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self.state.read().expect("MaterializedView lock poisoned");
        f.debug_struct("MaterializedView")
            .field("query", &self.query)
            .field("authority", &self.authority)
            .field("state", &*state)
            .field("version", &self.dynamic.version())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::domain::journal::FactValue;
    use aura_core::effects::indexed::FactId;
    use aura_core::semilattice::{Bottom, JoinSemilattice};

    // Test CRDT type: max counter
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct TestCounter(u64);

    impl JoinSemilattice for TestCounter {
        fn join(&self, other: &Self) -> Self {
            TestCounter(self.0.max(other.0))
        }
    }

    impl Bottom for TestCounter {
        fn bottom() -> Self {
            TestCounter(0)
        }
    }

    impl CvState for TestCounter {}

    impl FactReducible for TestCounter {
        fn reduce_facts(facts: &[SubscriptionFact]) -> Self {
            // Count facts as the counter value
            TestCounter(facts.len() as u64)
        }
    }

    fn test_authority() -> AuthorityId {
        AuthorityId::new_from_entropy([1u8; 32])
    }

    fn test_fact(id: u64, predicate: &str) -> SubscriptionFact {
        SubscriptionFact {
            id: FactId::new(id),
            predicate: predicate.to_string(),
            value: FactValue::Number(id as i64),
            authority: None,
        }
    }

    #[test]
    fn test_materialized_view_new() {
        let view = MaterializedView::<TestCounter>::new("test.query".to_string(), test_authority());

        assert_eq!(view.get_state(), TestCounter(0));
        assert_eq!(view.query(), "test.query");
    }

    #[test]
    fn test_materialized_view_with_state() {
        let view = MaterializedView::with_state(
            "test.query".to_string(),
            test_authority(),
            TestCounter(42),
        );

        assert_eq!(view.get_state(), TestCounter(42));
    }

    #[test]
    fn test_materialized_view_subscribe() {
        let view = MaterializedView::<TestCounter>::new("test.query".to_string(), test_authority());

        let dynamic = view.subscribe();
        assert_eq!(dynamic.get(), TestCounter(0));

        // Update state
        let facts = vec![test_fact(1, "test")];
        view.apply_facts(&facts);

        // Dynamic should reflect the update
        assert_eq!(dynamic.get(), TestCounter(1));
    }

    #[test]
    fn test_materialized_view_apply_facts() {
        let view = MaterializedView::<TestCounter>::new("test.query".to_string(), test_authority());

        // Apply some facts
        let facts = vec![test_fact(1, "user.name"), test_fact(2, "user.email")];

        let changed = view.apply_facts(&facts);
        assert!(changed);
        assert_eq!(view.get_state(), TestCounter(2));

        // Apply more facts (join with existing)
        let more_facts = vec![test_fact(3, "event.type")];

        let changed = view.apply_facts(&more_facts);
        // Should not change since max(2, 1) = 2
        assert!(!changed);
        assert_eq!(view.get_state(), TestCounter(2));

        // Apply even more facts
        let even_more = vec![test_fact(4, "a"), test_fact(5, "b"), test_fact(6, "c")];

        let changed = view.apply_facts(&even_more);
        assert!(changed);
        assert_eq!(view.get_state(), TestCounter(3));
    }

    #[test]
    fn test_materialized_view_reset() {
        let view = MaterializedView::with_state(
            "test.query".to_string(),
            test_authority(),
            TestCounter(100),
        );

        assert_eq!(view.get_state(), TestCounter(100));

        view.reset();

        assert_eq!(view.get_state(), TestCounter(0));
    }

    #[test]
    fn test_materialized_view_idempotent() {
        let view = MaterializedView::<TestCounter>::new("test.query".to_string(), test_authority());

        let facts = vec![test_fact(1, "test")];

        // First application
        let changed1 = view.apply_facts(&facts);
        assert!(changed1);
        assert_eq!(view.get_state(), TestCounter(1));

        // Same facts again - idempotent
        let changed2 = view.apply_facts(&facts);
        assert!(!changed2);
        assert_eq!(view.get_state(), TestCounter(1));
    }

    #[test]
    fn test_materialized_view_poll_subscribe() {
        let view = MaterializedView::<TestCounter>::new("test.query".to_string(), test_authority());

        let dynamic = view.subscribe();
        let mut sub = dynamic.subscribe();

        // Apply facts
        let facts = vec![test_fact(1, "a"), test_fact(2, "b"), test_fact(3, "c")];
        view.apply_facts(&facts);

        // Should receive update via polling
        let received = sub.poll().expect("Expected an update");
        assert_eq!(received, TestCounter(3));
    }

    #[test]
    fn test_materialized_view_debug() {
        let view = MaterializedView::with_state(
            "test.query".to_string(),
            test_authority(),
            TestCounter(42),
        );

        let debug_str = format!("{:?}", view);
        assert!(debug_str.contains("MaterializedView"));
        assert!(debug_str.contains("test.query"));
        assert!(debug_str.contains("42"));
    }
}
