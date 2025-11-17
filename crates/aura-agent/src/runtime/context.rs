//! Effect execution context for request tracing and propagation
//!
//! This module provides explicit context propagation for effect execution,
//! replacing ambient state with structured context that flows through
//! async operations.

// TODO: Refactor to use TimeEffects. Uses Instant::now() for context timing
// which should be replaced with effect system integration.
#![allow(clippy::disallowed_methods)]

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tracing::{Level, Span};
use uuid::Uuid;

use aura_core::{AuraError, AuraResult, DeviceId, FlowBudget};

/// Context that flows through effect execution
#[derive(Debug, Clone)]
pub struct EffectContext {
    /// Unique request ID for tracing
    pub request_id: Uuid,
    /// Device executing the effects
    pub device_id: DeviceId,
    /// Flow budget for this operation
    pub flow_budget: FlowBudget,
    /// Distributed trace context
    pub trace_context: TraceContext,
    /// Optional deadline for the operation
    pub deadline: Option<Instant>,
    /// Custom metadata for the request
    pub metadata: HashMap<String, String>,
    /// Parent context for nested operations
    parent: Option<Arc<EffectContext>>,
}

impl EffectContext {
    /// Create a new root context with generated UUIDs
    ///
    /// Note: Callers should use `new_with_ids()` and generate UUIDs via `RandomEffects::random_uuid()`
    /// to avoid direct `Uuid::new_v4()` calls.
    pub fn new(device_id: DeviceId, request_id: Uuid, trace_id: Uuid, span_id: Uuid) -> Self {
        Self {
            request_id,
            device_id,
            flow_budget: FlowBudget::default(),
            trace_context: TraceContext::new_with_ids(trace_id, span_id),
            deadline: None,
            metadata: HashMap::new(),
            parent: None,
        }
    }

    /// Create a new context with a specific request ID
    pub fn with_request_id(mut self, request_id: Uuid) -> Self {
        self.request_id = request_id;
        self
    }

    /// Set the flow budget
    pub fn with_flow_budget(mut self, budget: FlowBudget) -> Self {
        self.flow_budget = budget;
        self
    }

    /// Set a deadline for the operation
    pub fn with_deadline(mut self, deadline: Instant) -> Self {
        self.deadline = Some(deadline);
        self
    }

    /// Add metadata to the context
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Create a child context for nested operations
    ///
    /// Note: Callers should use `child_with_ids()` and generate UUIDs via `RandomEffects::random_uuid()`
    /// to avoid direct `Uuid::new_v4()` calls.
    pub fn child(&self, request_id: Uuid, span_id: Uuid) -> Self {
        let mut child = self.clone();
        child.parent = Some(Arc::new(self.clone()));
        child.request_id = request_id; // New request ID for child
        child.trace_context = self.trace_context.child_with_id(span_id);
        child
    }

    /// Check if the deadline has been exceeded
    pub fn is_deadline_exceeded(&self, now: Instant) -> bool {
        if let Some(deadline) = self.deadline {
            now > deadline
        } else {
            false
        }
    }

    /// Get remaining time until deadline
    pub fn time_until_deadline(&self, now: Instant) -> Option<Duration> {
        self.deadline
            .map(|d| d.saturating_duration_since(now))
    }

    /// Charge flow budget
    pub fn charge_flow(&mut self, amount: u64) -> AuraResult<()> {
        if self.flow_budget.remaining() >= amount {
            self.flow_budget = self.flow_budget.consume(amount)?;
            Ok(())
        } else {
            Err(AuraError::invalid("Insufficient flow budget"))
        }
    }

    /// Get the root context by traversing parents
    pub fn root(&self) -> EffectContext {
        let mut current = self.clone();
        while let Some(parent) = &current.parent {
            current = (**parent).clone();
        }
        current
    }

    /// Create a tracing span for this context
    pub fn span(&self) -> Span {
        let span = tracing::span!(
            Level::INFO,
            "effect",
            request_id = %self.request_id,
            device_id = %self.device_id.0,
            flow_budget = self.flow_budget.remaining(),
        );

        // Add trace parent if available
        if let Some(trace_parent) = &self.trace_context.trace_parent {
            span.record("trace_parent", &trace_parent.as_str());
        }

        span
    }
}

/// Distributed tracing context
#[derive(Debug, Clone)]
pub struct TraceContext {
    /// W3C Trace Context trace-parent header
    pub trace_parent: Option<String>,
    /// W3C Trace Context trace-state header
    pub trace_state: Option<String>,
    /// Trace ID
    pub trace_id: Uuid,
    /// Span ID
    pub span_id: Uuid,
    /// Whether sampling is enabled
    pub sampled: bool,
}

impl TraceContext {
    /// Create a new trace context with provided IDs
    ///
    /// Note: Callers should generate UUIDs via `RandomEffects::random_uuid()`
    /// to avoid direct `Uuid::new_v4()` calls.
    pub fn new_with_ids(trace_id: Uuid, span_id: Uuid) -> Self {
        Self {
            trace_parent: None,
            trace_state: None,
            trace_id,
            span_id,
            sampled: true,
        }
    }

    /// Create a child trace context with provided span ID
    ///
    /// Note: Callers should generate UUIDs via `RandomEffects::random_uuid()`
    /// to avoid direct `Uuid::new_v4()` calls.
    pub fn child_with_id(&self, span_id: Uuid) -> Self {
        let mut child = self.clone();
        child.span_id = span_id;
        child.update_trace_parent();
        child
    }

    /// Parse from W3C trace-parent header
    pub fn from_trace_parent(header: &str) -> AuraResult<Self> {
        // Simple parsing - full W3C compliance would be more complex
        let parts: Vec<&str> = header.split('-').collect();
        if parts.len() != 4 {
            return Err(AuraError::invalid("Invalid trace-parent format"));
        }

        Ok(Self {
            trace_parent: Some(header.to_string()),
            trace_state: None,
            trace_id: Uuid::parse_str(parts[1])
                .map_err(|_| AuraError::invalid("Invalid trace ID"))?,
            span_id: Uuid::parse_str(parts[2])
                .map_err(|_| AuraError::invalid("Invalid span ID"))?,
            sampled: parts[3] == "01",
        })
    }

    /// Update the trace-parent header
    fn update_trace_parent(&mut self) {
        let flags = if self.sampled { "01" } else { "00" };
        self.trace_parent = Some(format!(
            "00-{}-{}-{}",
            self.trace_id.as_simple(),
            self.span_id.as_simple(),
            flags
        ));
    }

    /// Export as headers for propagation
    pub fn as_headers(&self) -> HashMap<String, String> {
        let mut headers = HashMap::new();

        if let Some(ref trace_parent) = self.trace_parent {
            headers.insert("traceparent".to_string(), trace_parent.clone());
        }

        if let Some(ref trace_state) = self.trace_state {
            headers.insert("tracestate".to_string(), trace_state.clone());
        }

        headers
    }
}

impl Default for TraceContext {
    fn default() -> Self {
        // For Default trait, we need to use a placeholder UUID
        // Callers should use new_with_ids() when possible
        Self::new_with_ids(
            Uuid::from_bytes([0u8; 16]),
            Uuid::from_bytes([0u8; 16]),
        )
    }
}

/// Context-aware wrapper for async operations
pub struct ContextualFuture<F> {
    context: EffectContext,
    future: F,
}

impl<F> ContextualFuture<F> {
    /// Create a new contextual future
    pub fn new(context: EffectContext, future: F) -> Self {
        Self { context, future }
    }
}

impl<F> std::future::Future for ContextualFuture<F>
where
    F: std::future::Future,
{
    type Output = F::Output;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        // Enter the context span
        let span = self.context.span();
        let _guard = span.enter();

        // Poll the inner future
        let this = unsafe { self.get_unchecked_mut() };
        let future = unsafe { std::pin::Pin::new_unchecked(&mut this.future) };
        future.poll(cx)
    }
}

/// Extension trait for adding context to futures
pub trait WithContext: Sized {
    /// Wrap this future with an effect context
    fn with_context(self, context: EffectContext) -> ContextualFuture<Self> {
        ContextualFuture::new(context, self)
    }
}

impl<F: std::future::Future> WithContext for F {}

/// Context provider trait for handlers
pub trait ContextProvider: Send + Sync {
    /// Get the current context
    fn current_context(&self) -> Option<EffectContext>;

    /// Set the current context
    fn set_context(&self, context: EffectContext);

    /// Run with a specific context (for dyn-compatible usage)
    fn with_context_dyn(&self, context: EffectContext, f: Box<dyn FnOnce()>) {
        let previous = self.current_context();
        self.set_context(context);
        f();
        if let Some(prev) = previous {
            self.set_context(prev);
        }
    }
}

/// Extension trait providing generic with_context method
pub trait ContextProviderExt: ContextProvider {
    /// Run with a specific context (generic version)
    fn with_context<R>(&self, context: EffectContext, f: impl FnOnce() -> R) -> R {
        let previous = self.current_context();
        self.set_context(context);
        let result = f();
        if let Some(prev) = previous {
            self.set_context(prev);
        }
        result
    }
}

// Automatically implement for all ContextProvider implementors
impl<T: ContextProvider> ContextProviderExt for T {}

/// Thread-local context storage
pub mod thread_local {
    use super::*;
    use std::cell::RefCell;

    thread_local! {
        static CURRENT_CONTEXT: RefCell<Option<EffectContext>> = RefCell::new(None);
    }

    /// Get the current thread-local context
    pub fn current() -> Option<EffectContext> {
        CURRENT_CONTEXT.with(|c| c.borrow().clone())
    }

    /// Set the current thread-local context
    pub fn set(context: EffectContext) {
        CURRENT_CONTEXT.with(|c| {
            *c.borrow_mut() = Some(context);
        });
    }

    /// Clear the current thread-local context
    pub fn clear() {
        CURRENT_CONTEXT.with(|c| {
            *c.borrow_mut() = None;
        });
    }

    /// Run with a specific context
    pub fn with_context<R>(context: EffectContext, f: impl FnOnce() -> R) -> R {
        let previous = current();
        set(context);
        let result = f();
        match previous {
            Some(prev) => set(prev),
            None => clear(),
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_creation() {
        let device_id = DeviceId::new();
        #[allow(clippy::disallowed_methods)]
        // Test code - UUID generation acceptable for testing
        let context = EffectContext::new(
            device_id,
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
        )
        .with_flow_budget(FlowBudget::new(1000))
        .with_metadata("test", "value");

        assert_eq!(context.device_id, device_id);
        assert_eq!(context.flow_budget.remaining(), 1000);
        assert_eq!(context.metadata.get("test"), Some(&"value".to_string()));
    }

    #[test]
    fn test_context_hierarchy() {
        #[allow(clippy::disallowed_methods)]
        // Test code - UUID generation acceptable for testing
        let root = EffectContext::new(
            DeviceId::new(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
        );
        #[allow(clippy::disallowed_methods)]
        // Test code - UUID generation acceptable for testing
        let child = root.child(Uuid::new_v4(), Uuid::new_v4());

        assert_ne!(root.request_id, child.request_id);
        assert_eq!(root.device_id, child.device_id);
        assert!(child.parent.is_some());
    }

    #[test]
    fn test_flow_budget_charging() {
        #[allow(clippy::disallowed_methods)]
        // Test code - UUID generation acceptable for testing
        let mut context = EffectContext::new(
            DeviceId::new(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
        )
        .with_flow_budget(FlowBudget::new(100));

        assert!(context.charge_flow(50).is_ok());
        assert_eq!(context.flow_budget.remaining(), 50);

        assert!(context.charge_flow(60).is_err());
        assert_eq!(context.flow_budget.remaining(), 50);
    }

    #[test]
    fn test_deadline_checking() {
        #[allow(clippy::disallowed_methods)]
        // Test code - UUID generation acceptable for testing
        let now = Instant::now();
        let context = EffectContext::new(
            DeviceId::new(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
        )
        .with_deadline(now + Duration::from_secs(1));

        assert!(!context.is_deadline_exceeded(now));
        assert!(context.time_until_deadline(now).is_some());
    }

    #[test]
    fn test_trace_context() {
        #[allow(clippy::disallowed_methods)]
        // Test code - UUID generation acceptable for testing
        let trace = TraceContext::new_with_ids(Uuid::new_v4(), Uuid::new_v4());
        #[allow(clippy::disallowed_methods)]
        // Test code - UUID generation acceptable for testing
        let child = trace.child_with_id(Uuid::new_v4());

        assert_eq!(trace.trace_id, child.trace_id);
        assert_ne!(trace.span_id, child.span_id);
    }
}
