//! Replica Placement Strategy Abstraction
//!
//! Unified interface for selecting which peers should receive data replicas.
//! Supports both static (predetermined) and social (trust-based) strategies.
//!
//! The placement strategy determines:
//! - Which peers receive replicas
//! - How many replicas are created
//! - Fallback behavior when preferred peers are unavailable
//! - Replica rotation and replacement policies

use std::fmt;

/// Strategy for placing replicas across peers
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlacementStrategy {
    /// Predetermined set of peers with optional priority ordering
    Static {
        /// Number of replicas to maintain
        replication_factor: usize,
        /// Whether to allow fewer replicas if some peers are down
        allow_degraded: bool,
    },
    /// Trust-based peer selection using social graph
    Social {
        /// Minimum trust level required for peer selection
        min_trust_threshold: u32,
        /// Number of replicas to maintain
        replication_factor: usize,
        /// Whether to prefer geographically diverse peers
        prefer_diverse_locations: bool,
    },
    /// Hybrid: static core replicas + social backup peers
    Hybrid {
        /// Number of static replicas (must have)
        static_factor: usize,
        /// Number of social backup replicas (if static unavailable)
        social_factor: usize,
        /// Minimum trust level for social peers
        social_min_trust: u32,
    },
}

impl fmt::Display for PlacementStrategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlacementStrategy::Static {
                replication_factor,
                allow_degraded,
            } => {
                write!(
                    f,
                    "Static({} replicas, degraded={})",
                    replication_factor, allow_degraded
                )
            }
            PlacementStrategy::Social {
                min_trust_threshold,
                replication_factor,
                prefer_diverse_locations,
            } => {
                write!(
                    f,
                    "Social({} replicas, min_trust={}, diverse={})",
                    replication_factor, min_trust_threshold, prefer_diverse_locations
                )
            }
            PlacementStrategy::Hybrid {
                static_factor,
                social_factor,
                social_min_trust,
            } => {
                write!(
                    f,
                    "Hybrid(static={}, social={}, min_trust={})",
                    static_factor, social_factor, social_min_trust
                )
            }
        }
    }
}

/// Configuration for a replica placement decision
#[derive(Clone, Debug)]
pub struct PlacementConfig {
    /// The strategy to use
    pub strategy: PlacementStrategy,
    /// Maximum latency acceptable for replica peers (ms)
    pub max_latency_ms: u32,
    /// Whether to prefer peers with lower cost
    pub prefer_low_cost: bool,
    /// Minimum redundancy level (1 = single copy, 2 = dual copy, etc)
    pub min_redundancy: usize,
}

impl Default for PlacementConfig {
    fn default() -> Self {
        Self {
            strategy: PlacementStrategy::Social {
                min_trust_threshold: 50,
                replication_factor: 3,
                prefer_diverse_locations: true,
            },
            max_latency_ms: 500,
            prefer_low_cost: false,
            min_redundancy: 2,
        }
    }
}

/// Result of a placement decision
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlacementResult {
    /// Selected peer IDs for replica placement
    pub peers: Vec<String>,
    /// Strategy that was used
    pub strategy_used: PlacementStrategy,
    /// Number of replicas successfully placed
    pub replica_count: usize,
    /// Whether all desired replicas were placed
    pub is_satisfied: bool,
    /// Reason if placement is unsatisfied
    pub status: PlacementStatus,
}

/// Status of a replica placement attempt
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlacementStatus {
    /// All replicas successfully placed
    Satisfied,
    /// All required replicas placed, but fewer than desired
    PartiallyDegraded,
    /// Insufficient peers available
    InsufficientPeers,
    /// Could not meet minimum trust threshold
    InsufficientTrust,
    /// All static peers are down (in hybrid mode)
    StaticPeerFailure,
    /// Configuration error
    ConfigurationError(String),
}

impl fmt::Display for PlacementStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PlacementStatus::Satisfied => write!(f, "Satisfied"),
            PlacementStatus::PartiallyDegraded => write!(f, "PartiallyDegraded"),
            PlacementStatus::InsufficientPeers => write!(f, "InsufficientPeers"),
            PlacementStatus::InsufficientTrust => write!(f, "InsufficientTrust"),
            PlacementStatus::StaticPeerFailure => write!(f, "StaticPeerFailure"),
            PlacementStatus::ConfigurationError(msg) => write!(f, "ConfigurationError: {}", msg),
        }
    }
}

/// Replica placement decision engine
pub struct ReplicaPlacementEngine {
    config: PlacementConfig,
    decision_history: Vec<PlacementResult>,
}

impl ReplicaPlacementEngine {
    /// Create a new placement engine with default config
    pub fn new() -> Self {
        Self::with_config(PlacementConfig::default())
    }

    /// Create with custom configuration
    pub fn with_config(config: PlacementConfig) -> Self {
        Self {
            config,
            decision_history: Vec::new(),
        }
    }

    /// Get current configuration
    pub fn config(&self) -> &PlacementConfig {
        &self.config
    }

    /// Update configuration
    pub fn set_config(&mut self, config: PlacementConfig) {
        self.config = config;
    }

    /// Make a placement decision (normally would consult peer managers)
    /// For now, this is a placeholder that demonstrates the decision logic
    pub fn decide_placement(&mut self, _available_peers: &[String]) -> PlacementResult {
        // In real implementation, this would:
        // 1. Query current peer states
        // 2. Filter by strategy requirements
        // 3. Score remaining candidates
        // 4. Return ordered list of selected peers

        let status = PlacementStatus::Satisfied;
        let result = PlacementResult {
            peers: Vec::new(),
            strategy_used: self.config.strategy.clone(),
            replica_count: 0,
            is_satisfied: status == PlacementStatus::Satisfied,
            status,
        };

        self.decision_history.push(result.clone());
        result
    }

    /// Get recent placement decisions (last 20)
    pub fn get_decision_history(&self) -> Vec<PlacementResult> {
        self.decision_history
            .iter()
            .rev()
            .take(20)
            .cloned()
            .collect()
    }

    /// Get satisfaction rate from history
    pub fn get_satisfaction_rate(&self) -> f64 {
        if self.decision_history.is_empty() {
            return 0.0;
        }
        let satisfied = self
            .decision_history
            .iter()
            .filter(|r| r.is_satisfied)
            .count();
        satisfied as f64 / self.decision_history.len() as f64
    }

    /// Check if current config is valid
    pub fn validate_config(&self) -> Result<(), String> {
        match &self.config.strategy {
            PlacementStrategy::Static {
                replication_factor, ..
            } => {
                if *replication_factor == 0 {
                    return Err("replication_factor must be > 0".to_string());
                }
            }
            PlacementStrategy::Social {
                replication_factor,
                min_trust_threshold,
                ..
            } => {
                if *replication_factor == 0 {
                    return Err("replication_factor must be > 0".to_string());
                }
                if *min_trust_threshold > 100 {
                    return Err("min_trust_threshold must be <= 100".to_string());
                }
            }
            PlacementStrategy::Hybrid {
                static_factor,
                social_factor,
                social_min_trust,
            } => {
                if *static_factor == 0 && *social_factor == 0 {
                    return Err(
                        "at least one of static_factor or social_factor must be > 0".to_string()
                    );
                }
                if *social_min_trust > 100 {
                    return Err("social_min_trust must be <= 100".to_string());
                }
            }
        }

        if self.config.min_redundancy == 0 {
            return Err("min_redundancy must be > 0".to_string());
        }

        Ok(())
    }
}

impl Default for ReplicaPlacementEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_placement_strategy_display() {
        let static_strategy = PlacementStrategy::Static {
            replication_factor: 3,
            allow_degraded: false,
        };
        assert!(static_strategy.to_string().contains("Static"));

        let social_strategy = PlacementStrategy::Social {
            min_trust_threshold: 60,
            replication_factor: 3,
            prefer_diverse_locations: true,
        };
        assert!(social_strategy.to_string().contains("Social"));

        let hybrid_strategy = PlacementStrategy::Hybrid {
            static_factor: 2,
            social_factor: 2,
            social_min_trust: 50,
        };
        assert!(hybrid_strategy.to_string().contains("Hybrid"));
    }

    #[test]
    fn test_placement_config_default() {
        let config = PlacementConfig::default();
        assert_eq!(config.min_redundancy, 2);
        assert_eq!(config.max_latency_ms, 500);
    }

    #[test]
    fn test_placement_engine_creation() {
        let engine = ReplicaPlacementEngine::new();
        assert_eq!(engine.get_satisfaction_rate(), 0.0);
    }

    #[test]
    fn test_placement_engine_custom_config() {
        let config = PlacementConfig {
            strategy: PlacementStrategy::Static {
                replication_factor: 5,
                allow_degraded: true,
            },
            max_latency_ms: 1000,
            prefer_low_cost: true,
            min_redundancy: 3,
        };
        let engine = ReplicaPlacementEngine::with_config(config.clone());
        assert_eq!(engine.config().max_latency_ms, 1000);
    }

    #[test]
    fn test_placement_engine_validate_config_valid() {
        let config = PlacementConfig::default();
        let engine = ReplicaPlacementEngine::with_config(config);
        assert!(engine.validate_config().is_ok());
    }

    #[test]
    fn test_placement_engine_validate_config_invalid_replication() {
        let config = PlacementConfig {
            strategy: PlacementStrategy::Static {
                replication_factor: 0,
                allow_degraded: false,
            },
            ..Default::default()
        };
        let engine = ReplicaPlacementEngine::with_config(config);
        assert!(engine.validate_config().is_err());
    }

    #[test]
    fn test_placement_engine_validate_config_invalid_trust_threshold() {
        let config = PlacementConfig {
            strategy: PlacementStrategy::Social {
                min_trust_threshold: 150,
                replication_factor: 3,
                prefer_diverse_locations: false,
            },
            ..Default::default()
        };
        let engine = ReplicaPlacementEngine::with_config(config);
        assert!(engine.validate_config().is_err());
    }

    #[test]
    fn test_placement_status_display() {
        assert_eq!(PlacementStatus::Satisfied.to_string(), "Satisfied");
        assert_eq!(
            PlacementStatus::PartiallyDegraded.to_string(),
            "PartiallyDegraded"
        );
        assert_eq!(
            PlacementStatus::InsufficientPeers.to_string(),
            "InsufficientPeers"
        );
    }

    #[test]
    fn test_placement_result_equality() {
        let result1 = PlacementResult {
            peers: vec!["peer1".to_string()],
            strategy_used: PlacementStrategy::Static {
                replication_factor: 1,
                allow_degraded: false,
            },
            replica_count: 1,
            is_satisfied: true,
            status: PlacementStatus::Satisfied,
        };

        let result2 = result1.clone();
        assert_eq!(result1, result2);
    }
}
