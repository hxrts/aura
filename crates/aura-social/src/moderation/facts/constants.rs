//! Moderation fact type ID constants

/// Schema version for moderation fact payloads.
#[allow(dead_code)]
pub const MODERATION_FACT_SCHEMA_VERSION: u16 = 1;

/// Fact type ID for muting a user in a context or channel
pub const HOME_MUTE_FACT_TYPE_ID: &str = "moderation:home-mute";
/// Fact type ID for unmuting a previously muted user
pub const HOME_UNMUTE_FACT_TYPE_ID: &str = "moderation:home-unmute";
/// Fact type ID for banning a user from a context or channel
pub const HOME_BAN_FACT_TYPE_ID: &str = "moderation:home-ban";
/// Fact type ID for unbanning a previously banned user
pub const HOME_UNBAN_FACT_TYPE_ID: &str = "moderation:home-unban";
/// Fact type ID for kicking a user from a channel
pub const HOME_KICK_FACT_TYPE_ID: &str = "moderation:home-kick";
/// Fact type ID for granting steward privileges
pub const HOME_GRANT_STEWARD_FACT_TYPE_ID: &str = "moderation:home-grant-steward";
/// Fact type ID for revoking steward privileges
pub const HOME_REVOKE_STEWARD_FACT_TYPE_ID: &str = "moderation:home-revoke-steward";
/// Fact type ID for pinning a message
pub const HOME_PIN_FACT_TYPE_ID: &str = "moderation:home-pin";
/// Fact type ID for unpinning a message
pub const HOME_UNPIN_FACT_TYPE_ID: &str = "moderation:home-unpin";
