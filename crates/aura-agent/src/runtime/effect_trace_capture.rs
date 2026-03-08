//! Effect-trace capture helpers for deterministic replay and diagnostics.

use std::fs;
use std::path::Path;

use aura_core::AuraVmDeterminismProfileV1;
use aura_core::AuraFault;
use serde::{Deserialize, Serialize};
use telltale_vm::{canonical_effect_trace, EffectTraceCaptureMode, EffectTraceEntry, VMConfig};

/// Trace payload encoding for persisted replay artifacts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AuraEffectTraceEncoding {
    /// UTF-8 JSON payload.
    #[default]
    Json,
    /// Binary CBOR payload.
    Cbor,
}

impl AuraEffectTraceEncoding {
    /// Parse encoding from CLI/config text.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "json" => Some(Self::Json),
            "cbor" => Some(Self::Cbor),
            _ => None,
        }
    }
}

/// Capture granularity mapped to Telltale VM capture modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AuraEffectTraceGranularity {
    /// Capture all effect kinds.
    #[default]
    Full,
    /// Capture topology-only events.
    TopologyOnly,
    /// Disable capture.
    Disabled,
}

impl AuraEffectTraceGranularity {
    /// Convert to VM capture mode.
    #[must_use]
    pub fn to_vm_mode(self) -> EffectTraceCaptureMode {
        match self {
            Self::Full => EffectTraceCaptureMode::Full,
            Self::TopologyOnly => EffectTraceCaptureMode::TopologyOnly,
            Self::Disabled => EffectTraceCaptureMode::Disabled,
        }
    }

    /// Convert from VM capture mode.
    #[must_use]
    pub fn from_vm_mode(mode: EffectTraceCaptureMode) -> Self {
        match mode {
            EffectTraceCaptureMode::Full => Self::Full,
            EffectTraceCaptureMode::TopologyOnly => Self::TopologyOnly,
            EffectTraceCaptureMode::Disabled => Self::Disabled,
        }
    }
}

/// Capture options used by [`EffectTraceCapture`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectTraceCaptureOptions {
    /// Persisted encoding format.
    pub encoding: AuraEffectTraceEncoding,
    /// Capture granularity.
    pub granularity: AuraEffectTraceGranularity,
    /// Canonicalize trace before serialization.
    pub canonicalize: bool,
}

impl Default for EffectTraceCaptureOptions {
    fn default() -> Self {
        Self {
            encoding: AuraEffectTraceEncoding::Json,
            granularity: AuraEffectTraceGranularity::Full,
            canonicalize: true,
        }
    }
}

/// File/serialization errors from trace capture and replay tooling.
#[derive(Debug, thiserror::Error)]
pub enum EffectTraceCaptureError {
    /// IO failure while reading/writing trace artifacts.
    #[error("effect trace IO error at {path}: {source}")]
    Io {
        /// Path that failed.
        path: String,
        /// Wrapped IO error.
        source: std::io::Error,
    },
    /// JSON encoding/decoding failure.
    #[error("effect trace JSON serialization failed: {source}")]
    Json {
        /// Wrapped serde error.
        source: serde_json::Error,
    },
    /// CBOR encoding/decoding failure.
    #[error("effect trace CBOR serialization failed: {source}")]
    Cbor {
        /// Wrapped serde error.
        source: serde_cbor::Error,
    },
}

/// Trace-capture utility with canonicalization + granularity filtering.
#[derive(Debug, Clone, Copy)]
pub struct EffectTraceCapture {
    options: EffectTraceCaptureOptions,
}

/// Fault-aware effect trace bundle used for replay/debug artifacts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EffectTraceBundle {
    /// Captured effect trace entries.
    pub entries: Vec<EffectTraceEntry>,
    /// Serialized injected faults for replay.
    #[serde(default)]
    pub faults: Vec<AuraFault>,
    /// Optional determinism profile metadata active for the captured run.
    #[serde(default)]
    pub vm_determinism_profile: Option<AuraVmDeterminismProfileV1>,
}

impl Default for EffectTraceCapture {
    fn default() -> Self {
        Self::new(EffectTraceCaptureOptions::default())
    }
}

impl EffectTraceCapture {
    /// Build a capture utility with explicit options.
    #[must_use]
    pub fn new(options: EffectTraceCaptureOptions) -> Self {
        Self { options }
    }

    /// Read configured options.
    #[must_use]
    pub fn options(&self) -> EffectTraceCaptureOptions {
        self.options
    }

    /// Apply capture granularity onto VM configuration.
    pub fn apply_to_vm_config(&self, config: &mut VMConfig) {
        config.effect_trace_capture_mode = self.options.granularity.to_vm_mode();
    }

    /// Build capture utility from VM configuration.
    #[must_use]
    pub fn from_vm_config(config: &VMConfig) -> Self {
        Self::new(EffectTraceCaptureOptions {
            encoding: AuraEffectTraceEncoding::Json,
            granularity: AuraEffectTraceGranularity::from_vm_mode(config.effect_trace_capture_mode),
            canonicalize: true,
        })
    }

    /// Prepare a trace for persistence/replay according to capture options.
    #[must_use]
    pub fn capture_entries(&self, trace: &[EffectTraceEntry]) -> Vec<EffectTraceEntry> {
        let mut entries = if self.options.canonicalize {
            canonical_effect_trace(trace)
        } else {
            trace.to_vec()
        };

        match self.options.granularity {
            AuraEffectTraceGranularity::Full => {}
            AuraEffectTraceGranularity::TopologyOnly => {
                entries.retain(|entry| entry.effect_kind == "topology_event");
            }
            AuraEffectTraceGranularity::Disabled => {
                entries.clear();
            }
        }

        entries
    }

    /// Serialize trace entries using configured encoding.
    ///
    /// # Errors
    ///
    /// Returns serialization errors for invalid payloads.
    pub fn serialize_entries(
        &self,
        entries: &[EffectTraceEntry],
    ) -> Result<Vec<u8>, EffectTraceCaptureError> {
        match self.options.encoding {
            AuraEffectTraceEncoding::Json => serde_json::to_vec(entries)
                .map_err(|source| EffectTraceCaptureError::Json { source }),
            AuraEffectTraceEncoding::Cbor => serde_cbor::to_vec(&entries.to_vec())
                .map_err(|source| EffectTraceCaptureError::Cbor { source }),
        }
    }

    /// Deserialize trace entries using configured encoding.
    ///
    /// # Errors
    ///
    /// Returns serialization errors for malformed payloads.
    pub fn deserialize_entries(
        &self,
        payload: &[u8],
    ) -> Result<Vec<EffectTraceEntry>, EffectTraceCaptureError> {
        match self.options.encoding {
            AuraEffectTraceEncoding::Json => serde_json::from_slice(payload)
                .map_err(|source| EffectTraceCaptureError::Json { source }),
            AuraEffectTraceEncoding::Cbor => serde_cbor::from_slice(payload)
                .map_err(|source| EffectTraceCaptureError::Cbor { source }),
        }
    }

    /// Capture + serialize one trace payload.
    ///
    /// # Errors
    ///
    /// Returns serialization errors for invalid payloads.
    pub fn serialize_trace(
        &self,
        trace: &[EffectTraceEntry],
    ) -> Result<Vec<u8>, EffectTraceCaptureError> {
        let captured = self.capture_entries(trace);
        self.serialize_entries(&captured)
    }

    /// Serialize a fault-aware replay bundle.
    ///
    /// # Errors
    ///
    /// Returns serialization errors for invalid payloads.
    pub fn serialize_bundle(
        &self,
        trace: &[EffectTraceEntry],
        faults: &[AuraFault],
        vm_determinism_profile: Option<AuraVmDeterminismProfileV1>,
    ) -> Result<Vec<u8>, EffectTraceCaptureError> {
        let bundle = EffectTraceBundle {
            entries: self.capture_entries(trace),
            faults: faults.to_vec(),
            vm_determinism_profile,
        };
        match self.options.encoding {
            AuraEffectTraceEncoding::Json => serde_json::to_vec(&bundle)
                .map_err(|source| EffectTraceCaptureError::Json { source }),
            AuraEffectTraceEncoding::Cbor => serde_cbor::to_vec(&bundle)
                .map_err(|source| EffectTraceCaptureError::Cbor { source }),
        }
    }

    /// Deserialize a fault-aware replay bundle.
    ///
    /// # Errors
    ///
    /// Returns serialization errors for malformed payloads.
    pub fn deserialize_bundle(
        &self,
        payload: &[u8],
    ) -> Result<EffectTraceBundle, EffectTraceCaptureError> {
        match self.options.encoding {
            AuraEffectTraceEncoding::Json => serde_json::from_slice(payload)
                .map_err(|source| EffectTraceCaptureError::Json { source }),
            AuraEffectTraceEncoding::Cbor => serde_cbor::from_slice(payload)
                .map_err(|source| EffectTraceCaptureError::Cbor { source }),
        }
    }

    /// Write trace to one artifact path.
    ///
    /// # Errors
    ///
    /// Returns IO/serialization errors.
    pub fn write_trace_file(
        &self,
        path: impl AsRef<Path>,
        trace: &[EffectTraceEntry],
    ) -> Result<(), EffectTraceCaptureError> {
        let path_ref = path.as_ref();
        let payload = self.serialize_trace(trace)?;
        fs::write(path_ref, payload).map_err(|source| EffectTraceCaptureError::Io {
            path: path_ref.display().to_string(),
            source,
        })
    }

    /// Read + decode trace from one artifact path.
    ///
    /// # Errors
    ///
    /// Returns IO/serialization errors.
    pub fn read_trace_file(
        &self,
        path: impl AsRef<Path>,
    ) -> Result<Vec<EffectTraceEntry>, EffectTraceCaptureError> {
        let path_ref = path.as_ref();
        let payload = fs::read(path_ref).map_err(|source| EffectTraceCaptureError::Io {
            path: path_ref.display().to_string(),
            source,
        })?;
        self.deserialize_entries(&payload)
    }

    /// Write a fault-aware replay bundle to one artifact path.
    ///
    /// # Errors
    ///
    /// Returns IO/serialization errors.
    pub fn write_bundle_file(
        &self,
        path: impl AsRef<Path>,
        trace: &[EffectTraceEntry],
        faults: &[AuraFault],
        vm_determinism_profile: Option<AuraVmDeterminismProfileV1>,
    ) -> Result<(), EffectTraceCaptureError> {
        let path_ref = path.as_ref();
        let payload = self.serialize_bundle(trace, faults, vm_determinism_profile)?;
        fs::write(path_ref, payload).map_err(|source| EffectTraceCaptureError::Io {
            path: path_ref.display().to_string(),
            source,
        })
    }

    /// Read + decode a fault-aware replay bundle from one artifact path.
    ///
    /// # Errors
    ///
    /// Returns IO/serialization errors.
    pub fn read_bundle_file(
        &self,
        path: impl AsRef<Path>,
    ) -> Result<EffectTraceBundle, EffectTraceCaptureError> {
        let path_ref = path.as_ref();
        let payload = fs::read(path_ref).map_err(|source| EffectTraceCaptureError::Io {
            path: path_ref.display().to_string(),
            source,
        })?;
        self.deserialize_bundle(&payload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_entry(effect_id: u64, effect_kind: &str) -> EffectTraceEntry {
        EffectTraceEntry {
            effect_id,
            effect_kind: effect_kind.to_string(),
            inputs: json!({"in": effect_id}),
            outputs: json!({"out": effect_id}),
            handler_identity: "handler".to_string(),
            ordering_key: effect_id,
            topology: None,
        }
    }

    #[test]
    fn topology_only_filters_non_topology_entries() {
        let capture = EffectTraceCapture::new(EffectTraceCaptureOptions {
            granularity: AuraEffectTraceGranularity::TopologyOnly,
            canonicalize: false,
            ..EffectTraceCaptureOptions::default()
        });
        let trace = vec![
            sample_entry(0, "send_decision"),
            sample_entry(1, "topology_event"),
        ];

        let captured = capture.capture_entries(&trace);
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].effect_kind, "topology_event");
    }

    #[test]
    fn disabled_capture_returns_empty_trace() {
        let capture = EffectTraceCapture::new(EffectTraceCaptureOptions {
            granularity: AuraEffectTraceGranularity::Disabled,
            canonicalize: false,
            ..EffectTraceCaptureOptions::default()
        });
        let trace = vec![sample_entry(0, "send_decision")];
        assert!(capture.capture_entries(&trace).is_empty());
    }

    #[test]
    fn cbor_roundtrip_succeeds() {
        let capture = EffectTraceCapture::new(EffectTraceCaptureOptions {
            encoding: AuraEffectTraceEncoding::Cbor,
            canonicalize: false,
            ..EffectTraceCaptureOptions::default()
        });
        let trace = vec![
            sample_entry(0, "send_decision"),
            sample_entry(1, "handle_recv"),
        ];

        let payload = capture.serialize_entries(&trace).expect("serialize cbor");
        let decoded = capture
            .deserialize_entries(&payload)
            .expect("deserialize cbor");
        assert_eq!(decoded, trace);
    }

    #[test]
    fn json_bundle_roundtrip_preserves_faults() {
        let capture = EffectTraceCapture::new(EffectTraceCaptureOptions {
            encoding: AuraEffectTraceEncoding::Json,
            canonicalize: false,
            ..EffectTraceCaptureOptions::default()
        });
        let trace = vec![sample_entry(0, "send_decision")];
        let faults = vec![AuraFault::new(aura_core::AuraFaultKind::Legacy {
            fault_type: "network_partition".to_string(),
            detail: Some("group=2".to_string()),
        })];

        let payload = capture
            .serialize_bundle(&trace, &faults, None)
            .expect("serialize bundle");
        let decoded = capture
            .deserialize_bundle(&payload)
            .expect("deserialize bundle");
        assert_eq!(decoded.entries, trace);
        assert_eq!(decoded.faults, faults);
        assert_eq!(decoded.vm_determinism_profile, None);
    }

    #[test]
    fn bundle_roundtrip_preserves_determinism_profile() {
        let capture = EffectTraceCapture::default();
        let trace = vec![sample_entry(1, "handle_recv")];
        let payload = capture
            .serialize_bundle(
                &trace,
                &[],
                Some(AuraVmDeterminismProfileV1 {
                    policy_ref: "aura.vm.recovery_grant.prod".to_string(),
                    protocol_class: "aura.recovery.grant".to_string(),
                    determinism_mode: "full".to_string(),
                    effect_determinism_tier: "strict_deterministic".to_string(),
                    communication_replay_mode: "off".to_string(),
                }),
            )
            .expect("serialize bundle");
        let decoded = capture
            .deserialize_bundle(&payload)
            .expect("deserialize bundle");

        assert_eq!(
            decoded
                .vm_determinism_profile
                .as_ref()
                .map(|profile| profile.policy_ref.as_str()),
            Some("aura.vm.recovery_grant.prod")
        );
    }
}
