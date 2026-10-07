//! Moderation query and reduction layer
//!
//! This module provides query functions to derive current moderation state
//! from journal facts. It implements the reduction logic to compute:
//! - Current bans (after applying unbans)
//! - Current mutes (with expiration checking)
//! - Kick audit log history

pub mod facts;
pub mod governance;
pub mod query;
pub mod types;

pub use facts::{
    register_moderation_facts, HomeAdmitMemberFact, HomeBanFact, HomeGrantModeratorFact,
    HomeKickFact, HomeMuteFact, HomeRevokeModeratorFact, HomeUnbanFact, HomeUnmuteFact,
    HOME_ADMIT_MEMBER_FACT_TYPE_ID, HOME_BAN_FACT_TYPE_ID, HOME_GRANT_MODERATOR_FACT_TYPE_ID,
    HOME_KICK_FACT_TYPE_ID, HOME_MUTE_FACT_TYPE_ID, HOME_REVOKE_MODERATOR_FACT_TYPE_ID,
    HOME_UNBAN_FACT_TYPE_ID, HOME_UNMUTE_FACT_TYPE_ID,
};
pub use governance::{
    causal_order, home_governance_causal, live_ban_tags, live_member_admission_tags,
    live_moderator_grant_tags, live_mute_tags, observed_governance_vector,
    resolved_access_overrides, resolved_capability_config, sort_causally, HomeGovernanceEvent,
    HomeGovernanceKey, TaggedHomeGovernanceEvent,
};
pub use query::{
    is_user_banned, is_user_muted, query_current_bans, query_current_bans_in_live_channels,
    query_current_mutes, query_current_mutes_in_live_channels, query_kick_history,
    try_is_user_banned_and_muted, ModerationQueryBound, RequiredModerationQueryError,
};
pub use types::{BanStatus, KickRecord, ModerationScopeKey, MuteStatus};
