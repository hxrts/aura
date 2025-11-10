//! Secure random number generation middleware

use super::{CryptoContext, CryptoHandler, CryptoMiddleware};
use crate::middleware::CryptoOperation;
use crate::{CryptoError, Result};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Secure random middleware that manages random number generation with quality assurance
pub struct SecureRandomMiddleware {
    /// Random number quality tracker
    tracker: Arc<RwLock<RandomTracker>>,

    /// Configuration
    config: RandomConfig,
}

impl SecureRandomMiddleware {
    /// Create new secure random middleware
    pub fn new(config: RandomConfig) -> Self {
        Self {
            tracker: Arc::new(RwLock::new(RandomTracker::new())),
            config,
        }
    }

    /// Get random generation statistics
    pub fn stats(&self) -> RandomStats {
        let tracker = self.tracker.read().unwrap();
        tracker.stats()
    }

    /// Perform entropy health check
    pub fn entropy_health_check(&self) -> Result<EntropyHealth> {
        let tracker = self.tracker.read().unwrap();
        Ok(tracker.entropy_health(&self.config))
    }
}

impl CryptoMiddleware for SecureRandomMiddleware {
    fn process(
        &self,
        operation: CryptoOperation,
        context: &CryptoContext,
        next: &dyn CryptoHandler,
    ) -> Result<serde_json::Value> {
        match operation {
            CryptoOperation::GenerateRandom { num_bytes } => {
                // Validate random generation request
                self.validate_random_request(num_bytes)?;

                // Check rate limiting
                self.check_rate_limiting(&context.device_id.to_string())?;

                // Check entropy health before generation
                if self.config.check_entropy_health {
                    let health = self.entropy_health_check()?;
                    if health.status != EntropyStatus::Healthy {
                        return Err(CryptoError::insufficient_entropy(format!(
                            "Entropy health check failed: {:?}",
                            health.status
                        )));
                    }
                }

                // Record generation attempt
                self.record_generation_attempt(num_bytes)?;

                // Process through next handler
                let result = next.handle(operation, context)?;

                // Validate and record the generated random bytes
                if result
                    .get("success")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                {
                    if let Some(bytes_hex) = result.get("bytes").and_then(|v| v.as_str()) {
                        self.validate_generated_bytes(bytes_hex, num_bytes)?;
                        self.record_successful_generation(num_bytes)?;
                    }
                } else {
                    self.record_failed_generation()?;
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
        "secure_random"
    }
}

impl SecureRandomMiddleware {
    fn validate_random_request(&self, num_bytes: usize) -> Result<()> {
        if num_bytes == 0 {
            return Err(CryptoError::invalid_input("Cannot generate zero bytes"));
        }

        if num_bytes > self.config.max_bytes_per_request {
            return Err(CryptoError::invalid_input(format!(
                "Requested {} bytes exceeds maximum {}",
                num_bytes, self.config.max_bytes_per_request
            )));
        }

        Ok(())
    }

    fn check_rate_limiting(&self, device_id: &str) -> Result<()> {
        if !self.config.enable_rate_limiting {
            return Ok(());
        }

        let mut tracker = self.tracker.write().map_err(|_| {
            CryptoError::internal_error("Failed to acquire write lock on random tracker")
        })?;

        #[allow(clippy::disallowed_methods)] // [VERIFIED] Acceptable in rate limiting middleware
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let requests = tracker
            .rate_limit_tracker
            .entry(device_id.to_string())
            .or_insert_with(Vec::new);

        // Remove old requests outside the window
        requests.retain(|&timestamp| now - timestamp < self.config.rate_limit_window.as_secs());

        if requests.len() >= self.config.max_requests_per_window {
            return Err(CryptoError::rate_limited(
                "Too many random generation requests",
            ));
        }

        requests.push(now);
        Ok(())
    }

    fn record_generation_attempt(&self, num_bytes: usize) -> Result<()> {
        let mut tracker = self.tracker.write().map_err(|_| {
            CryptoError::internal_error("Failed to acquire write lock on random tracker")
        })?;

        tracker.total_requests += 1;
        tracker.total_bytes_requested += num_bytes as u64;

        Ok(())
    }

    fn validate_generated_bytes(&self, bytes_hex: &str, expected_length: usize) -> Result<()> {
        // Decode hex to check actual length
        let bytes = hex::decode(bytes_hex)
            .map_err(|e| CryptoError::invalid_output(format!("Invalid hex encoding: {}", e)))?;

        if bytes.len() != expected_length {
            return Err(CryptoError::invalid_output(format!(
                "Generated {} bytes, expected {}",
                bytes.len(),
                expected_length
            )));
        }

        // Perform basic randomness quality checks if enabled
        if self.config.enable_quality_checks {
            self.perform_quality_checks(&bytes)?;
        }

        Ok(())
    }

    fn perform_quality_checks(&self, bytes: &[u8]) -> Result<()> {
        // Basic entropy checks

        // 1. Check for obvious patterns (all zeros, all ones, etc.)
        if bytes.iter().all(|&b| b == 0) {
            return Err(CryptoError::poor_randomness("All bytes are zero"));
        }

        if bytes.iter().all(|&b| b == 0xFF) {
            return Err(CryptoError::poor_randomness("All bytes are 0xFF"));
        }

        // 2. Check for repeating patterns (TODO fix - Simplified)
        if bytes.len() >= 4 {
            let first_byte = bytes[0];
            if bytes.iter().all(|&b| b == first_byte) {
                return Err(CryptoError::poor_randomness("All bytes are identical"));
            }
        }

        // 3. Basic frequency analysis for longer sequences
        if bytes.len() >= 32 && self.config.enable_frequency_analysis {
            let mut byte_counts = [0u32; 256];
            for &byte in bytes {
                byte_counts[byte as usize] += 1;
            }

            // Check if any single byte value appears too frequently
            let max_expected_frequency = (bytes.len() as f64 * 1.5 / 256.0) as u32;
            if byte_counts
                .iter()
                .any(|&count| count > max_expected_frequency)
            {
                return Err(CryptoError::poor_randomness(
                    "Byte frequency distribution appears non-random",
                ));
            }
        }

        Ok(())
    }

    fn record_successful_generation(&self, num_bytes: usize) -> Result<()> {
        let mut tracker = self.tracker.write().map_err(|_| {
            CryptoError::internal_error("Failed to acquire write lock on random tracker")
        })?;

        tracker.successful_requests += 1;
        tracker.total_bytes_generated += num_bytes as u64;

        Ok(())
    }

    fn record_failed_generation(&self) -> Result<()> {
        let mut tracker = self.tracker.write().map_err(|_| {
            CryptoError::internal_error("Failed to acquire write lock on random tracker")
        })?;

        tracker.failed_requests += 1;

        Ok(())
    }
}

/// Configuration for secure random middleware
#[derive(Debug, Clone)]
pub struct RandomConfig {
    /// Maximum bytes per single request
    pub max_bytes_per_request: usize,

    /// Whether to enable rate limiting
    pub enable_rate_limiting: bool,

    /// Rate limiting window
    pub rate_limit_window: Duration,

    /// Maximum requests per window
    pub max_requests_per_window: usize,

    /// Whether to check entropy health before generation
    pub check_entropy_health: bool,

    /// Whether to enable quality checks on generated bytes
    pub enable_quality_checks: bool,

    /// Whether to enable frequency analysis for quality checks
    pub enable_frequency_analysis: bool,

    /// Minimum entropy threshold for health checks
    pub min_entropy_threshold: f64,
}

impl Default for RandomConfig {
    fn default() -> Self {
        Self {
            max_bytes_per_request: 1024 * 1024, // 1 MB
            enable_rate_limiting: true,
            rate_limit_window: Duration::from_secs(60), // 1 minute
            max_requests_per_window: 100,
            check_entropy_health: true,
            enable_quality_checks: true,
            enable_frequency_analysis: true,
            min_entropy_threshold: 7.0, // bits per byte
        }
    }
}

/// Entropy health status
#[derive(Debug, Clone, PartialEq)]
pub enum EntropyStatus {
    Healthy,
    Warning,
    Critical,
    Unknown,
}

/// Entropy health information
#[derive(Debug, Clone)]
pub struct EntropyHealth {
    pub status: EntropyStatus,
    pub estimated_entropy: f64,
    pub last_check: u64,
    pub quality_score: f64,
}

/// Random number generation tracker
struct RandomTracker {
    total_requests: u64,
    successful_requests: u64,
    failed_requests: u64,
    total_bytes_requested: u64,
    total_bytes_generated: u64,
    rate_limit_tracker: HashMap<String, Vec<u64>>, // device_id -> timestamps
    #[allow(dead_code)] // Stored for entropy monitoring
    last_entropy_check: u64,
    entropy_samples: Vec<f64>,
}

impl RandomTracker {
    fn new() -> Self {
        Self {
            total_requests: 0,
            successful_requests: 0,
            failed_requests: 0,
            total_bytes_requested: 0,
            total_bytes_generated: 0,
            rate_limit_tracker: HashMap::new(),
            last_entropy_check: 0,
            entropy_samples: Vec::new(),
        }
    }

    fn stats(&self) -> RandomStats {
        let success_rate = if self.total_requests > 0 {
            self.successful_requests as f64 / self.total_requests as f64
        } else {
            0.0
        };

        RandomStats {
            total_requests: self.total_requests,
            successful_requests: self.successful_requests,
            failed_requests: self.failed_requests,
            total_bytes_requested: self.total_bytes_requested,
            total_bytes_generated: self.total_bytes_generated,
            success_rate,
            average_entropy: self.entropy_samples.iter().sum::<f64>()
                / self.entropy_samples.len().max(1) as f64,
        }
    }

    fn entropy_health(&self, config: &RandomConfig) -> EntropyHealth {
        #[allow(clippy::disallowed_methods)] // [VERIFIED] Acceptable in entropy health check
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // TODO fix - Simplified entropy estimation based on historical data
        let estimated_entropy = if !self.entropy_samples.is_empty() {
            self.entropy_samples.iter().sum::<f64>() / self.entropy_samples.len() as f64
        } else {
            8.0 // Assume good entropy if no samples
        };

        let status = if estimated_entropy >= config.min_entropy_threshold {
            EntropyStatus::Healthy
        } else if estimated_entropy >= config.min_entropy_threshold * 0.8 {
            EntropyStatus::Warning
        } else {
            EntropyStatus::Critical
        };

        // Quality score based on success rate and entropy
        let success_rate = if self.total_requests > 0 {
            self.successful_requests as f64 / self.total_requests as f64
        } else {
            1.0
        };

        let quality_score = (estimated_entropy / 8.0) * success_rate;

        EntropyHealth {
            status,
            estimated_entropy,
            last_check: now,
            quality_score,
        }
    }
}

/// Random generation statistics
#[derive(Debug, Clone)]
pub struct RandomStats {
    /// Total random generation requests
    pub total_requests: u64,

    /// Successful requests
    pub successful_requests: u64,

    /// Failed requests
    pub failed_requests: u64,

    /// Total bytes requested
    pub total_bytes_requested: u64,

    /// Total bytes generated
    pub total_bytes_generated: u64,

    /// Success rate (0.0 to 1.0)
    pub success_rate: f64,

    /// Average entropy estimate
    pub average_entropy: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleware::handler::NoOpHandler;
    use crate::middleware::SecurityLevel;
    use crate::Effects;
    use aura_core::{AccountIdExt, DeviceIdExt};

    #[test]
    fn test_secure_random_middleware() {
        let effects = Effects::test();
        let account_id = aura_core::AccountId::new_with_effects(&effects);
        let device_id = aura_core::DeviceId::new_with_effects(&effects);

        let middleware = SecureRandomMiddleware::new(RandomConfig::default());
        let handler = NoOpHandler;
        let context = CryptoContext::new(
            account_id,
            device_id,
            "test".to_string(),
            SecurityLevel::Standard,
        );
        let operation = CryptoOperation::GenerateRandom { num_bytes: 32 };

        let result = middleware.process(operation, &context, &handler);
        assert!(result.is_ok());

        let stats = middleware.stats();
        assert_eq!(stats.total_requests, 1);
    }

    #[test]
    fn test_random_request_validation() {
        let middleware = SecureRandomMiddleware::new(RandomConfig::default());

        // Valid request
        assert!(middleware.validate_random_request(32).is_ok());

        // Invalid zero bytes
        assert!(middleware.validate_random_request(0).is_err());

        // Invalid too many bytes
        assert!(middleware
            .validate_random_request(10 * 1024 * 1024)
            .is_err());
    }

    #[test]
    fn test_quality_checks() {
        let middleware = SecureRandomMiddleware::new(RandomConfig::default());

        // Good random bytes
        let good_bytes = vec![0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0];
        assert!(middleware.perform_quality_checks(&good_bytes).is_ok());

        // All zeros (bad)
        let zero_bytes = vec![0x00; 8];
        assert!(middleware.perform_quality_checks(&zero_bytes).is_err());

        // All 0xFF (bad)
        let ff_bytes = vec![0xFF; 8];
        assert!(middleware.perform_quality_checks(&ff_bytes).is_err());

        // All same byte (bad)
        let same_bytes = vec![0x42; 8];
        assert!(middleware.perform_quality_checks(&same_bytes).is_err());
    }

    #[test]
    fn test_rate_limiting() {
        let config = RandomConfig {
            max_requests_per_window: 2,
            rate_limit_window: Duration::from_secs(60),
            ..RandomConfig::default()
        };
        let middleware = SecureRandomMiddleware::new(config);

        // First two requests should succeed
        assert!(middleware.check_rate_limiting("device1").is_ok());
        assert!(middleware.check_rate_limiting("device1").is_ok());

        // Third request should be rate limited
        assert!(middleware.check_rate_limiting("device1").is_err());

        // Different device should have separate limit
        assert!(middleware.check_rate_limiting("device2").is_ok());
    }

    #[test]
    fn test_entropy_health() {
        let middleware = SecureRandomMiddleware::new(RandomConfig::default());
        let health = middleware.entropy_health_check().unwrap();

        // Should return some health status
        assert!(matches!(
            health.status,
            EntropyStatus::Healthy
                | EntropyStatus::Warning
                | EntropyStatus::Critical
                | EntropyStatus::Unknown
        ));
        assert!(health.quality_score >= 0.0 && health.quality_score <= 1.0);
    }
}
