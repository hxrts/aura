//! BoundSignal: Signal paired with its source Query
//!
//! A BoundSignal represents a reactive signal whose value is derived from
//! executing a query against journal facts. When the query's dependency
//! predicates change, the signal automatically re-evaluates.
//!
//! ## Example
//!
//! ```ignore
//! let contacts_signal = BoundSignal::new(
//!     Signal::new("contacts"),
//!     ContactsQuery::default(),
//! );
//!
//! // Register with effects system
//! reactive.register_bound(&contacts_signal).await?;
//!
//! // Signal will automatically update when contact facts change
//! ```

use aura_core::effects::reactive::Signal;
use aura_core::query::{FactPredicate, Query};
use std::marker::PhantomData;

/// A signal bound to a query for automatic updates.
///
/// The signal's value type `T` must match the query's `Result` type.
/// When facts matching the query's dependencies change, the signal
/// should be re-evaluated.
#[derive(Debug)]
pub struct BoundSignal<Q: Query> {
    /// The reactive signal
    signal: Signal<Q::Result>,
    /// The source query
    query: Q,
    /// Phantom for covariance
    _marker: PhantomData<Q>,
}

impl<Q: Query> BoundSignal<Q> {
    /// Create a new bound signal from a signal and query.
    pub fn new(signal: Signal<Q::Result>, query: Q) -> Self {
        Self {
            signal,
            query,
            _marker: PhantomData,
        }
    }

    /// Create a bound signal with a named signal.
    pub fn with_name(name: &'static str, query: Q) -> Self {
        Self::new(Signal::new(name), query)
    }

    /// Get a reference to the signal.
    pub fn signal(&self) -> &Signal<Q::Result> {
        &self.signal
    }

    /// Get a reference to the query.
    pub fn query(&self) -> &Q {
        &self.query
    }

    /// Get the query's fact dependencies.
    pub fn dependencies(&self) -> Vec<FactPredicate> {
        self.query.dependencies()
    }

    /// Check if this signal should be invalidated by a fact change.
    pub fn is_affected_by(&self, predicate: &FactPredicate) -> bool {
        self.query
            .dependencies()
            .iter()
            .any(|dep| dep.matches(predicate))
    }
}

impl<Q: Query> Clone for BoundSignal<Q>
where
    Q: Clone,
{
    fn clone(&self) -> Self {
        Self {
            signal: self.signal.clone(),
            query: self.query.clone(),
            _marker: PhantomData,
        }
    }
}

/// Extension trait for creating bound signals from queries
pub trait QuerySignalExt: Query {
    /// Create a bound signal for this query with a given signal name.
    fn bind_to_signal(self, name: &'static str) -> BoundSignal<Self>
    where
        Self: Sized,
    {
        BoundSignal::with_name(name, self)
    }
}

impl<Q: Query> QuerySignalExt for Q {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queries::ContactsQuery;

    #[test]
    fn test_bound_signal_creation() {
        let query = ContactsQuery::default();
        let bound = query.bind_to_signal("contacts");

        assert_eq!(bound.signal().id().as_str(), "contacts");
        assert!(!bound.dependencies().is_empty());
    }

    #[test]
    fn test_is_affected_by() {
        let query = ContactsQuery::default();
        let bound = query.bind_to_signal("contacts");

        // Should be affected by contact facts
        let contact_predicate = FactPredicate::new("contact");
        assert!(bound.is_affected_by(&contact_predicate));

        // Should not be affected by unrelated facts
        let unrelated = FactPredicate::new("message");
        assert!(!bound.is_affected_by(&unrelated));
    }
}
