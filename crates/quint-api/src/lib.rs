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
//! ```rust
//! use quint_api::{QuintRunner, PropertySpec, VerificationResult};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let runner = QuintRunner::new()?;
//!     
//!     let property = PropertySpec::new("always safety")
//!         .with_invariant("counter >= 0")
//!         .with_context("counter", "Int");
//!     
//!     let result = runner.verify_property(&property).await?;
//!     println!("Verification result: {:?}", result);
//!     
//!     Ok(())
//! }
//! ```

pub mod error;
pub mod evaluator;
pub mod properties;
pub mod runner;
pub mod types;

// Re-export main API types
pub use error::{QuintError, QuintResult};
pub use evaluator::QuintEvaluator;
pub use properties::{PropertySpec, PropertySuite, PropertyKind};
pub use runner::{QuintRunner, RunnerConfig};
pub use types::VerificationResult;

// Re-export quint evaluator types
pub use quint_evaluator::ir::{QuintError as QuintIRError, QuintEx, QuintOutput};
pub use quint_evaluator::simulator::{ParsedQuint, SimulationResult};

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