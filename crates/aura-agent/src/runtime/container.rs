//! Dependency injection container for effect handlers
//!
//! This module provides a type-safe dependency injection container
//! for managing effect handlers and their lifecycles. It supports
//! scoped bindings, override capabilities, and test fixtures.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;

use tokio::sync::RwLock;
use tracing::{debug, info};

use aura_core::{AuraError, AuraResult};

use super::lifecycle::LifecycleAware;

/// A type-erased container for storing effect handlers
type AnyHandler = Arc<dyn Any + Send + Sync>;

/// Builder function that creates a handler instance
type HandlerBuilder = Box<dyn Fn() -> AnyHandler + Send + Sync>;

/// Scope for handler bindings
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BindingScope {
    /// Singleton - one instance for the entire application lifetime
    Singleton,
    /// Transient - new instance created for each request
    Transient,
    /// Scoped - instance shared within a specific scope (e.g., test)
    Scoped(u64), // Scope ID
}

/// Binding information for a handler
struct Binding {
    builder: HandlerBuilder,
    scope: BindingScope,
    singleton_instance: Option<AnyHandler>,
}

/// Effect container for dependency injection
pub struct EffectContainer {
    /// Type bindings
    bindings: Arc<RwLock<HashMap<TypeId, Binding>>>,
    /// Scoped instances
    scoped_instances: Arc<RwLock<HashMap<(TypeId, u64), AnyHandler>>>,
    /// Lifecycle-aware components
    lifecycle_components: Arc<RwLock<Vec<(String, Box<dyn LifecycleAware>)>>>,
    /// Current scope ID (for testing)
    current_scope: Arc<RwLock<Option<u64>>>,
}

impl EffectContainer {
    /// Create a new effect container
    pub fn new() -> Self {
        Self {
            bindings: Arc::new(RwLock::new(HashMap::new())),
            scoped_instances: Arc::new(RwLock::new(HashMap::new())),
            lifecycle_components: Arc::new(RwLock::new(Vec::new())),
            current_scope: Arc::new(RwLock::new(None)),
        }
    }

    /// Register a handler type with singleton scope
    pub async fn register_singleton<T>(&self, builder: impl Fn() -> T + Send + Sync + 'static)
    where
        T: Send + Sync + 'static,
    {
        self.register_with_scope(BindingScope::Singleton, builder)
            .await;
    }

    /// Register a handler type with transient scope
    pub async fn register_transient<T>(&self, builder: impl Fn() -> T + Send + Sync + 'static)
    where
        T: Send + Sync + 'static,
    {
        self.register_with_scope(BindingScope::Transient, builder)
            .await;
    }

    /// Register a handler type with specific scope
    pub async fn register_with_scope<T>(
        &self,
        scope: BindingScope,
        builder: impl Fn() -> T + Send + Sync + 'static,
    ) where
        T: Send + Sync + 'static,
    {
        let type_id = TypeId::of::<T>();
        let handler_builder = Box::new(move || Arc::new(builder()) as AnyHandler);

        let binding = Binding {
            builder: handler_builder,
            scope,
            singleton_instance: None,
        };

        let mut bindings = self.bindings.write().await;
        bindings.insert(type_id, binding);

        debug!(
            "Registered handler for type {:?} with scope {:?}",
            std::any::type_name::<T>(),
            scope
        );
    }

    /// Register an instance directly (useful for test overrides)
    pub async fn register_instance<T>(&self, instance: T)
    where
        T: Send + Sync + 'static,
    {
        let type_id = TypeId::of::<T>();
        let handler_instance = Arc::new(instance) as AnyHandler;

        let binding = Binding {
            builder: Box::new({
                let instance = handler_instance.clone();
                move || instance.clone()
            }),
            scope: BindingScope::Singleton,
            singleton_instance: Some(handler_instance),
        };

        let mut bindings = self.bindings.write().await;
        bindings.insert(type_id, binding);

        debug!(
            "Registered instance for type {:?}",
            std::any::type_name::<T>()
        );
    }

    /// Resolve a handler instance
    pub async fn resolve<T>(&self) -> AuraResult<Arc<T>>
    where
        T: Send + Sync + 'static,
    {
        let type_id = TypeId::of::<T>();
        let type_name = std::any::type_name::<T>();

        // Check if binding exists
        let bindings = self.bindings.read().await;
        let binding = bindings.get(&type_id).ok_or_else(|| {
            AuraError::invalid(format!("No binding found for type {}", type_name))
        })?;

        match binding.scope {
            BindingScope::Singleton => {
                // Return existing singleton or create new one
                if let Some(ref instance) = binding.singleton_instance {
                    return instance
                        .clone()
                        .downcast::<T>()
                        .map_err(|_| AuraError::invalid("Type mismatch in singleton instance"));
                }

                // Need to create singleton - drop read lock and acquire write lock
                drop(bindings);
                let mut bindings = self.bindings.write().await;

                // Re-check in case another thread created it
                if let Some(binding) = bindings.get_mut(&type_id) {
                    if let Some(ref instance) = binding.singleton_instance {
                        return instance.clone().downcast::<T>().map_err(|_| {
                            AuraError::invalid("Type mismatch in singleton instance")
                        });
                    }

                    // Create singleton instance
                    let instance = (binding.builder)();
                    binding.singleton_instance = Some(instance.clone());

                    instance
                        .downcast::<T>()
                        .map_err(|_| AuraError::invalid("Type mismatch in created singleton"))
                } else {
                    Err(AuraError::invalid(format!(
                        "Binding disappeared for type {}",
                        type_name
                    )))
                }
            }

            BindingScope::Transient => {
                // Always create new instance
                let instance = (binding.builder)();
                instance
                    .downcast::<T>()
                    .map_err(|_| AuraError::invalid("Type mismatch in transient instance"))
            }

            BindingScope::Scoped(scope_id) => {
                // Check current scope
                let current_scope = *self.current_scope.read().await;
                if current_scope != Some(scope_id) {
                    return Err(AuraError::invalid(format!(
                        "Cannot resolve scoped binding for {} outside of scope {}",
                        type_name, scope_id
                    )));
                }

                // Check if scoped instance exists
                let key = (type_id, scope_id);
                let scoped = self.scoped_instances.read().await;
                if let Some(instance) = scoped.get(&key) {
                    return instance
                        .clone()
                        .downcast::<T>()
                        .map_err(|_| AuraError::invalid("Type mismatch in scoped instance"));
                }

                // Create scoped instance
                drop(scoped);
                let mut scoped = self.scoped_instances.write().await;

                let instance = (binding.builder)();
                scoped.insert(key, instance.clone());

                instance
                    .downcast::<T>()
                    .map_err(|_| AuraError::invalid("Type mismatch in created scoped instance"))
            }
        }
    }

    /// Enter a scope (for testing)
    pub async fn enter_scope(&self, scope_id: u64) {
        *self.current_scope.write().await = Some(scope_id);
        info!("Entered scope {}", scope_id);
    }

    /// Exit the current scope
    pub async fn exit_scope(&self) {
        if let Some(scope_id) = *self.current_scope.read().await {
            // Clear scoped instances
            let mut scoped = self.scoped_instances.write().await;
            scoped.retain(|(_, sid), _| *sid != scope_id);

            *self.current_scope.write().await = None;
            info!("Exited scope {}", scope_id);
        }
    }

    /// Register a lifecycle-aware component
    pub async fn register_lifecycle<T>(&self, name: impl Into<String>, component: T)
    where
        T: LifecycleAware + 'static,
    {
        let mut components = self.lifecycle_components.write().await;
        components.push((name.into(), Box::new(component)));
    }

    /// Get all lifecycle components
    pub async fn lifecycle_components(&self) -> Vec<(String, Box<dyn LifecycleAware>)> {
        // Note: This returns a clone of the components list
        // In practice, we'd want to return references or use a different pattern
        Vec::new() // Placeholder - actual implementation would be more complex
    }

    /// Clear all bindings (useful for testing)
    pub async fn clear(&self) {
        self.bindings.write().await.clear();
        self.scoped_instances.write().await.clear();
        self.lifecycle_components.write().await.clear();
        *self.current_scope.write().await = None;
        info!("Container cleared");
    }
}

impl Default for EffectContainer {
    fn default() -> Self {
        Self::new()
    }
}

/// Extension trait for container builder pattern
pub trait ContainerBuilder {
    /// Register a singleton handler
    fn with_singleton<T>(self, builder: impl Fn() -> T + Send + Sync + 'static) -> Self
    where
        T: Send + Sync + 'static;

    /// Register a transient handler
    fn with_transient<T>(self, builder: impl Fn() -> T + Send + Sync + 'static) -> Self
    where
        T: Send + Sync + 'static;

    /// Register a test instance
    fn with_instance<T>(self, instance: T) -> Self
    where
        T: Send + Sync + 'static;
}

/// Test fixture builder for common handler configurations
pub struct TestFixture {
    container: EffectContainer,
}

impl TestFixture {
    /// Create a new test fixture
    pub fn new() -> Self {
        Self {
            container: EffectContainer::new(),
        }
    }

    /// Build fixture with mock handlers
    pub async fn with_mocks(self) -> EffectContainer {
        use aura_effects::{
            console::MockConsoleHandler, crypto::MockCryptoHandler, journal::MockJournalHandler,
            random::MockRandomHandler, storage::MemoryStorageHandler, time::SimulatedTimeHandler,
            transport::InMemoryTransportHandler,
        };

        // Register all mock handlers
        self.container
            .register_singleton(|| MockCryptoHandler::with_seed(0))
            .await;
        self.container
            .register_singleton(|| InMemoryTransportHandler::new(aura_effects::transport::TransportConfig::default()))
            .await;
        self.container
            .register_singleton(|| MemoryStorageHandler::default())
            .await;
        self.container
            .register_singleton(|| SimulatedTimeHandler::new_at_epoch())
            .await;
        self.container
            .register_singleton(|| MockConsoleHandler::new())
            .await;
        self.container
            .register_singleton(|| MockRandomHandler::new_with_seed(0))
            .await;
        self.container
            .register_singleton(MockJournalHandler::new)
            .await;

        self.container
    }

    /// Build fixture with production-like handlers
    pub async fn with_production_like(self) -> EffectContainer {
        // This would register more realistic handlers
        // For now, we'll use mocks but could be extended
        self.with_mocks().await
    }

    /// Build fixture with custom configuration
    pub async fn with_custom<F>(self, setup: F) -> EffectContainer
    where
        F: FnOnce(&EffectContainer) -> futures::future::BoxFuture<'_, ()>,
    {
        setup(&self.container).await;
        self.container
    }
}

impl Default for TestFixture {
    fn default() -> Self {
        Self::new()
    }
}

/// Scoped container for test isolation
pub struct ScopedContainer {
    container: Arc<EffectContainer>,
    scope_id: u64,
}

impl ScopedContainer {
    /// Create a new scoped container
    pub async fn new(container: Arc<EffectContainer>) -> Self {
        let scope_id = {
            use std::sync::atomic::{AtomicU64, Ordering};
            static SCOPE_COUNTER: AtomicU64 = AtomicU64::new(0);
            SCOPE_COUNTER.fetch_add(1, Ordering::Relaxed)
        };

        container.enter_scope(scope_id).await;

        Self {
            container,
            scope_id,
        }
    }

    /// Get the inner container
    pub fn inner(&self) -> &Arc<EffectContainer> {
        &self.container
    }
}

impl Drop for ScopedContainer {
    fn drop(&mut self) {
        // Schedule scope cleanup
        let container = self.container.clone();
        let scope_id = self.scope_id;

        // We can't await in drop, so we spawn a task
        tokio::spawn(async move {
            container.exit_scope().await;
            debug!("Cleaned up scope {}", scope_id);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::AuraResult;
    use aura_testkit::{aura_test, TestFixture};

    #[derive(Debug, Clone)]
    struct TestHandler {
        id: u64,
    }

    impl TestHandler {
        fn new(id: u64) -> Self {
            Self { id }
        }
    }

    #[aura_test]
    async fn test_singleton_registration() -> AuraResult<()> {
        let fixture = TestFixture::new().await?;
        let container = EffectContainer::new();

        let counter = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let counter_clone = counter.clone();

        container
            .register_singleton(move || {
                let id = counter_clone.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                TestHandler::new(id)
            })
            .await;

        // Resolve multiple times - should get same instance
        let instance1 = container.resolve::<TestHandler>().await?;
        let instance2 = container.resolve::<TestHandler>().await?;

        assert_eq!(instance1.id, instance2.id);
        assert_eq!(instance1.id, 0); // First instance created

        Ok(())
    }

    #[aura_test]
    async fn test_transient_registration() -> AuraResult<()> {
        let fixture = TestFixture::new().await?;
        let container = EffectContainer::new();

        let counter = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let counter_clone = counter.clone();

        container
            .register_transient(move || {
                let id = counter_clone.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                TestHandler::new(id)
            })
            .await;

        // Resolve multiple times - should get different instances
        let instance1 = container.resolve::<TestHandler>().await?;
        let instance2 = container.resolve::<TestHandler>().await?;

        assert_ne!(instance1.id, instance2.id);
        assert_eq!(instance1.id, 0);
        assert_eq!(instance2.id, 1);

        Ok(())
    }

    #[aura_test]
    async fn test_instance_registration() -> AuraResult<()> {
        let fixture = TestFixture::new().await?;
        let container = EffectContainer::new();

        let test_handler = TestHandler::new(42);
        container.register_instance(test_handler.clone()).await;

        let resolved = container.resolve::<TestHandler>().await?;
        assert_eq!(resolved.id, 42);

        Ok(())
    }

    #[aura_test]
    async fn test_scoped_container() -> AuraResult<()> {
        let fixture = TestFixture::new().await?;
        let container = Arc::new(EffectContainer::new());

        {
            let scoped = ScopedContainer::new(container.clone()).await;
            let scope_id = scoped.scope_id;

            scoped
                .inner()
                .register_with_scope(BindingScope::Scoped(scope_id), || TestHandler::new(100))
                .await;

            // Should resolve within scope
            let instance = scoped.inner().resolve::<TestHandler>().await?;
            assert_eq!(instance.id, 100);
        }

        // After scope is dropped, should not be able to resolve
        // (Note: In real test we'd need to wait for the cleanup task)
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Register a non-scoped handler to test
        container.register_singleton(|| TestHandler::new(200)).await;
        let instance = container.resolve::<TestHandler>().await?;
        assert_eq!(instance.id, 200); // Should get the singleton, not the scoped one

        Ok(())
    }

    #[aura_test]
    async fn test_fixture_builder() -> AuraResult<()> {
        let fixture = TestFixture::new().await?;
        let test_fixture = super::TestFixture::new();
        let container = test_fixture.with_mocks().await;

        // Should be able to resolve mock handlers
        use aura_effects::crypto::MockCryptoHandler;
        let crypto = container.resolve::<MockCryptoHandler>().await?;

        Ok(())
    }
}
