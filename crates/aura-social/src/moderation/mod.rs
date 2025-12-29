//! Moderation query and reduction layer
//!
//! This module provides query functions to derive current moderation state
//! from journal facts. It implements the reduction logic to compute:
//! - Current bans (after applying unbans)
//! - Current mutes (with expiration checking)
//! - Kick audit log history

pub mod facts;
pub mod query;
pub mod types;

pub use facts::{
    register_moderation_facts, HomeBanFact, HomeGrantStewardFact, HomeKickFact, HomeMuteFact,
    HomeRevokeStewardFact, HomeUnbanFact, HomeUnmuteFact, HOME_BAN_FACT_TYPE_ID,
    HOME_GRANT_STEWARD_FACT_TYPE_ID, HOME_KICK_FACT_TYPE_ID, HOME_MUTE_FACT_TYPE_ID,
    HOME_REVOKE_STEWARD_FACT_TYPE_ID, HOME_UNBAN_FACT_TYPE_ID, HOME_UNMUTE_FACT_TYPE_ID,
};
pub use query::{
    is_user_banned, is_user_muted, query_current_bans, query_current_mutes, query_kick_history,
};
pub use types::{BanStatus, KickRecord, MuteStatus};
