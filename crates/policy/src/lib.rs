//! Policy engine for authorization decisions
//!
//! This crate implements risk-based authorization using:
//! - Cedar-lite style policy evaluation
//! - Biscuit capability tokens for offline delegation
//! - Predefined policy templates
//!
//! # Policy Model
//!
//! Operations are classified by risk tier (Low/Medium/High/Critical):
//! - **Low**: Basic reads, no guardian requirement
//! - **Medium**: Moderate operations, attestation for browser devices
//! - **High**: Sensitive operations, requires 2+ guardians
//! - **Critical**: Account changes, requires threshold signature
//!
//! # Components
//!
//! - [`engine::PolicyEngine`]: Evaluates policies against context
//! - [`biscuit_builder`]: Issues verifiable capability tokens
//! - [`templates`]: Predefined policies (conservative, balanced, solo, etc.)

pub mod types;
pub mod engine;
pub mod biscuit_builder;
pub mod templates;

pub use types::*;
pub use engine::*;
pub use biscuit_builder::*;
pub use templates::*;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum PolicyError {
    #[error("Policy evaluation failed: {0}")]
    EvaluationFailed(String),
    
    #[error("Policy denied: {0}")]
    Denied(String),
    
    #[error("Invalid policy: {0}")]
    InvalidPolicy(String),
    
    #[error("Biscuit error: {0}")]
    BiscuitError(String),
    
    #[error("Missing required fact: {0}")]
    MissingFact(String),
    
    #[error("System time error: {0}")]
    SystemTimeError(String),
}

pub type Result<T> = std::result::Result<T, PolicyError>;

