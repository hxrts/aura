//! Replay helpers for persisted Telltale effect traces.

use std::fs;
use std::path::Path;
use std::sync::Arc;

use aura_core::AuraFault;
use serde::{Deserialize, Serialize};
use telltale_vm::{EffectTraceEntry, ReplayEffectHandler};

/// On-disk encoding for replay traces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReplayTraceEncoding {
    /// UTF-8 JSON array of `EffectTraceEntry`.
    #[default]
    Json,
    /// Binary CBOR payload of `EffectTraceEntry` array.
    Cbor,
}

/// Replay loading/saving/verification errors.
#[derive(Debug, thiserror::Error)]
pub enum ReplayTraceError {
    /// Failed to read or write one trace artifact file.
    #[error("replay trace IO error at {path}: {source}")]
    Io {
        /// Path that failed.
        path: String,
        /// Wrapped IO error.
        source: std::io::Error,
    },
    /// Trace JSON decode/encode failure.
    #[error("replay trace JSON serialization failed: {source}")]
    Json {
        /// Wrapped serde error.
        source: serde_json::Error,
    },
    /// Trace CBOR decode/encode failure.
    #[error("replay trace CBOR serialization failed: {source}")]
    Cbor {
        /// Wrapped serde error.
        source: serde_cbor::Error,
    },
    /// Fault replay callback rejected one fault.
    #[error("replay fault injection failed: {message}")]
    FaultReplay {
        /// Rejection reason.
        message: String,
    },
    /// Trace divergence against expected sequence.
    #[error(
        "replay divergence at index {index}: expected={expected_kind:?} actual={actual_kind:?}"
    )]
    Divergence {
        /// Divergence index.
        index: usize,
        /// Expected effect kind.
        expected_kind: Option<String>,
        /// Actual effect kind.
        actual_kind: Option<String>,
    },
}

/// Fault-aware replay bundle persisted in JSON/CBOR artifacts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReplayTraceBundle {
    /// Effect trace entries for replay handler execution.
    pub entries: Vec<EffectTraceEntry>,
    /// Canonical faults to re-inject before replay.
    #[serde(default)]
    pub faults: Vec<AuraFault>,
}

/// Shared replay trace container supporting `Arc<[...]>` for low-copy replay lanes.
#[derive(Debug, Clone)]
pub struct ReplayTrace {
    entries: Arc<[EffectTraceEntry]>,
    faults: Arc<[AuraFault]>,
}

impl ReplayTrace {
    /// Build from owned trace entries.
    #[must_use]
    pub fn from_entries(entries: Vec<EffectTraceEntry>) -> Self {
        Self {
            entries: Arc::from(entries),
            faults: Arc::from(Vec::<AuraFault>::new()),
        }
    }

    /// Build from entries and canonical faults.
    #[must_use]
    pub fn from_entries_and_faults(entries: Vec<EffectTraceEntry>, faults: Vec<AuraFault>) -> Self {
        Self {
            entries: Arc::from(entries),
            faults: Arc::from(faults),
        }
    }

    /// Build from a fault-aware bundle.
    #[must_use]
    pub fn from_bundle(bundle: ReplayTraceBundle) -> Self {
        Self::from_entries_and_faults(bundle.entries, bundle.faults)
    }

    /// Build a fault-aware bundle.
    #[must_use]
    pub fn to_bundle(&self) -> ReplayTraceBundle {
        ReplayTraceBundle {
            entries: self.as_slice().to_vec(),
            faults: self.faults(),
        }
    }

    /// Build from a shared trace buffer.
    #[must_use]
    pub fn from_shared(entries: Arc<[EffectTraceEntry]>) -> Self {
        Self {
            entries,
            faults: Arc::from(Vec::<AuraFault>::new()),
        }
    }

    /// Shared replay entries.
    #[must_use]
    pub fn shared(&self) -> Arc<[EffectTraceEntry]> {
        Arc::clone(&self.entries)
    }

    /// Borrow replay entries as a slice.
    #[must_use]
    pub fn as_slice(&self) -> &[EffectTraceEntry] {
        self.entries.as_ref()
    }

    /// Borrow replay faults as owned vector.
    #[must_use]
    pub fn faults(&self) -> Vec<AuraFault> {
        self.faults.as_ref().to_vec()
    }

    /// Number of entries in this replay trace.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True when there are no replay entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// True when a replay bundle carries pre-recorded faults.
    #[must_use]
    pub fn has_faults(&self) -> bool {
        !self.faults.is_empty()
    }

    /// Load a replay trace from file.
    ///
    /// # Errors
    ///
    /// Returns IO/serialization errors.
    pub fn load_file(
        path: impl AsRef<Path>,
        encoding: ReplayTraceEncoding,
    ) -> Result<Self, ReplayTraceError> {
        let path_ref = path.as_ref();
        let payload = fs::read(path_ref).map_err(|source| ReplayTraceError::Io {
            path: path_ref.display().to_string(),
            source,
        })?;
        match encoding {
            ReplayTraceEncoding::Json => {
                if let Ok(bundle) = serde_json::from_slice::<ReplayTraceBundle>(&payload) {
                    return Ok(Self::from_bundle(bundle));
                }
                let entries = serde_json::from_slice(&payload)
                    .map_err(|source| ReplayTraceError::Json { source })?;
                Ok(Self::from_entries(entries))
            }
            ReplayTraceEncoding::Cbor => {
                if let Ok(bundle) = serde_cbor::from_slice::<ReplayTraceBundle>(&payload) {
                    return Ok(Self::from_bundle(bundle));
                }
                let entries = serde_cbor::from_slice(&payload)
                    .map_err(|source| ReplayTraceError::Cbor { source })?;
                Ok(Self::from_entries(entries))
            }
        }
    }

    /// Persist a replay trace to file.
    ///
    /// # Errors
    ///
    /// Returns IO/serialization errors.
    pub fn save_file(
        &self,
        path: impl AsRef<Path>,
        encoding: ReplayTraceEncoding,
    ) -> Result<(), ReplayTraceError> {
        let path_ref = path.as_ref();
        let payload = match (encoding, self.has_faults()) {
            // Keep legacy array format when no faults are present.
            (ReplayTraceEncoding::Json, false) => serde_json::to_vec(self.as_slice())
                .map_err(|source| ReplayTraceError::Json { source })?,
            (ReplayTraceEncoding::Cbor, false) => serde_cbor::to_vec(&self.as_slice().to_vec())
                .map_err(|source| ReplayTraceError::Cbor { source })?,
            (ReplayTraceEncoding::Json, true) => serde_json::to_vec(&self.to_bundle())
                .map_err(|source| ReplayTraceError::Json { source })?,
            (ReplayTraceEncoding::Cbor, true) => serde_cbor::to_vec(&self.to_bundle())
                .map_err(|source| ReplayTraceError::Cbor { source })?,
        };
        fs::write(path_ref, payload).map_err(|source| ReplayTraceError::Io {
            path: path_ref.display().to_string(),
            source,
        })
    }

    /// Build a Telltale replay handler with fallback behavior.
    #[must_use]
    pub fn with_fallback<'a>(
        &'a self,
        fallback: &'a dyn telltale_vm::effect::EffectHandler,
    ) -> ReplayEffectHandler<'a> {
        ReplayEffectHandler::with_fallback(self.shared(), fallback)
    }

    /// Re-inject serialized faults before replaying effect entries.
    ///
    /// # Errors
    ///
    /// Returns [`ReplayTraceError::FaultReplay`] when the injector rejects a fault.
    pub fn replay_faults<F>(&self, mut injector: F) -> Result<(), ReplayTraceError>
    where
        F: FnMut(&AuraFault) -> Result<(), String>,
    {
        for fault in self.faults.as_ref() {
            injector(fault).map_err(|message| ReplayTraceError::FaultReplay { message })?;
        }
        Ok(())
    }
}

/// Stateful replay verifier for effect-sequence divergence detection.
#[derive(Debug, Clone)]
pub struct ReplayEffectSequence {
    expected: Arc<[EffectTraceEntry]>,
    cursor: usize,
}

impl ReplayEffectSequence {
    /// Create sequence verifier from shared replay trace.
    #[must_use]
    pub fn new(trace: &ReplayTrace) -> Self {
        Self {
            expected: trace.shared(),
            cursor: 0,
        }
    }

    /// Verify one observed effect entry against expected sequence.
    ///
    /// # Errors
    ///
    /// Returns divergence information when entries do not match.
    pub fn verify_next(&mut self, actual: &EffectTraceEntry) -> Result<(), ReplayTraceError> {
        let Some(expected) = self.expected.get(self.cursor) else {
            return Err(ReplayTraceError::Divergence {
                index: self.cursor,
                expected_kind: None,
                actual_kind: Some(actual.effect_kind.clone()),
            });
        };
        if expected != actual {
            return Err(ReplayTraceError::Divergence {
                index: self.cursor,
                expected_kind: Some(expected.effect_kind.clone()),
                actual_kind: Some(actual.effect_kind.clone()),
            });
        }
        self.cursor = self.cursor.saturating_add(1);
        Ok(())
    }

    /// Verify all expected entries were consumed.
    ///
    /// # Errors
    ///
    /// Returns divergence information when observed sequence ended early.
    pub fn finish(&self) -> Result<(), ReplayTraceError> {
        if self.cursor == self.expected.len() {
            return Ok(());
        }
        Err(ReplayTraceError::Divergence {
            index: self.cursor,
            expected_kind: self
                .expected
                .get(self.cursor)
                .map(|entry| entry.effect_kind.clone()),
            actual_kind: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::AuraFaultKind;
    use serde_json::json;
    use tempfile::NamedTempFile;

    fn sample_entry(effect_id: u64, kind: &str) -> EffectTraceEntry {
        EffectTraceEntry {
            effect_id,
            effect_kind: kind.to_string(),
            inputs: json!({"in": effect_id}),
            outputs: json!({"out": effect_id}),
            handler_identity: "test".to_string(),
            ordering_key: effect_id,
            topology: None,
        }
    }

    #[test]
    fn replay_trace_file_roundtrip_json() {
        let trace = ReplayTrace::from_entries(vec![
            sample_entry(0, "send_decision"),
            sample_entry(1, "handle_recv"),
        ]);
        let file = NamedTempFile::new().expect("tempfile");
        trace
            .save_file(file.path(), ReplayTraceEncoding::Json)
            .expect("save");

        let loaded =
            ReplayTrace::load_file(file.path(), ReplayTraceEncoding::Json).expect("load trace");
        assert_eq!(loaded.as_slice(), trace.as_slice());
    }

    #[test]
    fn replay_sequence_detects_divergence() {
        let trace = ReplayTrace::from_entries(vec![sample_entry(0, "send_decision")]);
        let mut verifier = ReplayEffectSequence::new(&trace);
        let error = verifier
            .verify_next(&sample_entry(0, "handle_recv"))
            .expect_err("must diverge");
        assert!(matches!(error, ReplayTraceError::Divergence { .. }));
    }

    #[test]
    fn replay_bundle_roundtrip_with_faults() {
        let trace = ReplayTrace::from_entries_and_faults(
            vec![sample_entry(0, "send_decision")],
            vec![AuraFault::new(AuraFaultKind::Legacy {
                fault_type: "network_partition".to_string(),
                detail: Some("g0|g1".to_string()),
            })],
        );
        let file = NamedTempFile::new().expect("tempfile");
        trace
            .save_file(file.path(), ReplayTraceEncoding::Json)
            .expect("save bundle");
        let loaded =
            ReplayTrace::load_file(file.path(), ReplayTraceEncoding::Json).expect("load bundle");
        assert_eq!(loaded.as_slice(), trace.as_slice());
        assert_eq!(loaded.faults(), trace.faults());
    }
}
