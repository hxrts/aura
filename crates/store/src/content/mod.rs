//! Content Processing Domain
//!
//! This domain handles all transformations of data content before storage:
//! - **Chunking**: Breaking large objects into manageable chunks with metadata
//! - **Encryption**: Protecting chunks with AES-256-GCM for confidentiality
//! - **Erasure Coding**: Adding redundancy via Reed-Solomon coding for durability
//!
//! # Processing Pipeline
//!
//! The typical data flow is:
//! ```text
//! Raw Data
//!   ↓
//! Chunking (splits into sized chunks, computes hashes)
//!   ↓
//! Encryption (encrypts each chunk with unique key/nonce)
//!   ↓
//! Erasure Coding (generates k-of-n fragments for redundancy)
//!   ↓
//! Storage (chunks ready for replication to peers)
//! ```
//!
//! # Key Properties
//!
//! - **Size Flexibility**: Chunk sizes configurable (default 1MB)
//! - **Deterministic**: Same data produces same chunks and hashes
//! - **Composable**: Each stage is independent, allowing flexibility
//! - **Auditable**: Chunk metadata enables verification without decryption
//!
//! # Integration Points
//!
//! - Upstream: Object manifest defines chunking parameters
//! - Downstream: Encryption keys come from DKD, erasure fragments go to replication
//! - Access Control: Separate domain, handles capability verification
//! - Replication: Separate domain, handles replica placement and verification

pub mod chunking;
pub mod encryption;
pub mod erasure;

pub use chunking::{chunk_data, compute_chunk_metadata, reassemble_chunks};
pub use encryption::{ContentEncryptionContext, Recipients};
pub use erasure::{
    ErasureCoder, ErasureError, ErasureFragment, ErasureParams, FragmentDistribution,
};
