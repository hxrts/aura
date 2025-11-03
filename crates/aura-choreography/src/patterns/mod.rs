//! Common choreographic patterns

pub mod broadcast_and_gather;
pub mod lottery;
pub mod propose_and_acknowledge;
pub mod threshold_collect;
pub mod threshold_examples;
pub mod verify_consistent_result;

pub use broadcast_and_gather::{
    BroadcastAndGatherChoreography, BroadcastGatherConfig, BroadcastGatherResult,
    BroadcastMessage, MessageValidator, DefaultMessageValidator,
    broadcast_and_gather, broadcast_and_gather_message,
};
pub use lottery::{DecentralizedLottery, LotteryMessage};
pub use propose_and_acknowledge::{
    ProposeAndAcknowledgeChoreography, ProposeAcknowledgeConfig, ProposeAcknowledgeResult,
    ProposeAcknowledgeMessage, ProposalValidator, DefaultProposalValidator,
    propose_to_participants, receive_proposal_from,
};
pub use threshold_collect::{
    ThresholdCollectChoreography, ThresholdCollectConfig, ThresholdCollectResult,
    ThresholdCollectMessage, ThresholdOperationProvider,
};
pub use threshold_examples::{
    DkdThresholdProvider, DkdContext, DkdMaterial, DkdResult,
    FrostThresholdProvider, FrostContext, FrostMaterial, FrostResult,
};
pub use verify_consistent_result::{
    VerifyConsistentResultChoreography, VerificationConfig, VerificationResult,
    VerificationMessage, ResultComparator, DefaultResultComparator,
    verify_consistent_result,
};
