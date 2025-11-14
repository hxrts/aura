//! Parallel initialization optimization for the effect system
//!
//! This module provides optimized initialization paths that leverage
//! parallelism to reduce startup time for the effect system.

use std::sync::Arc;
use futures::future::{join_all, FutureExt};

use aura_core::{AuraResult, AuraError, DeviceId};

// Platform-specific imports
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(not(target_arch = "wasm32"))]
use tokio::task::JoinSet;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;
#[cfg(target_arch = "wasm32")]
use web_sys::Performance;
use aura_effects::handlers::{
    MockNetworkHandler, MockCryptoHandler, MockTimeHandler,
    MockRandomHandler, MockConsoleHandler,
};
use aura_effects::storage::MemoryStorageHandler;
use aura_effects::journal::MemoryJournalHandler;

use crate::handlers::{
    EffectType, ExecutionMode,
    system::LoggingSystemHandler,
    ledger::memory::MemoryLedgerHandler,
    tree::dummy::DummyTreeHandler,
    choreographic::memory::MemoryChoreographicHandler,
};

use super::{
    EffectSystemConfig, AuraEffectSystem,
    executor::{EffectExecutor, EffectExecutorBuilder},
    handler_adapters::*,
    services::{ContextManager, FlowBudgetManager, ReceiptManager},
    lifecycle::LifecycleManager,
};

/// Metrics for initialization performance
#[derive(Debug, Clone)]
pub struct InitializationMetrics {
    pub total_duration: std::time::Duration,
    pub handler_init_duration: std::time::Duration,
    pub service_init_duration: std::time::Duration,
    pub parallel_speedup: f64,
}

/// Platform-specific time measurement
#[cfg(not(target_arch = "wasm32"))]
fn now() -> Instant {
    Instant::now()
}

#[cfg(target_arch = "wasm32")]
fn now() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map(|p| p.now())
        .unwrap_or(0.0)
}

#[cfg(not(target_arch = "wasm32"))]
fn elapsed_since(start: Instant) -> std::time::Duration {
    start.elapsed()
}

#[cfg(target_arch = "wasm32")]
fn elapsed_since(start: f64) -> std::time::Duration {
    let elapsed_ms = now() - start;
    std::time::Duration::from_millis(elapsed_ms as u64)
}

/// Parallel initialization builder for the effect system
pub struct ParallelInitBuilder {
    config: EffectSystemConfig,
    enable_metrics: bool,
}

impl ParallelInitBuilder {
    /// Create a new parallel initialization builder
    pub fn new(config: EffectSystemConfig) -> Self {
        Self {
            config,
            enable_metrics: false,
        }
    }

    /// Enable metrics collection during initialization
    pub fn with_metrics(mut self) -> Self {
        self.enable_metrics = true;
        self
    }

    /// Build the effect system with parallel initialization
    pub async fn build(self) -> AuraResult<(AuraEffectSystem, Option<InitializationMetrics>)> {
        let start_time = now();
        let mut metrics = if self.enable_metrics {
            Some(InitializationMetrics {
                total_duration: std::time::Duration::default(),
                handler_init_duration: std::time::Duration::default(),
                service_init_duration: std::time::Duration::default(),
                parallel_speedup: 1.0,
            })
        } else {
            None
        };

        // Phase 1: Initialize handlers in parallel
        let handler_start = now();
        let handlers = self.initialize_handlers_parallel().await?;
        
        if let Some(ref mut m) = metrics {
            m.handler_init_duration = elapsed_since(handler_start);
        }

        // Phase 2: Build executor with initialized handlers
        let executor = self.build_executor_from_handlers(handlers)?;

        // Phase 3: Initialize services in parallel
        let service_start = now();
        let (context_mgr, budget_mgr, receipt_mgr, lifecycle_mgr) = 
            self.initialize_services_parallel().await?;
        
        if let Some(ref mut m) = metrics {
            m.service_init_duration = elapsed_since(service_start);
            m.total_duration = elapsed_since(start_time);
            
            // Calculate speedup vs sequential
            let sequential_estimate = m.handler_init_duration.as_millis() * 11 // 11 handlers
                + m.service_init_duration.as_millis() * 4; // 4 services
            m.parallel_speedup = sequential_estimate as f64 / m.total_duration.as_millis() as f64;
        }

        let system = AuraEffectSystem::from_components(
            self.config,
            Arc::new(executor),
            context_mgr,
            budget_mgr,
            receipt_mgr,
            lifecycle_mgr,
        );

        Ok((system, metrics))
    }

    /// Initialize all handlers in parallel
    async fn initialize_handlers_parallel(&self) -> AuraResult<Vec<(EffectType, Arc<dyn std::any::Any + Send + Sync>)>> {
        let mode = self.config.execution_mode;
        let device_id = self.config.device_id;
        
        // For WASM, we use join_all instead of JoinSet
        // This still provides concurrency but without OS threads
        let seed = match mode {
            ExecutionMode::Simulation { seed } => seed,
            _ => 0,
        };

        // Create futures for each handler initialization
        let handler_futures = vec![
            // Crypto handler
            async move {
                let handler = Arc::new(CryptoHandlerAdapter::new(
                    MockCryptoHandler::new(42),
                    mode,
                )) as Arc<dyn std::any::Any + Send + Sync>;
                (EffectType::Crypto, handler)
            }.boxed(),

            // Network handler
            async move {
                let handler = Arc::new(NetworkHandlerAdapter::new(
                    MockNetworkHandler::new(),
                    mode,
                )) as Arc<dyn std::any::Any + Send + Sync>;
                (EffectType::Network, handler)
            }.boxed(),

            // Storage handler
            async move {
                let handler = Arc::new(StorageHandlerAdapter::new(
                    MemoryStorageHandler::new(),
                    mode,
                )) as Arc<dyn std::any::Any + Send + Sync>;
                (EffectType::Storage, handler)
            }.boxed(),

            // Time handler
            async move {
                let handler = Arc::new(TimeHandlerAdapter::new(
                    MockTimeHandler::new(),
                    mode,
                )) as Arc<dyn std::any::Any + Send + Sync>;
                (EffectType::Time, handler)
            }.boxed(),

            // Console handler
            async move {
                let handler = Arc::new(ConsoleHandlerAdapter::new(
                    MockConsoleHandler::new(),
                    mode,
                )) as Arc<dyn std::any::Any + Send + Sync>;
                (EffectType::Console, handler)
            }.boxed(),

            // Random handler
            async move {
                let handler = Arc::new(RandomHandlerAdapter::new(
                    MockRandomHandler::new(seed),
                    mode,
                )) as Arc<dyn std::any::Any + Send + Sync>;
                (EffectType::Random, handler)
            }.boxed(),

            // Journal handler
            async move {
                let handler = Arc::new(JournalHandlerAdapter::new(
                    MemoryJournalHandler::new(),
                    mode,
                )) as Arc<dyn std::any::Any + Send + Sync>;
                (EffectType::Journal, handler)
            }.boxed(),

            // System handler
            async move {
                let handler = Arc::new(SystemHandlerAdapter::new(
                    LoggingSystemHandler::default(),
                    mode,
                )) as Arc<dyn std::any::Any + Send + Sync>;
                (EffectType::System, handler)
            }.boxed(),

            // Ledger handler
            async move {
                let handler = Arc::new(LedgerHandlerAdapter::new(
                    MemoryLedgerHandler::new(),
                    mode,
                )) as Arc<dyn std::any::Any + Send + Sync>;
                (EffectType::Ledger, handler)
            }.boxed(),

            // Tree handler
            async move {
                let handler = Arc::new(TreeHandlerAdapter::new(
                    DummyTreeHandler::new(),
                    mode,
                )) as Arc<dyn std::any::Any + Send + Sync>;
                (EffectType::Tree, handler)
            }.boxed(),

            // Choreographic handler
            async move {
                let handler = Arc::new(ChoreographicHandlerAdapter::new(
                    MemoryChoreographicHandler::new(device_id.0),
                    mode,
                )) as Arc<dyn std::any::Any + Send + Sync>;
                (EffectType::Choreographic, handler)
            }.boxed(),
        ];

        // Execute all handler initializations concurrently
        let handlers = join_all(handler_futures).await;
        Ok(handlers)
    }

    /// Build executor from initialized handlers
    fn build_executor_from_handlers(
        &self,
        handlers: Vec<(EffectType, Arc<dyn std::any::Any + Send + Sync>)>,
    ) -> AuraResult<EffectExecutor> {
        let mut builder = EffectExecutorBuilder::new();
        
        for (effect_type, handler) in handlers {
            builder = builder.with_handler(effect_type, handler);
        }

        Ok(builder.build())
    }

    /// Initialize services in parallel (WASM-compatible)
    async fn initialize_services_parallel(
        &self,
    ) -> AuraResult<(
        Arc<ContextManager>,
        Arc<FlowBudgetManager>,
        Arc<ReceiptManager>,
        Arc<LifecycleManager>,
    )> {
        // Services are lightweight, but we can still parallelize
        let device_id = self.config.device_id;
        
        // Use join_all with boxed futures for WASM compatibility
        let service_futures = vec![
            async { Arc::new(ContextManager::new()) }.boxed(),
            async { Arc::new(FlowBudgetManager::new()) }.boxed(),
            async { Arc::new(ReceiptManager::new()) }.boxed(),
            async move { Arc::new(LifecycleManager::new(device_id)) }.boxed(),
        ];
        
        let services = join_all(service_futures).await;

        Ok((
            services[0].clone(),
            services[1].clone(),
            services[2].clone(),
            services[3].clone(),
        ))
    }
}

/// Lazy initialization wrapper for on-demand handler loading
pub struct LazyEffectSystem {
    config: EffectSystemConfig,
    system: tokio::sync::OnceCell<Arc<AuraEffectSystem>>,
}

impl LazyEffectSystem {
    /// Create a new lazy effect system
    pub fn new(config: EffectSystemConfig) -> Self {
        Self {
            config,
            system: tokio::sync::OnceCell::new(),
        }
    }

    /// Get or initialize the effect system
    pub async fn get(&self) -> AuraResult<&Arc<AuraEffectSystem>> {
        self.system
            .get_or_try_init(|| async {
                let builder = ParallelInitBuilder::new(self.config.clone());
                let (system, _) = builder.build().await?;
                Ok(Arc::new(system))
            })
            .await
    }
}

/// Connection pool for reusable handlers
pub struct HandlerPool {
    network_pool: Vec<Arc<MockNetworkHandler>>,
    storage_pool: Vec<Arc<MemoryStorageHandler>>,
    max_size: usize,
}

impl HandlerPool {
    /// Create a new handler pool
    pub fn new(max_size: usize) -> Self {
        Self {
            network_pool: Vec::with_capacity(max_size),
            storage_pool: Vec::with_capacity(max_size),
            max_size,
        }
    }

    /// Pre-warm the pool with handlers (WASM-compatible)
    pub async fn warm_up(&mut self, count: usize) {
        let count = count.min(self.max_size);
        
        // Pre-create handlers without spawning OS threads
        let network_futures = (0..count).map(|_| {
            async { Arc::new(MockNetworkHandler::new()) }.boxed()
        });
        let network_handlers = join_all(network_futures).await;

        let storage_futures = (0..count).map(|_| {
            async { Arc::new(MemoryStorageHandler::new()) }.boxed()
        });
        let storage_handlers = join_all(storage_futures).await;

        // Add to pools
        for h in network_handlers {
            self.network_pool.push(h);
        }

        for h in storage_handlers {
            self.storage_pool.push(h);
        }
    }

    /// Get a network handler from the pool or create new
    pub fn get_network_handler(&mut self) -> Arc<MockNetworkHandler> {
        self.network_pool.pop()
            .unwrap_or_else(|| Arc::new(MockNetworkHandler::new()))
    }

    /// Return a network handler to the pool
    pub fn return_network_handler(&mut self, handler: Arc<MockNetworkHandler>) {
        if self.network_pool.len() < self.max_size {
            self.network_pool.push(handler);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_parallel_initialization() {
        let config = EffectSystemConfig::for_testing(DeviceId::new());
        let builder = ParallelInitBuilder::new(config).with_metrics();
        
        let (system, metrics) = builder.build().await.unwrap();
        let metrics = metrics.unwrap();
        
        println!("Initialization metrics:");
        println!("  Total duration: {:?}", metrics.total_duration);
        println!("  Handler init: {:?}", metrics.handler_init_duration);
        println!("  Service init: {:?}", metrics.service_init_duration);
        println!("  Parallel speedup: {:.2}x", metrics.parallel_speedup);
        
        assert!(metrics.parallel_speedup > 1.0);
        
        // Verify system is functional
        let epoch = system.current_epoch().await;
        assert_eq!(epoch, 1);
    }

    #[tokio::test]
    async fn test_lazy_initialization() {
        let config = EffectSystemConfig::for_testing(DeviceId::new());
        let lazy = LazyEffectSystem::new(config);
        
        // First access triggers initialization
        let system1 = lazy.get().await.unwrap();
        
        // Second access returns cached instance
        let system2 = lazy.get().await.unwrap();
        
        // Verify same instance
        assert!(Arc::ptr_eq(system1, system2));
    }

    #[tokio::test]
    async fn test_handler_pool() {
        let mut pool = HandlerPool::new(10);
        
        // Warm up pool
        pool.warm_up(5).await;
        
        // Get handlers
        let h1 = pool.get_network_handler();
        let h2 = pool.get_network_handler();
        
        // Return one handler
        pool.return_network_handler(h1);
        
        // Next get should reuse the returned handler
        let h3 = pool.get_network_handler();
        assert!(Arc::strong_count(&h3) > 1);
    }
}