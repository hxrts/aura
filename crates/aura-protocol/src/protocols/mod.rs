//! Choreographic protocol coordination module
//!
//! This module provides choreographic implementations of Aura's distributed protocols
//! using the Rumpsteak-Aura framework, integrated with Aura's middleware stack.

pub mod choreographic;
pub mod coordination;
pub mod patterns;
pub mod threshold_crypto;

// Re-export choreographic integration types
pub use choreographic::{BridgedEndpoint, BridgedRole, RumpsteakAdapter};

// Re-export protocol implementations
pub use patterns::{DecentralizedLottery, LotteryMessage};
pub use threshold_crypto::{DkdMessage, DkdProtocol, FrostMessage, FrostSigningProtocol};

// Re-export comprehensive message types from aura-messages for external use
pub use aura_messages::crypto::{
    DkdFinalizeMessage, DkdMessage as DkdMessageComplete, DkdPointCommitmentMessage,
    DkdPointRevealMessage, FrostAggregateSignatureMessage, FrostMessage as FrostMessageComplete,
    FrostSignatureShareMessage, FrostSigningCommitmentMessage, FrostSigningInitMessage,
    InitiateDkdSessionMessage,
};
pub use coordination::{
    CoordinatorFailureRecovery, CoordinatorMessage, CoordinatorMonitor, EpochBumpChoreography,
    EpochMessage, SessionEpochMonitor,
};

// Message types re-exported from aura-messages for protocol implementations
pub use aura_messages::social::{
    AuthenticationPayload, HandshakeResult, HandshakeTranscript, PayloadKind, PskHandshakeConfig,
    RendezvousMessage, StorageCapabilityAnnouncement, TransportDescriptor, TransportKind,
    TransportOfferPayload,
};
