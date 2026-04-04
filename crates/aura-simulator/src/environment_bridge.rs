//! Aura-owned bridge from simulator-local state into Telltale 11-style
//! environment concepts.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Environment-facing mobility profile derived from adaptive-privacy movement state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuraMobilityProfile {
    pub profile_id: String,
    pub clusters: Vec<String>,
    pub home_locality_bias_millis: u64,
    pub neighborhood_locality_bias_millis: u64,
    pub recorded_at_tick: u64,
}

/// Environment-facing link-admission observation derived from sync opportunities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuraLinkAdmissionObservation {
    pub profile_id: String,
    pub density: String,
    pub peers: Vec<String>,
    pub recorded_at_tick: u64,
}

/// Environment-facing node-capability observation derived from provider saturation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuraNodeCapabilityObservation {
    pub provider: String,
    pub queue_depth: usize,
    pub utilization_per_mille: u64,
    pub recorded_at_tick: u64,
}

/// Snapshot of Aura environment concepts aligned to Telltale 11 vocabulary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AuraEnvironmentSnapshot {
    pub mobility_profiles: Vec<AuraMobilityProfile>,
    pub link_admissions: Vec<AuraLinkAdmissionObservation>,
    pub node_capabilities: Vec<AuraNodeCapabilityObservation>,
}

/// Trace entry for the Aura environment bridge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AuraEnvironmentTraceEntry {
    MobilityProfileConfigured(AuraMobilityProfile),
    LinkAdmissionObserved(AuraLinkAdmissionObservation),
    NodeCapabilityObserved(AuraNodeCapabilityObservation),
}

/// Deterministic trace over Aura environment bridge updates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AuraEnvironmentTrace {
    pub entries: Vec<AuraEnvironmentTraceEntry>,
}

/// Aura-owned environment bridge state.
#[derive(Debug, Clone, Default)]
pub struct AuraEnvironmentBridge {
    mobility_profiles: HashMap<String, AuraMobilityProfile>,
    link_admissions: HashMap<String, AuraLinkAdmissionObservation>,
    node_capabilities: HashMap<String, AuraNodeCapabilityObservation>,
    trace: AuraEnvironmentTrace,
}

impl AuraEnvironmentBridge {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_mobility_profile(&mut self, profile: AuraMobilityProfile) {
        self.mobility_profiles
            .insert(profile.profile_id.clone(), profile.clone());
        self.trace
            .entries
            .push(AuraEnvironmentTraceEntry::MobilityProfileConfigured(
                profile,
            ));
    }

    pub fn record_link_admission(&mut self, observation: AuraLinkAdmissionObservation) {
        self.link_admissions
            .insert(observation.profile_id.clone(), observation.clone());
        self.trace
            .entries
            .push(AuraEnvironmentTraceEntry::LinkAdmissionObserved(
                observation,
            ));
    }

    pub fn record_node_capability(&mut self, observation: AuraNodeCapabilityObservation) {
        self.node_capabilities
            .insert(observation.provider.clone(), observation.clone());
        self.trace
            .entries
            .push(AuraEnvironmentTraceEntry::NodeCapabilityObserved(
                observation,
            ));
    }

    #[must_use]
    pub fn snapshot(&self) -> AuraEnvironmentSnapshot {
        let mut mobility_profiles = self.mobility_profiles.values().cloned().collect::<Vec<_>>();
        mobility_profiles.sort_by(|left, right| left.profile_id.cmp(&right.profile_id));

        let mut link_admissions = self.link_admissions.values().cloned().collect::<Vec<_>>();
        link_admissions.sort_by(|left, right| left.profile_id.cmp(&right.profile_id));

        let mut node_capabilities = self.node_capabilities.values().cloned().collect::<Vec<_>>();
        node_capabilities.sort_by(|left, right| left.provider.cmp(&right.provider));

        AuraEnvironmentSnapshot {
            mobility_profiles,
            link_admissions,
            node_capabilities,
        }
    }

    #[must_use]
    pub fn trace(&self) -> &AuraEnvironmentTrace {
        &self.trace
    }
}
