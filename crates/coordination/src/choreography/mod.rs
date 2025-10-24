//! Protocol Choreographies - Choreographic Programming with Session Types
//!
//! This module implements protocols using **choreographic programming**, where
//! distributed protocols are written as linear async functions that look like
//! single-threaded code but coordinate across multiple devices.
//!
//! ## Choreographic Programming
//!
//! Choreographies describe multi-party protocols from a global viewpoint,
//! automatically handling:
//! - Message coordination (who sends what to whom)
//! - Synchronization (waiting for messages)
//! - Error handling and timeouts
//! - Session types (compile-time protocol correctness)
//!
//! ## Example Choreography
//!
//! ```rust,ignore
//! pub async fn dkd_choreography(ctx: &mut ProtocolContext) -> Result<Vec<u8>, ProtocolError> {
//!     // Phase 1: All parties broadcast commitments
//!     let commitment = compute_commitment();
//!     ctx.execute(Instruction::WriteToLedger(commitment_event)).await?;
//!     
//!     // Wait for threshold commitments (choreographic synchronization)
//!     let peer_commitments = ctx.execute(Instruction::AwaitThreshold {
//!         count: ctx.threshold().unwrap(),
//!         filter: commitment_filter(),
//!         timeout_epochs: Some(100),
//!     }).await?;
//!     
//!     // Phase 2: All parties reveal...
//! }
//! ```
//!
//! ## Benefits of Choreographic Style
//!
//! - **Global viewpoint**: Protocol described as single program
//! - **Local projection**: Each device executes its role automatically
//! - **Session types**: Communication patterns type-checked
//! - **Deadlock freedom**: Guaranteed by choreographic structure
//!
//! Reference:
//! - work/04_declarative_protocol_evolution.md - Phase 2
//! - Choreographic Programming: https://arxiv.org/abs/1303.0039

pub mod dkd;
pub mod locking;
pub mod recovery;
pub mod resharing;

pub use dkd::dkd_choreography;
pub use locking::locking_choreography;
pub use recovery::{nudge_guardian, recovery_choreography};
pub use resharing::resharing_choreography;
