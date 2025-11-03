//! Core middleware traits and interfaces

use super::{MiddlewareContext, MiddlewareResult, HandlerMetadata};
use crate::effects::AuraEffects;
use std::future::Future;
use std::pin::Pin;

/// Core middleware handler trait
pub trait MiddlewareHandler<Req, Resp, Err>: Send + Sync 
where
    Req: Send + Sync,
    Resp: Send + Sync,
    Err: std::error::Error + Send + Sync + 'static,
{
    /// Handle a request and produce a response
    fn handle(
        &self,
        request: Req,
        context: &MiddlewareContext,
        effects: &dyn AuraEffects,
    ) -> Pin<Box<dyn Future<Output = Result<Resp, Err>> + Send + '_>>;

    /// Get handler metadata
    fn metadata(&self) -> HandlerMetadata {
        HandlerMetadata::default()
    }
}

/// Protocol-specific handler trait
pub trait ProtocolHandler: Send + Sync {
    /// The type of requests this handler processes
    type Request: Send + Sync;
    
    /// The type of responses this handler produces
    type Response: Send + Sync;
    
    /// The type of errors this handler can produce
    type Error: std::error::Error + Send + Sync + 'static;

    /// Handle a protocol-specific request
    fn handle(
        &mut self,
        request: Self::Request,
        effects: &dyn AuraEffects,
    ) -> Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send + '_>>;

    /// Get the protocol name this handler supports
    fn protocol_name(&self) -> &str;

    /// Get handler metadata
    fn metadata(&self) -> HandlerMetadata {
        HandlerMetadata::default()
    }

    /// Initialize the handler
    fn initialize(&mut self, _effects: &dyn AuraEffects) -> Result<(), Self::Error> {
        Ok(())
    }

    /// Shutdown the handler
    fn shutdown(&mut self, _effects: &dyn AuraEffects) -> Result<(), Self::Error> {
        Ok(())
    }
}

/// Request preprocessing trait
pub trait RequestHandler<Req>: Send + Sync 
where
    Req: Send + Sync,
{
    /// The type of errors this handler can produce
    type Error: std::error::Error + Send + Sync + 'static;

    /// Process an incoming request before it reaches the main handler
    fn process_request(
        &self,
        request: Req,
        context: &MiddlewareContext,
        effects: &dyn AuraEffects,
    ) -> Pin<Box<dyn Future<Output = Result<Req, Self::Error>> + Send + '_>>;
}

/// Response postprocessing trait
pub trait ResponseHandler<Resp>: Send + Sync 
where
    Resp: Send + Sync,
{
    /// The type of errors this handler can produce
    type Error: std::error::Error + Send + Sync + 'static;

    /// Process an outgoing response after it's produced by the main handler
    fn process_response(
        &self,
        response: Resp,
        context: &MiddlewareContext,
        effects: &dyn AuraEffects,
    ) -> Pin<Box<dyn Future<Output = Result<Resp, Self::Error>> + Send + '_>>;
}

/// Bidirectional middleware trait that can process both requests and responses
pub trait BidirectionalHandler<Req, Resp>: Send + Sync 
where
    Req: Send + Sync,
    Resp: Send + Sync,
{
    /// The type of errors this handler can produce
    type Error: std::error::Error + Send + Sync + 'static;

    /// Process an incoming request
    fn process_request(
        &self,
        request: Req,
        context: &MiddlewareContext,
        effects: &dyn AuraEffects,
    ) -> Pin<Box<dyn Future<Output = Result<Req, Self::Error>> + Send + '_>>;

    /// Process an outgoing response
    fn process_response(
        &self,
        response: Resp,
        context: &MiddlewareContext,
        effects: &dyn AuraEffects,
    ) -> Pin<Box<dyn Future<Output = Result<Resp, Self::Error>> + Send + '_>>;
}

/// Stateful middleware trait for handlers that maintain state
pub trait StatefulHandler: Send + Sync {
    /// The type of state this handler maintains
    type State: Send + Sync;
    
    /// The type of errors this handler can produce
    type Error: std::error::Error + Send + Sync + 'static;

    /// Get immutable reference to the handler's state
    fn state(&self) -> &Self::State;

    /// Get mutable reference to the handler's state
    fn state_mut(&mut self) -> &mut Self::State;

    /// Update the handler's state
    fn update_state<F>(&mut self, updater: F) -> Result<(), Self::Error>
    where
        F: FnOnce(&mut Self::State) -> Result<(), Self::Error>;

    /// Reset the handler's state to its initial value
    fn reset_state(&mut self) -> Result<(), Self::Error>;

    /// Save the handler's state
    fn save_state(&self, effects: &dyn AuraEffects) -> Pin<Box<dyn Future<Output = Result<(), Self::Error>> + Send + '_>>;

    /// Load the handler's state
    fn load_state(&mut self, effects: &dyn AuraEffects) -> Pin<Box<dyn Future<Output = Result<(), Self::Error>> + Send + '_>>;
}

/// Configurable middleware trait for handlers with runtime configuration
pub trait ConfigurableHandler: Send + Sync {
    /// The type of configuration this handler uses
    type Config: Send + Sync;
    
    /// The type of errors this handler can produce
    type Error: std::error::Error + Send + Sync + 'static;

    /// Get the current configuration
    fn config(&self) -> &Self::Config;

    /// Update the configuration
    fn update_config(&mut self, config: Self::Config) -> Result<(), Self::Error>;

    /// Validate a configuration before applying it
    fn validate_config(config: &Self::Config) -> Result<(), Self::Error>;

    /// Get the default configuration
    fn default_config() -> Self::Config;
}

/// Lifecycle-aware middleware trait
pub trait LifecycleHandler: Send + Sync {
    /// The type of errors this handler can produce
    type Error: std::error::Error + Send + Sync + 'static;

    /// Called when the handler is first created
    fn on_create(&mut self, context: &MiddlewareContext, effects: &dyn AuraEffects) -> Result<(), Self::Error> {
        Ok(())
    }

    /// Called when the handler starts processing requests
    fn on_start(&mut self, context: &MiddlewareContext, effects: &dyn AuraEffects) -> Result<(), Self::Error> {
        Ok(())
    }

    /// Called when the handler stops processing requests
    fn on_stop(&mut self, context: &MiddlewareContext, effects: &dyn AuraEffects) -> Result<(), Self::Error> {
        Ok(())
    }

    /// Called when the handler is being destroyed
    fn on_destroy(&mut self, context: &MiddlewareContext, effects: &dyn AuraEffects) -> Result<(), Self::Error> {
        Ok(())
    }

    /// Called on configuration changes
    fn on_config_change(&mut self, context: &MiddlewareContext, effects: &dyn AuraEffects) -> Result<(), Self::Error> {
        Ok(())
    }

    /// Called on error conditions
    fn on_error(&mut self, error: &dyn std::error::Error, context: &MiddlewareContext, effects: &dyn AuraEffects) -> Result<(), Self::Error> {
        Ok(())
    }
}

/// Metrics-aware middleware trait
pub trait MetricsHandler: Send + Sync {
    /// Record a metric event
    fn record_metric(&self, name: &str, value: f64, labels: &[(&str, &str)], effects: &dyn AuraEffects);

    /// Record a timing metric
    fn record_timing(&self, name: &str, duration: std::time::Duration, labels: &[(&str, &str)], effects: &dyn AuraEffects) {
        self.record_metric(name, duration.as_secs_f64(), labels, effects);
    }

    /// Record a counter metric
    fn record_counter(&self, name: &str, count: u64, labels: &[(&str, &str)], effects: &dyn AuraEffects) {
        self.record_metric(name, count as f64, labels, effects);
    }

    /// Record a gauge metric
    fn record_gauge(&self, name: &str, value: f64, labels: &[(&str, &str)], effects: &dyn AuraEffects) {
        self.record_metric(name, value, labels, effects);
    }
}

/// Health check trait for middleware
pub trait HealthCheckHandler: Send + Sync {
    /// The type of health check result
    type HealthStatus: Send + Sync;
    
    /// The type of errors this handler can produce
    type Error: std::error::Error + Send + Sync + 'static;

    /// Perform a health check
    fn health_check(&self, effects: &dyn AuraEffects) -> Pin<Box<dyn Future<Output = Result<Self::HealthStatus, Self::Error>> + Send + '_>>;

    /// Check if the handler is ready to process requests
    fn readiness_check(&self, effects: &dyn AuraEffects) -> Pin<Box<dyn Future<Output = Result<bool, Self::Error>> + Send + '_>>;

    /// Check if the handler is alive
    fn liveness_check(&self, effects: &dyn AuraEffects) -> Pin<Box<dyn Future<Output = Result<bool, Self::Error>> + Send + '_>>;
}

/// Combined trait for handlers that support all middleware features
pub trait FullHandler<Req, Resp, Err>: 
    MiddlewareHandler<Req, Resp, Err> +
    StatefulHandler<Error = Err> +
    ConfigurableHandler<Error = Err> +
    LifecycleHandler<Error = Err> +
    MetricsHandler +
    HealthCheckHandler<Error = Err>
where
    Req: Send + Sync,
    Resp: Send + Sync,
    Err: std::error::Error + Send + Sync + 'static,
{
}

/// Middleware chain trait for composing multiple handlers
pub trait MiddlewareChain<Req, Resp>: Send + Sync 
where
    Req: Send + Sync,
    Resp: Send + Sync,
{
    /// The type of errors this chain can produce
    type Error: std::error::Error + Send + Sync + 'static;

    /// Execute the middleware chain
    fn execute(
        &self,
        request: Req,
        context: &MiddlewareContext,
        effects: &dyn AuraEffects,
    ) -> Pin<Box<dyn Future<Output = Result<Resp, Self::Error>> + Send + '_>>;

    /// Add a handler to the chain
    fn add_handler<H>(&mut self, handler: H) -> Result<(), Self::Error>
    where
        H: MiddlewareHandler<Req, Resp, Self::Error> + 'static;

    /// Get the number of handlers in the chain
    fn handler_count(&self) -> usize;

    /// Get metadata for all handlers in the chain
    fn chain_metadata(&self) -> Vec<HandlerMetadata>;
}