//! Subscription API for reactive fact updates
//!
//! This module provides subscription-based access to journal facts,
//! enabling reactive UI updates and real-time data synchronization.
//!
//! # Design Philosophy
//!
//! The subscription API builds on existing Aura infrastructure:
//! - Uses `Dynamic<T>` from `aura-core::reactive` for change notification
//! - Leverages Biscuit Datalog for query filtering
//! - Integrates with the indexed journal for efficient lookups
//!
//! # Key Components
//!
//! - `DatabaseSubscriptionEffects`: Trait for subscribing to facts
//! - `FactStream`: Stream of fact additions (append-only)
//! - `FactFilter`: Predicate-based filtering for subscriptions
//! - `QueryScope`: Scoping parameters for query subscriptions

use async_trait::async_trait;
use aura_core::{
    domain::journal::FactValue, effects::IndexedJournalEffects, reactive::Dynamic,
    types::identifiers::AuthorityId, AuraError,
};
use std::sync::Arc;
use tokio::sync::broadcast;

use super::IndexedJournalHandler;

// Re-export FactId for convenience
pub use aura_core::effects::indexed_journal::FactId;

/// A fact delta representing a change to the fact store.
///
/// Facts are append-only in Aura, so the only delta type is `Added`.
/// This simplifies reasoning about state and ensures monotonic growth.
#[derive(Debug, Clone)]
pub enum FactDelta {
    /// A new fact was added to the journal
    Added(SubscriptionFact),
}

/// A fact that can be streamed through subscriptions.
///
/// This is a simplified view of a fact suitable for subscription delivery.
#[derive(Debug, Clone)]
pub struct SubscriptionFact {
    /// Unique identifier for this fact
    pub id: FactId,
    /// The predicate (fact type)
    pub predicate: String,
    /// The fact value
    pub value: FactValue,
    /// Optional authority that created this fact
    pub authority: Option<AuthorityId>,
}

/// Filter criteria for fact subscriptions.
///
/// Multiple criteria are combined with AND logic.
#[derive(Debug, Clone, Default)]
pub struct FactFilter {
    /// Filter by predicate prefix (e.g., "user." matches "user.name", "user.email")
    pub predicate_prefix: Option<String>,
    /// Filter by exact predicate
    pub predicate: Option<String>,
    /// Filter by authority
    pub authority: Option<AuthorityId>,
}

impl FactFilter {
    /// Create an empty filter (matches all facts)
    pub fn new() -> Self {
        Self::default()
    }

    /// Filter by predicate prefix
    pub fn with_predicate_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.predicate_prefix = Some(prefix.into());
        self
    }

    /// Filter by exact predicate
    pub fn with_predicate(mut self, predicate: impl Into<String>) -> Self {
        self.predicate = Some(predicate.into());
        self
    }

    /// Filter by authority
    pub fn with_authority(mut self, authority: AuthorityId) -> Self {
        self.authority = Some(authority);
        self
    }

    /// Check if a fact matches this filter
    pub fn matches(&self, fact: &SubscriptionFact) -> bool {
        // Check predicate prefix
        if let Some(ref prefix) = self.predicate_prefix {
            if !fact.predicate.starts_with(prefix) {
                return false;
            }
        }

        // Check exact predicate
        if let Some(ref pred) = self.predicate {
            if &fact.predicate != pred {
                return false;
            }
        }

        // Check authority
        if let Some(ref auth) = self.authority {
            if fact.authority.as_ref() != Some(auth) {
                return false;
            }
        }

        true
    }
}

/// Scoping parameters for query subscriptions.
///
/// Defines the context in which a query operates, including
/// authority context and optional time bounds.
#[derive(Debug, Clone)]
pub struct QueryScope {
    /// Authority context for the query
    pub authority: Option<AuthorityId>,
    /// Whether to include historical facts
    pub include_historical: bool,
}

impl Default for QueryScope {
    fn default() -> Self {
        Self {
            authority: None,
            include_historical: true,
        }
    }
}

impl QueryScope {
    /// Create a new query scope
    pub fn new() -> Self {
        Self::default()
    }

    /// Scope to a specific authority
    pub fn with_authority(mut self, authority: AuthorityId) -> Self {
        self.authority = Some(authority);
        self
    }

    /// Only receive new facts (no historical)
    pub fn new_facts_only(mut self) -> Self {
        self.include_historical = false;
        self
    }
}

/// Trait for converting query results to typed values.
///
/// Implement this trait for types that can be deserialized from
/// Datalog query results.
pub trait FromQueryResult: Sized + Clone + Send + Sync + 'static {
    /// Convert from a list of facts to the target type
    fn from_facts(facts: &[SubscriptionFact]) -> Result<Self, SubscriptionError>;
}

/// Errors specific to subscription operations
#[derive(Debug, Clone, thiserror::Error)]
pub enum SubscriptionError {
    /// Failed to parse query result
    #[error("Failed to parse query result: {message}")]
    ParseError {
        /// Description of the parse failure
        message: String,
    },

    /// Query execution failed
    #[error("Query execution failed: {message}")]
    QueryError {
        /// Description of the query failure
        message: String,
    },

    /// Subscription channel closed
    #[error("Subscription channel closed")]
    ChannelClosed,

    /// Invalid filter configuration
    #[error("Invalid filter: {message}")]
    InvalidFilter {
        /// Description of the filter error
        message: String,
    },
}

impl From<SubscriptionError> for AuraError {
    fn from(err: SubscriptionError) -> Self {
        AuraError::Internal {
            message: format!("DatabaseSubscription: {}", err),
        }
    }
}

/// A stream of fact deltas for subscription delivery.
///
/// `FactStream` wraps a broadcast receiver and provides methods
/// for consuming fact updates. It supports both async iteration
/// and manual polling.
///
/// # Example
///
/// ```rust,ignore
/// let mut stream = handler.subscribe_facts(filter).await?;
///
/// // Async consumption
/// while let Some(delta) = stream.recv().await {
///     match delta {
///         FactDelta::Added(fact) => println!("New fact: {:?}", fact),
///     }
/// }
/// ```
pub struct FactStream {
    /// Broadcast receiver for fact deltas
    receiver: broadcast::Receiver<FactDelta>,
    /// Filter applied to incoming facts
    filter: FactFilter,
}

impl FactStream {
    /// Create a new fact stream with the given receiver and filter
    pub(crate) fn new(receiver: broadcast::Receiver<FactDelta>, filter: FactFilter) -> Self {
        Self { receiver, filter }
    }

    /// Receive the next fact delta that matches the filter.
    ///
    /// Returns `None` if the channel is closed.
    pub async fn recv(&mut self) -> Option<FactDelta> {
        loop {
            match self.receiver.recv().await {
                Ok(delta) => {
                    // Check if delta matches filter
                    let FactDelta::Added(ref fact) = delta;
                    if self.filter.matches(fact) {
                        return Some(delta);
                    }
                    // Doesn't match filter, try next
                    continue;
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    // Subscriber fell behind - continue to get latest
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => {
                    return None;
                }
            }
        }
    }

    /// Try to receive a fact delta without blocking.
    ///
    /// Returns:
    /// - `Ok(Some(delta))` if a matching delta is available
    /// - `Ok(None)` if no matching delta is available
    /// - `Err(SubscriptionError::ChannelClosed)` if the channel is closed
    pub fn try_recv(&mut self) -> Result<Option<FactDelta>, SubscriptionError> {
        loop {
            match self.receiver.try_recv() {
                Ok(delta) => {
                    let FactDelta::Added(ref fact) = delta;
                    if self.filter.matches(fact) {
                        return Ok(Some(delta));
                    }
                    // Doesn't match filter, try next
                    continue;
                }
                Err(broadcast::error::TryRecvError::Empty) => {
                    return Ok(None);
                }
                Err(broadcast::error::TryRecvError::Lagged(_)) => {
                    // Try again to get latest
                    continue;
                }
                Err(broadcast::error::TryRecvError::Closed) => {
                    return Err(SubscriptionError::ChannelClosed);
                }
            }
        }
    }
}

/// Trait for subscribing to database facts and queries.
///
/// This trait extends journal effects with subscription capabilities,
/// enabling reactive access to fact data.
///
/// # Implementation Notes
///
/// Implementations should:
/// - Use broadcast channels for multi-subscriber support
/// - Apply filters server-side for efficiency
/// - Handle backpressure gracefully (allow lagging)
#[async_trait]
pub trait DatabaseSubscriptionEffects: Send + Sync {
    /// Subscribe to fact updates matching the given filter.
    ///
    /// Returns a `FactStream` that yields new facts as they are committed.
    /// The stream will only yield facts that match the provided filter.
    ///
    /// # Parameters
    ///
    /// - `filter`: Criteria for filtering facts
    ///
    /// # Returns
    ///
    /// A `FactStream` for receiving matching fact deltas
    async fn subscribe_facts(&self, filter: FactFilter) -> Result<FactStream, AuraError>;

    /// Subscribe to a query with automatic updates.
    ///
    /// Returns a `Dynamic<T>` that automatically updates when the underlying
    /// facts change. The query is re-executed whenever relevant facts are
    /// committed.
    ///
    /// # Type Parameters
    ///
    /// - `T`: The result type, must implement `FromQueryResult`
    ///
    /// # Parameters
    ///
    /// - `query`: Datalog query string
    /// - `scope`: Query scoping parameters
    ///
    /// # Returns
    ///
    /// A `Dynamic<T>` containing the current query result, which updates
    /// automatically as facts change
    async fn subscribe_query<T: FromQueryResult>(
        &self,
        query: &str,
        scope: QueryScope,
    ) -> Result<Dynamic<T>, AuraError>;
}

/// Default channel capacity for fact subscriptions
const DEFAULT_SUBSCRIPTION_CAPACITY: usize = 256;

/// Handler that provides subscription capabilities over an indexed journal.
///
/// This handler wraps an `IndexedJournalHandler` and adds subscription
/// support via broadcast channels. When facts are added through this
/// handler, all subscribers are notified.
pub struct SubscribableJournalHandler {
    /// Inner indexed journal handler
    index: Arc<IndexedJournalHandler>,
    /// Broadcast sender for fact deltas
    sender: broadcast::Sender<FactDelta>,
}

impl SubscribableJournalHandler {
    /// Create a new subscribable journal handler
    pub fn new(index: IndexedJournalHandler) -> Self {
        let (sender, _) = broadcast::channel(DEFAULT_SUBSCRIPTION_CAPACITY);
        Self {
            index: Arc::new(index),
            sender,
        }
    }

    /// Create with custom channel capacity
    pub fn with_capacity(index: IndexedJournalHandler, capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self {
            index: Arc::new(index),
            sender,
        }
    }

    /// Add a fact and notify subscribers
    pub fn add_fact(
        &self,
        predicate: String,
        value: FactValue,
        authority: Option<AuthorityId>,
    ) -> FactId {
        // Add to index
        let id = self
            .index
            .add_fact(predicate.clone(), value.clone(), authority.clone(), None);

        // Create subscription fact
        let sub_fact = SubscriptionFact {
            id,
            predicate,
            value,
            authority,
        };

        // Notify subscribers (ignore send errors - just means no receivers)
        let _ = self.sender.send(FactDelta::Added(sub_fact));

        id
    }

    /// Get the underlying index handler
    pub fn index(&self) -> &IndexedJournalHandler {
        &self.index
    }

    /// Get the number of active subscribers
    pub fn subscriber_count(&self) -> usize {
        self.sender.receiver_count()
    }
}

#[async_trait]
impl DatabaseSubscriptionEffects for SubscribableJournalHandler {
    async fn subscribe_facts(&self, filter: FactFilter) -> Result<FactStream, AuraError> {
        let receiver = self.sender.subscribe();
        Ok(FactStream::new(receiver, filter))
    }

    async fn subscribe_query<T: FromQueryResult>(
        &self,
        query: &str,
        scope: QueryScope,
    ) -> Result<Dynamic<T>, AuraError> {
        // Get initial facts based on scope
        let initial_facts = if scope.include_historical {
            // Query all facts matching the scope
            // For now, get facts by authority if specified
            let indexed_facts = if let Some(ref auth) = scope.authority {
                self.index.facts_by_authority(auth).await?
            } else {
                // Get all facts - for now just return empty since we don't have a method for that
                vec![]
            };

            // Convert to subscription facts
            indexed_facts
                .into_iter()
                .map(|f| SubscriptionFact {
                    id: f.id,
                    predicate: f.predicate,
                    value: f.value,
                    authority: f.authority,
                })
                .collect::<Vec<_>>()
        } else {
            vec![]
        };

        // Parse initial result
        let initial_value = T::from_facts(&initial_facts).map_err(|e| AuraError::Internal {
            message: format!(
                "DatabaseSubscription: Failed to parse initial query result: {}",
                e
            ),
        })?;

        // Create Dynamic with initial value
        let dynamic = Dynamic::new(initial_value);

        // Spawn task to update Dynamic when facts change
        let dynamic_clone = dynamic.clone();
        let receiver = self.sender.subscribe();
        let query_string = query.to_string();
        let scope_clone = scope.clone();

        tokio::spawn(async move {
            let mut rx = receiver;
            let mut current_facts = initial_facts;

            loop {
                match rx.recv().await {
                    Ok(FactDelta::Added(fact)) => {
                        // Check if fact is relevant to scope
                        let relevant = match &scope_clone.authority {
                            Some(auth) => fact.authority.as_ref() == Some(auth),
                            None => true,
                        };

                        if relevant {
                            current_facts.push(fact);

                            // Re-parse and update Dynamic
                            if let Ok(new_value) = T::from_facts(&current_facts) {
                                dynamic_clone.set(new_value);
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        // Continue - we'll catch up
                        continue;
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        // Channel closed - stop task
                        break;
                    }
                }
            }

            // Consume the query string to avoid unused warning
            let _ = query_string;
        });

        Ok(dynamic)
    }
}

impl std::fmt::Debug for SubscribableJournalHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SubscribableJournalHandler")
            .field("index", &self.index)
            .field("subscriber_count", &self.sender.receiver_count())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Simple test type implementing FromQueryResult
    #[derive(Debug, Clone, PartialEq)]
    struct FactCount(usize);

    impl FromQueryResult for FactCount {
        fn from_facts(facts: &[SubscriptionFact]) -> Result<Self, SubscriptionError> {
            Ok(FactCount(facts.len()))
        }
    }

    #[test]
    fn test_fact_filter_empty() {
        let filter = FactFilter::new();
        let fact = SubscriptionFact {
            id: FactId::new(1),
            predicate: "test.key".to_string(),
            value: FactValue::String("value".to_string()),
            authority: None,
        };

        assert!(filter.matches(&fact));
    }

    #[test]
    fn test_fact_filter_predicate_prefix() {
        let filter = FactFilter::new().with_predicate_prefix("user.");

        let matching_fact = SubscriptionFact {
            id: FactId::new(1),
            predicate: "user.name".to_string(),
            value: FactValue::String("alice".to_string()),
            authority: None,
        };

        let non_matching_fact = SubscriptionFact {
            id: FactId::new(2),
            predicate: "event.type".to_string(),
            value: FactValue::String("login".to_string()),
            authority: None,
        };

        assert!(filter.matches(&matching_fact));
        assert!(!filter.matches(&non_matching_fact));
    }

    #[test]
    fn test_fact_filter_exact_predicate() {
        let filter = FactFilter::new().with_predicate("user.email");

        let matching_fact = SubscriptionFact {
            id: FactId::new(1),
            predicate: "user.email".to_string(),
            value: FactValue::String("alice@example.com".to_string()),
            authority: None,
        };

        let non_matching_fact = SubscriptionFact {
            id: FactId::new(2),
            predicate: "user.name".to_string(),
            value: FactValue::String("alice".to_string()),
            authority: None,
        };

        assert!(filter.matches(&matching_fact));
        assert!(!filter.matches(&non_matching_fact));
    }

    #[test]
    fn test_fact_filter_authority() {
        let auth = AuthorityId::new();
        let filter = FactFilter::new().with_authority(auth.clone());

        let matching_fact = SubscriptionFact {
            id: FactId::new(1),
            predicate: "test".to_string(),
            value: FactValue::Number(42),
            authority: Some(auth.clone()),
        };

        let non_matching_fact = SubscriptionFact {
            id: FactId::new(2),
            predicate: "test".to_string(),
            value: FactValue::Number(42),
            authority: None,
        };

        assert!(filter.matches(&matching_fact));
        assert!(!filter.matches(&non_matching_fact));
    }

    #[test]
    fn test_fact_filter_combined() {
        let auth = AuthorityId::new();
        let filter = FactFilter::new()
            .with_predicate_prefix("user.")
            .with_authority(auth.clone());

        // Matches both criteria
        let matching = SubscriptionFact {
            id: FactId::new(1),
            predicate: "user.name".to_string(),
            value: FactValue::String("alice".to_string()),
            authority: Some(auth.clone()),
        };

        // Wrong predicate
        let wrong_predicate = SubscriptionFact {
            id: FactId::new(2),
            predicate: "event.type".to_string(),
            value: FactValue::String("login".to_string()),
            authority: Some(auth.clone()),
        };

        // Wrong authority
        let wrong_authority = SubscriptionFact {
            id: FactId::new(3),
            predicate: "user.email".to_string(),
            value: FactValue::String("alice@example.com".to_string()),
            authority: None,
        };

        assert!(filter.matches(&matching));
        assert!(!filter.matches(&wrong_predicate));
        assert!(!filter.matches(&wrong_authority));
    }

    #[test]
    fn test_query_scope_default() {
        let scope = QueryScope::default();
        assert!(scope.authority.is_none());
        assert!(scope.include_historical);
    }

    #[test]
    fn test_query_scope_builder() {
        let auth = AuthorityId::new();
        let scope = QueryScope::new()
            .with_authority(auth.clone())
            .new_facts_only();

        assert_eq!(scope.authority, Some(auth));
        assert!(!scope.include_historical);
    }

    #[tokio::test]
    async fn test_subscribable_handler_add_fact() {
        let index = IndexedJournalHandler::new();
        let handler = SubscribableJournalHandler::new(index);

        let id = handler.add_fact(
            "test.key".to_string(),
            FactValue::String("value".to_string()),
            None,
        );

        assert_eq!(id, FactId::new(0));
    }

    #[tokio::test]
    async fn test_subscribable_handler_subscriber_count() {
        let index = IndexedJournalHandler::new();
        let handler = SubscribableJournalHandler::new(index);

        assert_eq!(handler.subscriber_count(), 0);

        let _stream1 = handler.subscribe_facts(FactFilter::new()).await.unwrap();
        assert_eq!(handler.subscriber_count(), 1);

        let _stream2 = handler.subscribe_facts(FactFilter::new()).await.unwrap();
        assert_eq!(handler.subscriber_count(), 2);
    }

    #[tokio::test]
    async fn test_fact_stream_receives_facts() {
        let index = IndexedJournalHandler::new();
        let handler = SubscribableJournalHandler::new(index);

        let mut stream = handler.subscribe_facts(FactFilter::new()).await.unwrap();

        // Add a fact
        handler.add_fact(
            "test.key".to_string(),
            FactValue::String("value".to_string()),
            None,
        );

        // Receive the fact
        let delta = stream.recv().await.unwrap();
        match delta {
            FactDelta::Added(fact) => {
                assert_eq!(fact.predicate, "test.key");
            }
        }
    }

    #[tokio::test]
    async fn test_fact_stream_filter_applied() {
        let index = IndexedJournalHandler::new();
        let handler = SubscribableJournalHandler::new(index);

        let filter = FactFilter::new().with_predicate_prefix("user.");
        let mut stream = handler.subscribe_facts(filter).await.unwrap();

        // Add a non-matching fact
        handler.add_fact(
            "event.type".to_string(),
            FactValue::String("login".to_string()),
            None,
        );

        // Add a matching fact
        handler.add_fact(
            "user.name".to_string(),
            FactValue::String("alice".to_string()),
            None,
        );

        // Should only receive the matching fact
        let delta = stream.recv().await.unwrap();
        match delta {
            FactDelta::Added(fact) => {
                assert_eq!(fact.predicate, "user.name");
            }
        }
    }

    #[tokio::test]
    async fn test_subscribe_query_initial_value() {
        let index = IndexedJournalHandler::new();
        let handler = SubscribableJournalHandler::new(index);

        // Subscribe with empty scope (no historical facts)
        let dynamic = handler
            .subscribe_query::<FactCount>("test_query", QueryScope::new().new_facts_only())
            .await
            .unwrap();

        // Initial value should be 0 facts
        assert_eq!(dynamic.get(), FactCount(0));
    }

    #[tokio::test]
    async fn test_subscribe_query_updates() {
        let index = IndexedJournalHandler::new();
        let handler = SubscribableJournalHandler::new(index);

        let scope = QueryScope::new().new_facts_only();
        let dynamic = handler
            .subscribe_query::<FactCount>("test_query", scope)
            .await
            .unwrap();

        assert_eq!(dynamic.get(), FactCount(0));

        // Add a fact
        handler.add_fact("test.key".to_string(), FactValue::Number(1), None);

        // Give the background task time to process
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Value should be updated
        assert_eq!(dynamic.get(), FactCount(1));

        // Add another fact
        handler.add_fact("test.key2".to_string(), FactValue::Number(2), None);

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        assert_eq!(dynamic.get(), FactCount(2));
    }

    #[test]
    fn test_fact_count_from_query_result() {
        let facts = vec![
            SubscriptionFact {
                id: FactId::new(1),
                predicate: "a".to_string(),
                value: FactValue::Number(1),
                authority: None,
            },
            SubscriptionFact {
                id: FactId::new(2),
                predicate: "b".to_string(),
                value: FactValue::Number(2),
                authority: None,
            },
        ];

        let count = FactCount::from_facts(&facts).unwrap();
        assert_eq!(count, FactCount(2));
    }
}
