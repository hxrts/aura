//! Timing protection middleware for constant-time operations

use super::{CryptoContext, CryptoHandler, CryptoMiddleware};
use crate::middleware::CryptoOperation;
use crate::{CryptoError, Result};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

/// Timing protection middleware that ensures constant-time operations
pub struct TimingProtectionMiddleware {
    /// Timing analyzer for detecting timing leaks
    analyzer: Arc<RwLock<TimingAnalyzer>>,

    /// Configuration
    config: TimingConfig,
}

impl TimingProtectionMiddleware {
    /// Create new timing protection middleware
    pub fn new(config: TimingConfig) -> Self {
        Self {
            analyzer: Arc::new(RwLock::new(TimingAnalyzer::new())),
            config,
        }
    }

    /// Get timing protection statistics
    pub fn stats(&self) -> TimingStats {
        let analyzer = self.analyzer.read().unwrap();
        analyzer.stats()
    }

    /// Analyze timing patterns for potential leaks
    pub fn analyze_timing_patterns(&self) -> Result<TimingAnalysis> {
        let analyzer = self.analyzer.read().unwrap();
        Ok(analyzer.analyze_patterns(&self.config))
    }
}

impl CryptoMiddleware for TimingProtectionMiddleware {
    fn process(
        &self,
        operation: CryptoOperation,
        context: &CryptoContext,
        next: &dyn CryptoHandler,
    ) -> Result<serde_json::Value> {
        // Determine if this operation requires timing protection
        let requires_protection = self.requires_timing_protection(&operation);

        if !requires_protection && !self.config.monitor_all_operations {
            return next.handle(operation, context);
        }

        #[allow(clippy::disallowed_methods)]
        // [VERIFIED] Acceptable in timing protection measurement
        let start_time = Instant::now();

        // Execute the operation
        let result = next.handle(operation.clone(), context);

        let execution_time = start_time.elapsed();

        // Record timing information
        self.record_timing(&operation, execution_time, result.is_ok())?;

        // Apply timing protection if required
        if requires_protection {
            self.apply_timing_protection(&operation, execution_time)?;
        }

        // Check for timing anomalies
        if self.config.detect_timing_anomalies {
            self.check_timing_anomalies(&operation, execution_time)?;
        }

        result
    }

    fn name(&self) -> &str {
        "timing_protection"
    }
}

impl TimingProtectionMiddleware {
    fn requires_timing_protection(&self, operation: &CryptoOperation) -> bool {
        match operation {
            CryptoOperation::DeriveKey { .. } => true,
            CryptoOperation::GenerateSignature { .. } => true,
            CryptoOperation::VerifySignature { .. } => self.config.protect_verification,
            CryptoOperation::Encrypt { .. } => true,
            CryptoOperation::Decrypt { .. } => true,
            CryptoOperation::GenerateRandom { .. } => false,
            CryptoOperation::Hash { .. } => false,
            CryptoOperation::RotateKeys { .. } => true,
        }
    }

    fn record_timing(
        &self,
        operation: &CryptoOperation,
        execution_time: Duration,
        success: bool,
    ) -> Result<()> {
        let mut analyzer = self.analyzer.write().map_err(|_| {
            CryptoError::internal_error("Failed to acquire write lock on timing analyzer")
        })?;

        let operation_type = self.operation_type_string(operation);
        analyzer.record_timing(operation_type, execution_time, success);

        Ok(())
    }

    fn apply_timing_protection(
        &self,
        operation: &CryptoOperation,
        execution_time: Duration,
    ) -> Result<()> {
        if !self.config.enable_constant_time {
            return Ok(());
        }

        let target_time = self.get_target_time(operation);

        if execution_time < target_time {
            let delay = target_time - execution_time;

            // Add jitter to prevent timing-based fingerprinting
            let jitter = if self.config.enable_jitter {
                #[allow(clippy::disallowed_methods)]
                // [VERIFIED] Acceptable in timing protection jitter
                Duration::from_nanos(
                    (rand::random::<u64>() % 1000) * 1000, // 0-1ms jitter
                )
            } else {
                Duration::ZERO
            };

            let total_delay = delay + jitter;

            // Perform constant-time delay
            self.constant_time_delay(total_delay);
        }

        Ok(())
    }

    fn get_target_time(&self, operation: &CryptoOperation) -> Duration {
        match operation {
            CryptoOperation::DeriveKey { .. } => self.config.target_key_derivation_time,
            CryptoOperation::GenerateSignature { .. } => self.config.target_signature_time,
            CryptoOperation::VerifySignature { .. } => self.config.target_verification_time,
            CryptoOperation::Encrypt { .. } => self.config.target_encryption_time,
            CryptoOperation::Decrypt { .. } => self.config.target_decryption_time,
            CryptoOperation::RotateKeys { .. } => self.config.target_key_rotation_time,
            _ => Duration::from_millis(1), // Default minimum time
        }
    }

    fn constant_time_delay(&self, delay: Duration) {
        // Implement a constant-time delay that doesn't reveal timing information
        // This is a simplified implementation - real-world implementations would use
        // more sophisticated constant-time delay mechanisms

        #[allow(clippy::disallowed_methods)]
        // [VERIFIED] Acceptable in constant-time delay measurement
        let start = Instant::now();
        let mut counter = 0u64;

        // Busy-wait loop that performs constant operations
        while start.elapsed() < delay {
            // Perform some constant-time operations
            counter = counter.wrapping_mul(1664525).wrapping_add(1013904223);

            // Add a small yielding mechanism to prevent excessive CPU usage
            if counter % 10000 == 0 {
                std::hint::spin_loop();
            }
        }
    }

    fn check_timing_anomalies(
        &self,
        operation: &CryptoOperation,
        execution_time: Duration,
    ) -> Result<()> {
        let analyzer = self.analyzer.read().map_err(|_| {
            CryptoError::internal_error("Failed to acquire read lock on timing analyzer")
        })?;

        let operation_type = self.operation_type_string(operation);

        if let Some(baseline) = analyzer.get_baseline_timing(&operation_type) {
            let deviation = if execution_time > baseline {
                execution_time - baseline
            } else {
                baseline - execution_time
            };

            let relative_deviation = deviation.as_nanos() as f64 / baseline.as_nanos() as f64;

            if relative_deviation > self.config.anomaly_threshold {
                eprintln!(
                    "Timing anomaly detected for {}: {}ms (baseline: {}ms, deviation: {:.2}%)",
                    operation_type,
                    execution_time.as_millis(),
                    baseline.as_millis(),
                    relative_deviation * 100.0
                );

                if self.config.fail_on_anomalies {
                    return Err(CryptoError::timing_anomaly(format!(
                        "Operation: {}, execution: {}ms, baseline: {}ms, deviation: {:.2}%",
                        operation_type,
                        execution_time.as_millis(),
                        baseline.as_millis(),
                        relative_deviation * 100.0
                    )));
                }
            }
        }

        Ok(())
    }

    fn operation_type_string(&self, operation: &CryptoOperation) -> String {
        match operation {
            CryptoOperation::DeriveKey { .. } => "derive_key".to_string(),
            CryptoOperation::GenerateSignature { .. } => "generate_signature".to_string(),
            CryptoOperation::VerifySignature { .. } => "verify_signature".to_string(),
            CryptoOperation::GenerateRandom { .. } => "generate_random".to_string(),
            CryptoOperation::RotateKeys { .. } => "rotate_keys".to_string(),
            CryptoOperation::Encrypt { .. } => "encrypt".to_string(),
            CryptoOperation::Decrypt { .. } => "decrypt".to_string(),
            CryptoOperation::Hash { .. } => "hash".to_string(),
        }
    }
}

/// Configuration for timing protection middleware
#[derive(Debug, Clone)]
pub struct TimingConfig {
    /// Whether to enable constant-time protection
    pub enable_constant_time: bool,

    /// Whether to add random jitter to timing
    pub enable_jitter: bool,

    /// Whether to monitor all operations for timing
    pub monitor_all_operations: bool,

    /// Whether to protect verification operations
    pub protect_verification: bool,

    /// Whether to detect timing anomalies
    pub detect_timing_anomalies: bool,

    /// Whether to fail operations on timing anomalies
    pub fail_on_anomalies: bool,

    /// Anomaly detection threshold (relative deviation)
    pub anomaly_threshold: f64,

    /// Target timing for different operations
    pub target_key_derivation_time: Duration,
    pub target_signature_time: Duration,
    pub target_verification_time: Duration,
    pub target_encryption_time: Duration,
    pub target_decryption_time: Duration,
    pub target_key_rotation_time: Duration,
}

impl Default for TimingConfig {
    fn default() -> Self {
        Self {
            enable_constant_time: true,
            enable_jitter: true,
            monitor_all_operations: false,
            protect_verification: false, // Verification timing is typically less sensitive
            detect_timing_anomalies: true,
            fail_on_anomalies: false,
            anomaly_threshold: 0.5, // 50% deviation threshold
            target_key_derivation_time: Duration::from_millis(10),
            target_signature_time: Duration::from_millis(5),
            target_verification_time: Duration::from_millis(2),
            target_encryption_time: Duration::from_millis(5),
            target_decryption_time: Duration::from_millis(5),
            target_key_rotation_time: Duration::from_millis(50),
        }
    }
}

/// Timing information for an operation
#[derive(Debug, Clone)]
struct TimingInfo {
    execution_time: Duration,
    success: bool,
    #[allow(dead_code)] // Stored for temporal analysis and debugging
    timestamp: u64,
}

/// Timing analyzer for detecting patterns and anomalies
struct TimingAnalyzer {
    timing_data: HashMap<String, Vec<TimingInfo>>,
    total_operations: u64,
    protected_operations: u64,
    anomalies_detected: u64,
}

impl TimingAnalyzer {
    fn new() -> Self {
        Self {
            timing_data: HashMap::new(),
            total_operations: 0,
            protected_operations: 0,
            anomalies_detected: 0,
        }
    }

    fn record_timing(&mut self, operation_type: String, execution_time: Duration, success: bool) {
        #[allow(clippy::disallowed_methods)] // [VERIFIED] Acceptable in timing data recording
        let timestamp = std::time::SystemTime::now();
        let timing_info = TimingInfo {
            execution_time,
            success,
            timestamp: timestamp
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        };

        let operation_type_clone = operation_type.clone();
        self.timing_data
            .entry(operation_type)
            .or_insert_with(Vec::new)
            .push(timing_info);

        self.total_operations += 1;

        // Keep only recent timing data to prevent memory growth
        if let Some(timings) = self.timing_data.get_mut(&operation_type_clone) {
            if timings.len() > 1000 {
                timings.drain(..500); // Keep only the latest 500 entries
            }
        }
    }

    fn get_baseline_timing(&self, operation_type: &str) -> Option<Duration> {
        if let Some(timings) = self.timing_data.get(operation_type) {
            if timings.len() < 5 {
                return None; // Need at least 5 samples for baseline
            }

            // Calculate median timing as baseline
            let mut times: Vec<Duration> = timings
                .iter()
                .filter(|t| t.success) // Only use successful operations
                .map(|t| t.execution_time)
                .collect();

            if times.is_empty() {
                return None;
            }

            times.sort();
            Some(times[times.len() / 2])
        } else {
            None
        }
    }

    fn analyze_patterns(&self, _config: &TimingConfig) -> TimingAnalysis {
        let mut operation_analysis = HashMap::new();

        for (operation_type, timings) in &self.timing_data {
            if timings.len() < 10 {
                continue; // Need sufficient data for analysis
            }

            let successful_timings: Vec<Duration> = timings
                .iter()
                .filter(|t| t.success)
                .map(|t| t.execution_time)
                .collect();

            if successful_timings.is_empty() {
                continue;
            }

            let mean =
                successful_timings.iter().sum::<Duration>() / successful_timings.len() as u32;

            let variance = successful_timings
                .iter()
                .map(|&time| {
                    let diff = if time > mean {
                        time - mean
                    } else {
                        mean - time
                    };
                    diff.as_nanos() as f64
                })
                .map(|diff| diff * diff)
                .sum::<f64>()
                / successful_timings.len() as f64;

            let std_dev = variance.sqrt();
            let coefficient_of_variation = std_dev / mean.as_nanos() as f64;

            let min_time = *successful_timings.iter().min().unwrap();
            let max_time = *successful_timings.iter().max().unwrap();

            operation_analysis.insert(
                operation_type.clone(),
                OperationTimingAnalysis {
                    sample_count: successful_timings.len(),
                    mean_time: mean,
                    min_time,
                    max_time,
                    std_deviation: Duration::from_nanos(std_dev as u64),
                    coefficient_of_variation,
                    constant_time_score: 1.0 - coefficient_of_variation.min(1.0),
                },
            );
        }

        TimingAnalysis {
            total_operations: self.total_operations,
            protected_operations: self.protected_operations,
            anomalies_detected: self.anomalies_detected,
            operation_analysis,
        }
    }

    fn stats(&self) -> TimingStats {
        TimingStats {
            total_operations: self.total_operations,
            protected_operations: self.protected_operations,
            anomalies_detected: self.anomalies_detected,
            operation_types: self.timing_data.len(),
        }
    }
}

/// Analysis of timing patterns
#[derive(Debug, Clone)]
pub struct TimingAnalysis {
    pub total_operations: u64,
    pub protected_operations: u64,
    pub anomalies_detected: u64,
    pub operation_analysis: HashMap<String, OperationTimingAnalysis>,
}

/// Timing analysis for a specific operation type
#[derive(Debug, Clone)]
pub struct OperationTimingAnalysis {
    pub sample_count: usize,
    pub mean_time: Duration,
    pub min_time: Duration,
    pub max_time: Duration,
    pub std_deviation: Duration,
    pub coefficient_of_variation: f64,
    pub constant_time_score: f64, // 0.0 to 1.0, higher is better
}

/// Timing protection statistics
#[derive(Debug, Clone)]
pub struct TimingStats {
    /// Total operations processed
    pub total_operations: u64,

    /// Operations with timing protection applied
    pub protected_operations: u64,

    /// Timing anomalies detected
    pub anomalies_detected: u64,

    /// Number of different operation types
    pub operation_types: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleware::handler::NoOpHandler;
    use crate::middleware::SecurityLevel;
    use aura_crypto::Effects;
    use aura_types::{AccountIdExt, DeviceIdExt};

    #[test]
    fn test_timing_protection_middleware() {
        let effects = Effects::test(42);
        let account_id = aura_types::AccountId::new_with_effects(&effects);
        let device_id = aura_types::DeviceId::new_with_effects(&effects);

        let config = TimingConfig {
            enable_constant_time: false, // Disable for testing
            ..TimingConfig::default()
        };
        let middleware = TimingProtectionMiddleware::new(config);
        let handler = NoOpHandler;
        let context = CryptoContext::new(
            account_id,
            device_id,
            "test".to_string(),
            SecurityLevel::High,
        );
        let operation = CryptoOperation::DeriveKey {
            app_id: "test".to_string(),
            context: "test".to_string(),
            derivation_path: vec![],
        };

        let result = middleware.process(operation, &context, &handler);
        assert!(result.is_ok());

        let stats = middleware.stats();
        assert_eq!(stats.total_operations, 1);
    }

    #[test]
    fn test_timing_protection_requirements() {
        let middleware = TimingProtectionMiddleware::new(TimingConfig::default());

        // Operations that require timing protection
        assert!(
            middleware.requires_timing_protection(&CryptoOperation::DeriveKey {
                app_id: "test".to_string(),
                context: "test".to_string(),
                derivation_path: vec![],
            })
        );

        assert!(
            middleware.requires_timing_protection(&CryptoOperation::GenerateSignature {
                message: vec![],
                signing_package: vec![],
            })
        );

        assert!(
            middleware.requires_timing_protection(&CryptoOperation::Encrypt {
                plaintext: vec![],
                recipient_keys: vec![],
            })
        );

        // Operations that don't require timing protection
        assert!(!middleware
            .requires_timing_protection(&CryptoOperation::GenerateRandom { num_bytes: 32 }));

        assert!(
            !middleware.requires_timing_protection(&CryptoOperation::Hash {
                data: vec![],
                algorithm: "blake3".to_string(),
            })
        );
    }

    #[test]
    fn test_constant_time_delay() {
        let middleware = TimingProtectionMiddleware::new(TimingConfig::default());

        let start = Instant::now();
        middleware.constant_time_delay(Duration::from_millis(10));
        let elapsed = start.elapsed();

        // Should take at least the requested delay time
        assert!(elapsed >= Duration::from_millis(9)); // Allow for some timing variance
        assert!(elapsed <= Duration::from_millis(20)); // But not too much
    }

    #[test]
    fn test_timing_analysis() {
        let middleware = TimingProtectionMiddleware::new(TimingConfig::default());

        // Record some timing data
        let _ = middleware.record_timing(
            &CryptoOperation::GenerateRandom { num_bytes: 32 },
            Duration::from_millis(5),
            true,
        );

        let _ = middleware.record_timing(
            &CryptoOperation::GenerateRandom { num_bytes: 32 },
            Duration::from_millis(6),
            true,
        );

        let analysis = middleware.analyze_timing_patterns().unwrap();
        assert!(analysis.total_operations >= 2);
    }
}
