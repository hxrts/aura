//! Key derivation middleware for DKD protocols

use super::{CryptoMiddleware, CryptoHandler, CryptoContext};
use crate::{CryptoError, Result};
use crate::middleware::CryptoOperation;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

/// Key derivation middleware that optimizes and validates DKD operations
pub struct KeyDerivationMiddleware {
    /// Key cache for performance
    cache: Arc<RwLock<KeyCache>>,
    
    /// Configuration
    config: KeyDerivationConfig,
}

impl KeyDerivationMiddleware {
    /// Create new key derivation middleware
    pub fn new(config: KeyDerivationConfig) -> Self {
        Self {
            cache: Arc::new(RwLock::new(KeyCache::new())),
            config,
        }
    }
    
    /// Get key derivation statistics
    pub fn stats(&self) -> KeyDerivationStats {
        let cache = self.cache.read().unwrap();
        KeyDerivationStats {
            cached_keys: cache.keys.len(),
            derivation_requests: cache.derivation_requests,
            cache_hits: cache.cache_hits,
            cache_misses: cache.cache_misses,
            validation_failures: cache.validation_failures,
        }
    }
    
    /// Clear key cache
    pub fn clear_cache(&self) {
        let mut cache = self.cache.write().unwrap();
        cache.clear();
    }
}

impl CryptoMiddleware for KeyDerivationMiddleware {
    fn process(
        &self,
        operation: CryptoOperation,
        context: &CryptoContext,
        next: &dyn CryptoHandler,
    ) -> Result<serde_json::Value> {
        match operation {
            CryptoOperation::DeriveKey { app_id, context: derivation_context, derivation_path } => {
                // Validate derivation parameters
                self.validate_derivation_parameters(&app_id, &derivation_context, &derivation_path)?;
                
                // Check rate limiting
                self.check_rate_limiting(&context.account_id.to_string(), &app_id)?;
                
                // Check cache if enabled
                if self.config.enable_caching {
                    let cache_key = self.generate_cache_key(
                        &context.account_id.to_string(),
                        &app_id,
                        &derivation_context,
                        &derivation_path,
                    );
                    
                    if let Some(cached_result) = self.get_cached_key(&cache_key)? {
                        return Ok(serde_json::json!({
                            "operation": "derive_key",
                            "app_id": app_id,
                            "context": derivation_context,
                            "key_hash": cached_result.key_hash,
                            "cached": true,
                            "success": true
                        }));
                    }
                }
                
                // Record derivation attempt
                self.record_derivation_attempt(&context.account_id.to_string(), &app_id)?;
                
                // Process through next handler
                let operation_clone = CryptoOperation::DeriveKey {
                    app_id: app_id.clone(),
                    context: derivation_context.clone(),
                    derivation_path: derivation_path.clone(),
                };
                let result = next.handle(operation_clone, context)?;
                
                // Cache the result if successful and caching is enabled
                if self.config.enable_caching && result.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
                    if let Some(key_hash) = result.get("key_hash").and_then(|v| v.as_str()) {
                        let cache_key = self.generate_cache_key(
                            &context.account_id.to_string(),
                            &app_id,
                            &derivation_context,
                            &derivation_path,
                        );
                        let cached_key = CachedKey {
                            key_hash: key_hash.to_string(),
                            derived_at: context.timestamp,
                            app_id: app_id.clone(),
                            derivation_context: derivation_context.clone(),
                            derivation_path: derivation_path.clone(),
                        };
                        self.cache_key(cache_key, cached_key)?;
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
        "key_derivation"
    }
}

impl KeyDerivationMiddleware {
    fn validate_derivation_parameters(
        &self,
        app_id: &str,
        context: &str,
        derivation_path: &[u32],
    ) -> Result<()> {
        // Validate app_id
        if app_id.is_empty() {
            return Err(CryptoError::invalid_input("App ID cannot be empty"));
        }
        
        if app_id.len() > self.config.max_app_id_length {
            return Err(CryptoError::invalid_input(format!(
                "App ID too long: {} > {}",
                app_id.len(),
                self.config.max_app_id_length
            )));
        }
        
        // Validate context
        if context.is_empty() {
            return Err(CryptoError::invalid_input("Context cannot be empty"));
        }
        
        if context.len() > self.config.max_context_length {
            return Err(CryptoError::invalid_input(format!(
                "Context too long: {} > {}",
                context.len(),
                self.config.max_context_length
            )));
        }
        
        // Validate derivation path
        if derivation_path.len() > self.config.max_derivation_path_length {
            return Err(CryptoError::invalid_input(format!(
                "Derivation path too long: {} > {}",
                derivation_path.len(),
                self.config.max_derivation_path_length
            )));
        }
        
        // Check for forbidden characters
        if !app_id.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == '.') {
            return Err(CryptoError::invalid_input(
                "App ID contains invalid characters"
            ));
        }
        
        Ok(())
    }
    
    fn check_rate_limiting(&self, account_id: &str, app_id: &str) -> Result<()> {
        if !self.config.enable_rate_limiting {
            return Ok(());
        }
        
        let mut cache = self.cache.write().map_err(|_| {
            CryptoError::internal_error("Failed to acquire write lock on key cache")
        })?;
        
        let key = format!("{}:{}", account_id, app_id);
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        
        let requests = cache.rate_limit_tracker
            .entry(key)
            .or_insert_with(Vec::new);
        
        // Remove old requests outside the window
        requests.retain(|&timestamp| now - timestamp < self.config.rate_limit_window_seconds);
        
        // Check if rate limit exceeded
        if requests.len() >= self.config.max_derivations_per_window {
            cache.validation_failures += 1;
            return Err(CryptoError::rate_limited(
                "Too many key derivation requests"
            ));
        }
        
        requests.push(now);
        Ok(())
    }
    
    fn generate_cache_key(
        &self,
        account_id: &str,
        app_id: &str,
        context: &str,
        derivation_path: &[u32],
    ) -> String {
        let path_str = derivation_path
            .iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>()
            .join("/");
        format!("{}:{}:{}:{}", account_id, app_id, context, path_str)
    }
    
    fn get_cached_key(&self, key: &str) -> Result<Option<CachedKey>> {
        let mut cache = self.cache.write().map_err(|_| {
            CryptoError::internal_error("Failed to acquire write lock on key cache")
        })?;
        
        if let Some(cached_key) = cache.keys.get(key).cloned() {
            // Check if cache entry is still valid
            let age = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs() - cached_key.derived_at;
            
            if age <= self.config.cache_ttl_seconds {
                cache.cache_hits += 1;
                Ok(Some(cached_key))
            } else {
                // Remove expired entry
                cache.keys.remove(key);
                cache.cache_misses += 1;
                Ok(None)
            }
        } else {
            cache.cache_misses += 1;
            Ok(None)
        }
    }
    
    fn cache_key(&self, key: String, cached_key: CachedKey) -> Result<()> {
        let mut cache = self.cache.write().map_err(|_| {
            CryptoError::internal_error("Failed to acquire write lock on key cache")
        })?;
        
        // Evict old entries if cache is full
        if cache.keys.len() >= self.config.max_cache_entries {
            cache.evict_oldest();
        }
        
        cache.keys.insert(key, cached_key);
        Ok(())
    }
    
    fn record_derivation_attempt(&self, _account_id: &str, _app_id: &str) -> Result<()> {
        let mut cache = self.cache.write().map_err(|_| {
            CryptoError::internal_error("Failed to acquire write lock on key cache")
        })?;
        
        cache.derivation_requests += 1;
        Ok(())
    }
}

/// Configuration for key derivation middleware
#[derive(Debug, Clone)]
pub struct KeyDerivationConfig {
    /// Whether to enable key caching
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
    
    /// Maximum derivation path length
    pub max_derivation_path_length: usize,
}

impl Default for KeyDerivationConfig {
    fn default() -> Self {
        Self {
            enable_caching: true,
            cache_ttl_seconds: 3600, // 1 hour
            max_cache_entries: 1000,
            enable_rate_limiting: true,
            rate_limit_window_seconds: 60, // 1 minute
            max_derivations_per_window: 20,
            max_app_id_length: 128,
            max_context_length: 256,
            max_derivation_path_length: 10,
        }
    }
}

/// Cached key information
#[derive(Debug, Clone)]
struct CachedKey {
    key_hash: String,
    derived_at: u64,
    app_id: String,
    derivation_context: String,
    derivation_path: Vec<u32>,
}

/// Key cache storage
struct KeyCache {
    keys: HashMap<String, CachedKey>,
    rate_limit_tracker: HashMap<String, Vec<u64>>,
    derivation_requests: u64,
    cache_hits: u64,
    cache_misses: u64,
    validation_failures: u64,
}

impl KeyCache {
    fn new() -> Self {
        Self {
            keys: HashMap::new(),
            rate_limit_tracker: HashMap::new(),
            derivation_requests: 0,
            cache_hits: 0,
            cache_misses: 0,
            validation_failures: 0,
        }
    }
    
    fn clear(&mut self) {
        self.keys.clear();
        self.rate_limit_tracker.clear();
        self.derivation_requests = 0;
        self.cache_hits = 0;
        self.cache_misses = 0;
        self.validation_failures = 0;
    }
    
    fn evict_oldest(&mut self) {
        if let Some(oldest_key) = self.keys
            .iter()
            .min_by_key(|(_, key)| key.derived_at)
            .map(|(key, _)| key.clone())
        {
            self.keys.remove(&oldest_key);
        }
    }
}

/// Key derivation statistics
#[derive(Debug, Clone)]
pub struct KeyDerivationStats {
    /// Number of cached keys
    pub cached_keys: usize,
    
    /// Total derivation requests
    pub derivation_requests: u64,
    
    /// Cache hits
    pub cache_hits: u64,
    
    /// Cache misses
    pub cache_misses: u64,
    
    /// Validation failures
    pub validation_failures: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleware::handler::NoOpHandler;
    use crate::middleware::SecurityLevel;
    use aura_types::{AccountIdExt, DeviceIdExt};
    use aura_crypto::Effects;
    
    #[test]
    fn test_key_derivation_middleware() {
        let effects = Effects::test(42);
        let account_id = aura_types::AccountId::new_with_effects(&effects);
        let device_id = aura_types::DeviceId::new_with_effects(&effects);
        
        let middleware = KeyDerivationMiddleware::new(KeyDerivationConfig::default());
        let handler = NoOpHandler;
        let context = CryptoContext::new(
            account_id,
            device_id,
            "test".to_string(),
            SecurityLevel::High,
        );
        let operation = CryptoOperation::DeriveKey {
            app_id: "test-app".to_string(),
            context: "user-context".to_string(),
            derivation_path: vec![0, 1, 2],
        };
        
        let result = middleware.process(operation, &context, &handler);
        assert!(result.is_ok());
        
        let stats = middleware.stats();
        assert_eq!(stats.derivation_requests, 1);
    }
    
    #[test]
    fn test_key_derivation_validation() {
        let middleware = KeyDerivationMiddleware::new(KeyDerivationConfig::default());
        
        // Valid parameters
        assert!(middleware.validate_derivation_parameters(
            "valid-app",
            "valid-context",
            &[0, 1, 2]
        ).is_ok());
        
        // Invalid app_id
        assert!(middleware.validate_derivation_parameters(
            "",
            "valid-context",
            &[0, 1, 2]
        ).is_err());
        
        assert!(middleware.validate_derivation_parameters(
            "app with spaces",
            "valid-context",
            &[0, 1, 2]
        ).is_err());
        
        // Invalid context
        assert!(middleware.validate_derivation_parameters(
            "valid-app",
            "",
            &[0, 1, 2]
        ).is_err());
        
        // Invalid derivation path
        let long_path: Vec<u32> = (0..20).collect();
        assert!(middleware.validate_derivation_parameters(
            "valid-app",
            "valid-context",
            &long_path
        ).is_err());
    }
    
    #[test]
    fn test_rate_limiting() {
        let config = KeyDerivationConfig {
            max_derivations_per_window: 2,
            rate_limit_window_seconds: 60,
            ..KeyDerivationConfig::default()
        };
        let middleware = KeyDerivationMiddleware::new(config);
        
        // First two requests should succeed
        assert!(middleware.check_rate_limiting("account1", "app1").is_ok());
        assert!(middleware.check_rate_limiting("account1", "app1").is_ok());
        
        // Third request should be rate limited
        assert!(middleware.check_rate_limiting("account1", "app1").is_err());
        
        // Different app should have separate limit
        assert!(middleware.check_rate_limiting("account1", "app2").is_ok());
    }
}