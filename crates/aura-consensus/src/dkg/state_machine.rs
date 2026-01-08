//! DKG collection state machine.
//!
//! Owns package collection and aggregation decisions, leaving transcript
//! finalization and consensus commit to the caller.

use super::ceremony::aggregate_dkg_transcript;
use super::types::{DealerPackage, DkgConfig, DkgTranscript};
use super::verifier::verify_dealer_package;
use aura_core::{AuraError, AuthorityId, Result};
use std::collections::BTreeMap;

/// Phase of the DKG state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DkgPhase {
    /// Collecting dealer packages.
    Collecting,
    /// Packages aggregated into a transcript.
    Aggregated,
}

/// Aggregation mode used to finalize a transcript.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DkgAggregationMode {
    /// Full quorum of packages (N-of-N or max_signers) collected.
    FullQuorum,
    /// Threshold fallback: proceeded with >= threshold packages.
    ThresholdFallback,
}

/// Outcome of a package collection step.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DkgCollectionUpdate {
    pub accepted: bool,
    pub total_packages: usize,
    pub threshold_met: bool,
}

/// Aggregation result from collected packages.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DkgAggregationResult {
    pub transcript: DkgTranscript,
    pub mode: DkgAggregationMode,
    pub package_count: usize,
}

/// State machine for collecting and aggregating DKG dealer packages.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DkgCollectionState {
    config: DkgConfig,
    packages: BTreeMap<AuthorityId, DealerPackage>,
    phase: DkgPhase,
}

impl DkgCollectionState {
    pub fn new(config: DkgConfig) -> Self {
        Self {
            config,
            packages: BTreeMap::new(),
            phase: DkgPhase::Collecting,
        }
    }

    pub fn config(&self) -> &DkgConfig {
        &self.config
    }

    pub fn phase(&self) -> DkgPhase {
        self.phase
    }

    pub fn package_count(&self) -> usize {
        self.packages.len()
    }

    /// Record a dealer package, validating it against the config.
    pub fn record_package(&mut self, package: DealerPackage) -> Result<DkgCollectionUpdate> {
        if self.phase != DkgPhase::Collecting {
            return Err(AuraError::invalid("DKG state not collecting packages"));
        }

        verify_dealer_package(&package)?;

        // Ensure shares cover all configured participants.
        for participant in &self.config.participants {
            if !package.encrypted_shares.contains_key(participant) {
                return Err(AuraError::invalid(
                    "Dealer package missing participant share",
                ));
            }
        }

        let accepted = self.packages.insert(package.dealer, package).is_none();
        let total_packages = self.packages.len();
        let threshold_met = total_packages >= self.config.threshold as usize;

        Ok(DkgCollectionUpdate {
            accepted,
            total_packages,
            threshold_met,
        })
    }

    /// Aggregate collected packages into a DKG transcript.
    ///
    /// This permits threshold fallback: if at least `threshold` packages are present,
    /// the transcript can be aggregated even if fewer than all expected packages
    /// have arrived.
    pub fn aggregate(&mut self) -> Result<DkgAggregationResult> {
        if self.phase != DkgPhase::Collecting {
            return Err(AuraError::invalid("DKG state already aggregated"));
        }

        let mut packages: Vec<DealerPackage> = self.packages.values().cloned().collect();

        if packages.len() < self.config.threshold as usize {
            return Err(AuraError::invalid(
                "DKG aggregation requires at least threshold packages",
            ));
        }

        let max_signers = self.config.max_signers as usize;
        if packages.len() > max_signers {
            packages.truncate(max_signers);
        }

        let mode = if packages.len() >= self.config.participants.len() {
            DkgAggregationMode::FullQuorum
        } else {
            DkgAggregationMode::ThresholdFallback
        };

        let transcript = aggregate_dkg_transcript(&self.config, packages.clone())?;
        self.phase = DkgPhase::Aggregated;

        Ok(DkgAggregationResult {
            transcript,
            mode,
            package_count: packages.len(),
        })
    }
}
