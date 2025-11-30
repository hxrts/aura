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

pub mod evaluator;
pub mod handler;
pub mod properties;
pub mod runner;
pub mod types;

pub use evaluator::QuintEvaluator;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert!(!version().is_empty());
    }
}
