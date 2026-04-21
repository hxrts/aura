use super::RunnerConfig;
use std::time::Duration;

/// Verification statistics tracking.
#[derive(Debug, Clone, Default)]
pub struct VerificationStatistics {
    /// Total properties verified.
    pub total_properties: u64,
    /// Cache hits.
    pub cache_hits: u64,
    /// Cache misses.
    pub cache_misses: u64,
    /// Total verification time.
    pub total_time: Duration,
    /// Number of counterexamples found.
    pub counterexamples_found: u64,
    /// Number of successful verifications.
    pub successful_verifications: u64,
}

/// System diagnostics information.
#[derive(Debug, Clone)]
pub struct SystemDiagnostics {
    /// Runner version.
    pub runner_version: String,
    /// Cache information.
    pub cache_info: CacheInfo,
    /// System capabilities.
    pub capabilities: SystemCapabilities,
}

/// Cache information.
#[derive(Debug, Clone)]
pub struct CacheInfo {
    /// Current cache size.
    pub size: usize,
    /// Maximum cache size.
    pub max_size: usize,
    /// Cache hit rate.
    pub hit_rate: f64,
}

/// System capabilities.
#[derive(Debug, Clone)]
pub struct SystemCapabilities {
    /// Counterexample generation enabled.
    pub counterexample_generation: bool,
    /// Trace normalization enabled.
    pub trace_optimization: bool,
    /// Parallel execution enabled.
    pub parallel_execution: bool,
    /// Caching enabled.
    pub caching: bool,
    /// Aura integration enabled.
    pub aura_integration: bool,
}

/// System health check result.
#[derive(Debug, Clone)]
pub struct SystemHealth {
    /// Overall health status.
    pub overall_status: HealthStatus,
    /// Individual health checks.
    pub checks: Vec<HealthCheck>,
    /// Recommendations for improvement.
    pub recommendations: Vec<String>,
    /// Check timestamp.
    pub timestamp: u64,
}

/// Individual health check.
#[derive(Debug, Clone)]
pub struct HealthCheck {
    /// Check name.
    pub name: String,
    /// Check status.
    pub status: HealthStatus,
    /// Status message.
    pub message: String,
}

/// Health status.
#[derive(Debug, Clone, PartialEq)]
pub enum HealthStatus {
    Ok,
    Warning,
    Error,
}

impl SystemCapabilities {
    pub(crate) fn from_config(config: &RunnerConfig) -> Self {
        Self {
            counterexample_generation: config.generate_counterexamples,
            trace_optimization: config.optimize_traces,
            parallel_execution: config.enable_parallel,
            caching: config.enable_caching,
            aura_integration: config.verify_capability_soundness || config.verify_privacy_contracts,
        }
    }
}

pub(crate) fn cache_hit_rate(cache_hits: u64, cache_misses: u64) -> f64 {
    let total = cache_hits.saturating_add(cache_misses);
    if total == 0 {
        0.0
    } else {
        cache_hits as f64 / total as f64
    }
}
