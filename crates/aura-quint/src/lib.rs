//! Native Quint API for Aura
//!
//! This crate provides a native Rust interface to the Quint formal verification language
//! using the Quint Rust evaluator directly. It eliminates the Node.js bridge dependency
//! while providing full access to Quint's verification capabilities.
//!
//! # Architecture
//!
//! The native implementation uses a hybrid approach:
//! - **Parsing**: TypeScript parser generates JSON IR from .qnt files
//! - **Evaluation**: Native Rust evaluator consumes JSON IR for simulation
//!
//! # Usage
//!
//! ```rust,ignore
//! use aura_quint::{QuintRunner, PropertySpec, VerificationResult};
//!
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let mut runner = QuintRunner::new()?;
//!
//!     let property = PropertySpec::new("always safety")
//!         .with_invariant("counter >= 0")
//!         .with_context("counter", "Int");
//!
//!     // Run inside your async runtime of choice
//!     let result = async {
//!         runner.verify_property(&property).await
//!     }
//!     .await?;
//!     println!("Verification result: {:?}", result);
//!
//!     Ok(())
//! }
//! ```

pub mod bridge_export;
pub mod bridge_format;
pub mod bridge_import;
pub mod bridge_validate;
pub mod evaluator;
pub mod handler;
pub mod properties;
pub mod runner;
pub mod types;

/// Re-export of the upstream Telltale Lean bridge crate for direct integration.
pub use telltale_lean_bridge as upstream_telltale_lean_bridge;

pub use bridge_export::{
    export_quint_to_telltale_bundle, parse_quint_modules, validate_export_bundle,
    BridgeExportError, QuintModuleSummary,
};
pub use bridge_format::{
    BridgeBundleV1, ProofBackendV1, ProofCertificateV1, PropertyClassV1, PropertyInterchangeV1,
    SessionEdgeV1, SessionNodeKindV1, SessionNodeV1, SessionTypeInterchangeV1,
    AURA_LEAN_QUINT_BRIDGE_SCHEMA_V1,
};
pub use bridge_import::{
    generate_quint_invariant_module, map_certificates_to_quint_assertions,
    parse_telltale_properties, BridgeImportError,
};
pub use bridge_validate::{
    run_cross_validation, CrossValidationDiscrepancy, CrossValidationReport, QuintCheckResult,
    QuintModelCheckExecutor, StaticQuintExecutor,
};
pub use evaluator::{InvariantVerificationResult, QuintEvaluator, TemporalVerificationResult};
pub use handler::{QuintEvaluator as QuintEffectHandler, QuintEvaluatorConfig};
pub use properties::{PropertyKind, PropertySpec, PropertySuite};
pub use runner::{QuintRunner, RunnerConfig};
pub use types::VerificationResult;

// Re-export quint evaluator types
pub use quint_evaluator::ir::{QuintError as QuintIRError, QuintEx, QuintOutput};

// Re-export quint evaluator simulator types
pub use quint_evaluator::simulator::{ParsedQuint, SimulationResult};

// Re-export the unified error system
pub use aura_core::{AuraError, Result as AuraResult};

/// Version information for the Quint API
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Schema version reported by the upstream `telltale-lean-bridge` crate.
#[must_use]
pub fn upstream_telltale_lean_bridge_schema_version() -> &'static str {
    telltale_lean_bridge::LEAN_BRIDGE_SCHEMA_VERSION
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert!(!version().is_empty());
    }

    #[test]
    fn test_upstream_telltale_lean_bridge_schema_version() {
        assert!(!upstream_telltale_lean_bridge_schema_version().is_empty());
    }
}
