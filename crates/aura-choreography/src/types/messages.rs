//! Message definitions compatible with rumpsteak-aura `choreography!` macro

// Note: Message trait currently not available in rumpsteak-choreography
// For now, we'll create our own marker trait that can be replaced later
use aura_types::{AccountId, DeviceId};
use serde::{Deserialize, Serialize};

/// Temporary marker trait for choreography messages
/// This will be replaced with rumpsteak's Message trait when available
pub trait Message: Clone + serde::Serialize + serde::de::DeserializeOwned {}

/// Timestamp type alias for compatibility
pub type Timestamp = u64;

/// Threshold protocol messages
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ShareCommitment {
    #[serde(with = "serde_bytes")]
    pub commitment: Vec<u8>,
    pub participant_id: DeviceId,
    #[serde(with = "serde_bytes")]
    pub nonce: Vec<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ShareReveal {
    #[serde(with = "serde_bytes")]
    pub share_data: Vec<u8>,
    #[serde(with = "serde_bytes")]
    pub nonce: Vec<u8>,
    pub participant_id: DeviceId,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ThresholdContext {
    #[serde(with = "serde_bytes")]
    pub context_id: Vec<u8>,
    pub threshold: u32,
    pub total_participants: u32,
    pub timestamp: Timestamp,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReconstructedSecret {
    #[serde(with = "serde_bytes")]
    pub secret_share: Vec<u8>,
    pub participant_id: DeviceId,
    #[serde(with = "serde_bytes")]
    pub verification_proof: Vec<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VerifiedResult {
    #[serde(with = "serde_bytes")]
    pub result_hash: Vec<u8>,
    pub signatures: Vec<Vec<u8>>,
    pub success: bool,
}

/// FROST signature protocol messages
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NonceCommitment {
    #[serde(with = "serde_bytes")]
    pub commitment: Vec<u8>,
    pub signer_id: usize,
    pub round_id: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SignatureShare {
    #[serde(with = "serde_bytes")]
    pub share: Vec<u8>,
    pub signer_id: usize,
    pub round_id: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AggregatedSignature {
    #[serde(with = "serde_bytes")]
    pub signature: Vec<u8>,
    pub round_id: u64,
    pub participant_count: u32,
}

/// Coordination protocol messages
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Proposal {
    #[serde(with = "serde_bytes")]
    pub proposal_id: Vec<u8>,
    #[serde(with = "serde_bytes")]
    pub content: Vec<u8>,
    pub proposer: DeviceId,
    pub timestamp: Timestamp,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Response {
    #[serde(with = "serde_bytes")]
    pub proposal_id: Vec<u8>,
    pub response: bool, // accept/reject
    pub responder: DeviceId,
    pub timestamp: Timestamp,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Acknowledgment {
    #[serde(with = "serde_bytes")]
    pub proposal_id: Vec<u8>,
    pub acknowledger: DeviceId,
    pub timestamp: Timestamp,
}

/// Journal synchronization messages
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JournalSyncRequest {
    pub account_id: AccountId,
    #[serde(with = "serde_bytes")]
    pub last_known_hash: Vec<u8>,
    pub requestor: DeviceId,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JournalSyncResponse {
    pub account_id: AccountId,
    #[serde(with = "serde_bytes")]
    pub entries: Vec<u8>, // Serialized journal entries
    #[serde(with = "serde_bytes")]
    pub current_hash: Vec<u8>,
    pub responder: DeviceId,
}

// Implement Message trait for all message types
impl Message for ShareCommitment {}
impl Message for ShareReveal {}
impl Message for ThresholdContext {}
impl Message for ReconstructedSecret {}
impl Message for VerifiedResult {}
impl Message for NonceCommitment {}
impl Message for SignatureShare {}
impl Message for AggregatedSignature {}
impl Message for Proposal {}
impl Message for Response {}
impl Message for Acknowledgment {}
impl Message for JournalSyncRequest {}
impl Message for JournalSyncResponse {}
