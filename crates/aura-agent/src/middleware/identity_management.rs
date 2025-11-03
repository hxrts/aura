//! Identity management middleware for DKD protocols

use super::{AgentContext, AgentHandler, AgentMiddleware};
use crate::error::Result;
use crate::middleware::AgentOperation;
use crate::utils::time::AgentTimeProvider;
use aura_types::AuraError;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Identity management middleware that handles DKD protocols
pub struct IdentityManagementMiddleware {
    /// Identity cache for performance
    cache: Arc<RwLock<IdentityCache>>,

    /// Configuration
    config: IdentityConfig,

    /// Time provider for timestamp generation
    time_provider: Arc<AgentTimeProvider>,
}

impl IdentityManagementMiddleware {
    /// Create new identity management middleware with production time provider
    pub fn new(config: IdentityConfig) -> Self {
        Self {
            cache: Arc::new(RwLock::new(IdentityCache::new())),
            config,
            time_provider: Arc::new(AgentTimeProvider::production()),
        }
    }

    /// Create new identity management middleware with custom time provider
    pub fn with_time_provider(
        config: IdentityConfig,
        time_provider: Arc<AgentTimeProvider>,
    ) -> Self {
        Self {
            cache: Arc::new(RwLock::new(IdentityCache::new())),
            config,
            time_provider,
        }
    }

    /// Get identity management statistics
    pub fn stats(&self) -> IdentityStats {
        let cache = self.cache.read().unwrap();
        IdentityStats {
            cached_identities: cache.identities.len(),
            derivation_requests: cache.derivation_requests,
            cache_hits: cache.cache_hits,
            cache_misses: cache.cache_misses,
        }
    }

    /// Clear identity cache
    pub fn clear_cache(&self) {
        let mut cache = self.cache.write().unwrap();
        cache.clear();
    }
}

impl AgentMiddleware for IdentityManagementMiddleware {
    fn process(
        &self,
        operation: AgentOperation,
        context: &AgentContext,
        next: &dyn AgentHandler,
    ) -> Result<serde_json::Value> {
        match &operation {
            AgentOperation::DeriveIdentity {
                app_id,
                context: app_context,
            } => {
                // Check cache first if enabled
                if self.config.enable_caching {
                    let cache_key = self.generate_cache_key(
                        &context.account_id.to_string(),
                        app_id,
                        app_context,
                    );

                    if let Some(cached_identity) = self.get_cached_identity(&cache_key)? {
                        return Ok(serde_json::json!({
                            "operation": "derive_identity",
                            "app_id": app_id.clone(),
                            "context": app_context.clone(),
                            "identity_hash": cached_identity.identity_hash,
                            "cached": true,
                            "success": true
                        }));
                    }
                }

                // Clone the data we need for processing
                let app_id_clone = app_id.clone();
                let app_context_clone = app_context.clone();

                // Validate DKD parameters
                self.validate_dkd_parameters(&app_id_clone, &app_context_clone)?;

                // Record derivation attempt
                self.record_derivation_attempt(&context.account_id.to_string(), &app_id_clone)?;

                // Process through next handler
                let result = next.handle(operation, context)?;

                // Cache the result if successful and caching is enabled
                if self.config.enable_caching
                    && result
                        .get("success")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false)
                {
                    if let Some(identity_hash) =
                        result.get("identity_hash").and_then(|v| v.as_str())
                    {
                        let cache_key = self.generate_cache_key(
                            &context.account_id.to_string(),
                            &app_id_clone,
                            &app_context_clone,
                        );
                        let identity = CachedIdentity {
                            identity_hash: identity_hash.to_string(),
                            derived_at: context.timestamp,
                            app_id: app_id_clone,
                            app_context: app_context_clone,
                        };
                        self.cache_identity(cache_key, identity)?;
                    }
                }

                Ok(result)
            }

            _ => {
                // Pass through other operations
                next.handle(operation, context)
            }
        }
    }

    fn name(&self) -> &str {
        "identity_management"
    }
}

impl IdentityManagementMiddleware {
    fn validate_dkd_parameters(&self, app_id: &str, context: &str) -> Result<()> {
        // Validate app_id
        if app_id.is_empty() {
            return Err(AuraError::invalid_input(
                "App ID cannot be empty".to_string(),
            ));
        }

        if app_id.len() > self.config.max_app_id_length {
            return Err(AuraError::invalid_input(format!(
                "App ID too long: {} > {}",
                app_id.len(),
                self.config.max_app_id_length
            )));
        }

        // Validate context
        if context.is_empty() {
            return Err(AuraError::invalid_input(
                "Context cannot be empty".to_string(),
            ));
        }

        if context.len() > self.config.max_context_length {
            return Err(AuraError::invalid_input(format!(
                "Context too long: {} > {}",
                context.len(),
                self.config.max_context_length
            )));
        }

        // Check for forbidden characters
        if !app_id
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == '.')
        {
            return Err(AuraError::invalid_input(
                "App ID contains invalid characters".to_string(),
            ));
        }

        Ok(())
    }

    fn generate_cache_key(&self, account_id: &str, app_id: &str, context: &str) -> String {
        format!("{}:{}:{}", account_id, app_id, context)
    }

    fn get_cached_identity(&self, key: &str) -> Result<Option<CachedIdentity>> {
        let mut cache = self.cache.write().map_err(|_| {
            AuraError::internal_error("Failed to acquire write lock on identity cache".to_string())
        })?;

        if let Some(identity) = cache.identities.get(key) {
            // Check if cache entry is still valid
            let now = self.time_provider.timestamp_secs();
            let age = now - identity.derived_at;

            if age <= self.config.cache_ttl_seconds {
                let result = identity.clone();
                cache.cache_hits += 1;
                return Ok(Some(result));
            } else {
                // Remove expired entry
                cache.identities.remove(key);
                cache.cache_misses += 1;
                return Ok(None);
            }
        }

        cache.cache_misses += 1;
        Ok(None)
    }

    fn cache_identity(&self, key: String, identity: CachedIdentity) -> Result<()> {
        let mut cache = self.cache.write().map_err(|_| {
            AuraError::internal_error("Failed to acquire write lock on identity cache".to_string())
        })?;

        // Evict old entries if cache is full
        if cache.identities.len() >= self.config.max_cache_entries {
            cache.evict_oldest();
        }

        cache.identities.insert(key, identity);
        Ok(())
    }

    fn record_derivation_attempt(&self, account_id: &str, app_id: &str) -> Result<()> {
        let mut cache = self.cache.write().map_err(|_| {
            AuraError::internal_error("Failed to acquire write lock on identity cache".to_string())
        })?;

        cache.derivation_requests += 1;

        // Check rate limiting if enabled
        if self.config.enable_rate_limiting {
            let key = format!("{}:{}", account_id, app_id);
            let now = self.time_provider.timestamp_secs();

            let requests = cache.rate_limit_tracker.entry(key).or_insert_with(Vec::new);

            // Remove old requests outside the window
            requests.retain(|&timestamp| now - timestamp < self.config.rate_limit_window_seconds);

            // Check if rate limit exceeded
            if requests.len() >= self.config.max_derivations_per_window {
                return Err(AuraError::rate_limited(
                    "Too many identity derivation requests".to_string(),
                ));
            }

            requests.push(now);
        }

        Ok(())
    }
}

/// Configuration for identity management middleware
#[derive(Debug, Clone)]
pub struct IdentityConfig {
    /// Whether to enable identity caching
    pub enable_caching: bool,

    /// Cache TTL in seconds
    pub cache_ttl_seconds: u64,

    /// Maximum cache entries
    pub max_cache_entries: usize,

    /// Whether to enable rate limiting
    pub enable_rate_limiting: bool,

    /// Rate limit window in seconds
    pub rate_limit_window_seconds: u64,

    /// Maximum derivations per window
    pub max_derivations_per_window: usize,

    /// Maximum app ID length
    pub max_app_id_length: usize,

    /// Maximum context length
    pub max_context_length: usize,
}

impl Default for IdentityConfig {
    fn default() -> Self {
        Self {
            enable_caching: true,
            cache_ttl_seconds: 3600, // 1 hour
            max_cache_entries: 1000,
            enable_rate_limiting: true,
            rate_limit_window_seconds: 60, // 1 minute
            max_derivations_per_window: 10,
            max_app_id_length: 128,
            max_context_length: 256,
        }
    }
}

/// Cached identity information
#[derive(Debug, Clone)]
struct CachedIdentity {
    identity_hash: String,
    derived_at: u64,
    app_id: String,
    app_context: String,
}

/// Identity cache storage
struct IdentityCache {
    identities: HashMap<String, CachedIdentity>,
    rate_limit_tracker: HashMap<String, Vec<u64>>,
    derivation_requests: u64,
    cache_hits: u64,
    cache_misses: u64,
}

impl IdentityCache {
    fn new() -> Self {
        Self {
            identities: HashMap::new(),
            rate_limit_tracker: HashMap::new(),
            derivation_requests: 0,
            cache_hits: 0,
            cache_misses: 0,
        }
    }

    fn clear(&mut self) {
        self.identities.clear();
        self.rate_limit_tracker.clear();
        self.derivation_requests = 0;
        self.cache_hits = 0;
        self.cache_misses = 0;
    }

    fn evict_oldest(&mut self) {
        if let Some(oldest_key) = self
            .identities
            .iter()
            .min_by_key(|(_, identity)| identity.derived_at)
            .map(|(key, _)| key.clone())
        {
            self.identities.remove(&oldest_key);
        }
    }
}

/// Identity management statistics
#[derive(Debug, Clone)]
pub struct IdentityStats {
    /// Number of cached identities
    pub cached_identities: usize,

    /// Total derivation requests
    pub derivation_requests: u64,

    /// Cache hits
    pub cache_hits: u64,

    /// Cache misses
    pub cache_misses: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleware::handler::NoOpHandler;
    use aura_crypto::Effects;
    use aura_types::{AccountIdExt, DeviceIdExt};

    #[test]
    fn test_identity_management_middleware() {
        let effects = Effects::test(42);
        let account_id = aura_types::AccountId::new_with_effects(&effects);
        let device_id = aura_types::DeviceId::new_with_effects(&effects);

        let middleware = IdentityManagementMiddleware::new(IdentityConfig::default());
        let handler = NoOpHandler;
        let context = AgentContext::new(account_id, device_id, "test".to_string());
        let operation = AgentOperation::DeriveIdentity {
            app_id: "test-app".to_string(),
            context: "user-context".to_string(),
        };

        let result = middleware.process(operation, &context, &handler);
        assert!(result.is_ok());

        let stats = middleware.stats();
        assert_eq!(stats.derivation_requests, 1);
    }

    #[test]
    fn test_identity_validation() {
        let middleware = IdentityManagementMiddleware::new(IdentityConfig::default());

        // Valid parameters
        assert!(middleware
            .validate_dkd_parameters("valid-app", "valid-context")
            .is_ok());

        // Invalid app_id
        assert!(middleware
            .validate_dkd_parameters("", "valid-context")
            .is_err());
        assert!(middleware
            .validate_dkd_parameters("app with spaces", "valid-context")
            .is_err());

        // Invalid context
        assert!(middleware.validate_dkd_parameters("valid-app", "").is_err());
    }

    #[test]
    fn test_identity_caching() {
        let effects = Effects::test(42);
        let account_id = aura_types::AccountId::new_with_effects(&effects);
        let device_id = aura_types::DeviceId::new_with_effects(&effects);

        let middleware = IdentityManagementMiddleware::new(IdentityConfig::default());
        let handler = NoOpHandler;
        let context = AgentContext::new(account_id, device_id, "test".to_string());
        let operation = AgentOperation::DeriveIdentity {
            app_id: "test-app".to_string(),
            context: "user-context".to_string(),
        };

        // First call - cache miss
        let result1 = middleware.process(operation.clone(), &context, &handler);
        assert!(result1.is_ok());

        let stats_after_first = middleware.stats();
        assert_eq!(stats_after_first.cache_misses, 1);

        // Second call should be cache hit (but NoOpHandler doesn't provide identity_hash for caching)
        let result2 = middleware.process(operation, &context, &handler);
        assert!(result2.is_ok());
    }
}
