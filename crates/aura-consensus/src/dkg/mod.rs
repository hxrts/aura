//! BFT-DKG orchestration and transcript handling.

pub mod ceremony;
pub mod dealer;
pub mod recovery;
pub mod state_machine;
pub mod storage;
pub mod transcript;
pub mod types;
pub mod verifier;

pub use ceremony::run_consensus_dkg;
pub use state_machine::{DkgAggregationMode, DkgAggregationResult, DkgCollectionState, DkgPhase};
pub use storage::{DkgTranscriptStore, MemoryTranscriptStore, StorageTranscriptStore};
pub use types::{DealerPackage, DkgConfig, DkgTranscript};
