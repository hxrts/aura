//! Unified fault schema for simulation, replay, and chaos injection.
//!
//! This module provides a canonical fault model aligned with Telltale-style typed faults while
//! preserving compatibility with Aura's existing simulation fault enums.

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Schema version for [`AuraFault`].
pub const AURA_FAULT_SCHEMA_V1: &str = "aura.fault.v1";

/// Directed edge used by communication fault variants.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FaultEdge {
    /// Optional session identifier.
    pub session: Option<String>,
    /// Sender role/node.
    pub from: String,
    /// Receiver role/node.
    pub to: String,
}

impl FaultEdge {
    /// Build an edge descriptor without session scope.
    #[must_use]
    pub fn new(from: impl Into<String>, to: impl Into<String>) -> Self {
        Self {
            session: None,
            from: from.into(),
            to: to.into(),
        }
    }

    /// Attach session scope.
    #[must_use]
    pub fn with_session(mut self, session: impl Into<String>) -> Self {
        self.session = Some(session.into());
        self
    }
}

/// Corruption mode for payload mutation faults.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CorruptionMode {
    /// Flip random bits.
    BitFlip,
    /// Truncate payload.
    Truncation,
    /// Duplicate payload segments.
    Duplication,
    /// Insert random bytes.
    Insertion,
    /// Reorder bytes/chunks.
    Reordering,
    /// Generic/opaque corruption mode.
    Opaque,
}

/// Canonical fault variants used across simulator/runtime harnesses.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AuraFaultKind {
    /// Probabilistic message drop on one edge.
    MessageDrop {
        /// Affected communication edge.
        edge: FaultEdge,
        /// Drop probability in `[0.0, 1.0]`.
        probability: f64,
    },
    /// Message delay fault on one edge.
    MessageDelay {
        /// Affected communication edge.
        edge: FaultEdge,
        /// Minimum delay.
        min: Duration,
        /// Maximum delay.
        max: Duration,
    },
    /// Message corruption fault on one edge.
    MessageCorruption {
        /// Affected communication edge.
        edge: FaultEdge,
        /// Corruption mode.
        mode: CorruptionMode,
    },
    /// Node crash fault.
    NodeCrash {
        /// Node identifier.
        node: String,
        /// Optional activation tick.
        at_tick: Option<u64>,
        /// Optional crash duration.
        duration: Option<Duration>,
    },
    /// Network partition fault.
    NetworkPartition {
        /// Partition groups.
        partition: Vec<Vec<String>>,
        /// Optional duration.
        duration: Option<Duration>,
    },
    /// Aura-specific extension: force flow budget exhaustion.
    FlowBudgetExhaustion {
        /// Optional context identifier.
        context: Option<String>,
        /// Severity multiplier (`>= 1.0`).
        factor: f64,
    },
    /// Aura-specific extension: journal corruption injection.
    JournalCorruption {
        /// Logical domain or journal label.
        domain: String,
        /// Corruption probability in `[0.0, 1.0]`.
        probability: f64,
    },
    /// Fallback compatibility envelope for legacy stringly faults.
    Legacy {
        /// Legacy type identifier.
        fault_type: String,
        /// Optional textual payload.
        detail: Option<String>,
    },
}

/// Canonical fault envelope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuraFault {
    /// Schema identifier.
    pub schema_version: String,
    /// Fault payload.
    pub fault: AuraFaultKind,
}

impl AuraFault {
    /// Build a canonical fault envelope.
    #[must_use]
    pub fn new(fault: AuraFaultKind) -> Self {
        Self {
            schema_version: AURA_FAULT_SCHEMA_V1.to_string(),
            fault,
        }
    }
}

impl From<AuraFaultKind> for AuraFault {
    fn from(fault: AuraFaultKind) -> Self {
        Self::new(fault)
    }
}

#[cfg(feature = "simulation")]
impl From<crate::effects::simulation::FaultType> for AuraFaultKind {
    fn from(value: crate::effects::simulation::FaultType) -> Self {
        use crate::effects::simulation::{
            ByzantineFault, ComputationFault, FaultType, NetworkFault, StorageFault, TimeFault,
        };

        match value {
            FaultType::Network(network) => match network {
                NetworkFault::Partition { groups } => Self::NetworkPartition {
                    partition: groups,
                    duration: None,
                },
                NetworkFault::PacketLoss { probability } => Self::MessageDrop {
                    edge: FaultEdge::new("*", "*"),
                    probability,
                },
                NetworkFault::Latency { delay } => Self::MessageDelay {
                    edge: FaultEdge::new("*", "*"),
                    min: delay,
                    max: delay,
                },
                NetworkFault::Corruption { probability } => Self::MessageCorruption {
                    edge: FaultEdge::new("*", "*"),
                    mode: if probability > 0.0 {
                        CorruptionMode::Opaque
                    } else {
                        CorruptionMode::BitFlip
                    },
                },
                NetworkFault::Congestion { bandwidth_limit } => Self::Legacy {
                    fault_type: "network_congestion".to_string(),
                    detail: Some(format!("bandwidth_limit={bandwidth_limit}")),
                },
                NetworkFault::Outage => Self::NodeCrash {
                    node: "*".to_string(),
                    at_tick: None,
                    duration: None,
                },
            },
            FaultType::Storage(storage) => match storage {
                StorageFault::Failure { probability } => Self::Legacy {
                    fault_type: "storage_failure".to_string(),
                    detail: Some(format!("probability={probability}")),
                },
                StorageFault::Corruption { probability } => Self::JournalCorruption {
                    domain: "storage".to_string(),
                    probability,
                },
                StorageFault::CapacityExhausted => Self::Legacy {
                    fault_type: "storage_capacity_exhausted".to_string(),
                    detail: None,
                },
                StorageFault::Unavailable => Self::Legacy {
                    fault_type: "storage_unavailable".to_string(),
                    detail: None,
                },
                StorageFault::Slowness { delay } => Self::Legacy {
                    fault_type: "storage_slowness".to_string(),
                    detail: Some(format!("delay_ms={}", delay.as_millis())),
                },
            },
            FaultType::Computation(computation) => match computation {
                ComputationFault::MemoryExhaustion => Self::FlowBudgetExhaustion {
                    context: None,
                    factor: 1.5,
                },
                ComputationFault::CpuSlowness { factor } => Self::Legacy {
                    fault_type: "cpu_slowness".to_string(),
                    detail: Some(format!("factor={factor}")),
                },
                ComputationFault::ResultCorruption { probability } => Self::Legacy {
                    fault_type: "result_corruption".to_string(),
                    detail: Some(format!("probability={probability}")),
                },
                ComputationFault::Timeout { duration } => Self::Legacy {
                    fault_type: "timeout".to_string(),
                    detail: Some(format!("duration_ms={}", duration.as_millis())),
                },
            },
            FaultType::Time(time_fault) => match time_fault {
                TimeFault::ClockDrift { rate } => Self::Legacy {
                    fault_type: "clock_drift".to_string(),
                    detail: Some(format!("rate={rate}")),
                },
                TimeFault::ClockSkew { offset } => Self::Legacy {
                    fault_type: "clock_skew".to_string(),
                    detail: Some(format!("offset_ms={}", offset.as_millis())),
                },
                TimeFault::TimeJump { delta } => Self::Legacy {
                    fault_type: "time_jump".to_string(),
                    detail: Some(format!("delta_ms={}", delta.as_millis())),
                },
            },
            FaultType::Byzantine(byzantine) => match byzantine {
                ByzantineFault::Equivocation => Self::MessageCorruption {
                    edge: FaultEdge::new("*", "*"),
                    mode: CorruptionMode::Opaque,
                },
                ByzantineFault::InvalidSignatures => Self::Legacy {
                    fault_type: "invalid_signatures".to_string(),
                    detail: None,
                },
                ByzantineFault::Silence => Self::MessageDrop {
                    edge: FaultEdge::new("*", "*"),
                    probability: 1.0,
                },
                ByzantineFault::ProtocolViolation => Self::Legacy {
                    fault_type: "protocol_violation".to_string(),
                    detail: None,
                },
            },
        }
    }
}

#[cfg(feature = "simulation")]
impl From<crate::effects::chaos::CorruptionType> for CorruptionMode {
    fn from(value: crate::effects::chaos::CorruptionType) -> Self {
        match value {
            crate::effects::chaos::CorruptionType::BitFlip => Self::BitFlip,
            crate::effects::chaos::CorruptionType::Truncation => Self::Truncation,
            crate::effects::chaos::CorruptionType::Duplication => Self::Duplication,
            crate::effects::chaos::CorruptionType::Insertion => Self::Insertion,
            crate::effects::chaos::CorruptionType::Reordering => Self::Reordering,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edge_builder_sets_session_scope() {
        let edge = FaultEdge::new("A", "B").with_session("sid-1");
        assert_eq!(edge.session.as_deref(), Some("sid-1"));
    }

    #[test]
    fn fault_envelope_sets_schema_version() {
        let fault = AuraFault::new(AuraFaultKind::Legacy {
            fault_type: "legacy".to_string(),
            detail: None,
        });
        assert_eq!(fault.schema_version, AURA_FAULT_SCHEMA_V1);
    }
}
