//! Moderation fact type ID constants

/// Fact type ID for muting a user in a context or channel
pub const BLOCK_MUTE_FACT_TYPE_ID: &str = "moderation:block-mute";
/// Fact type ID for unmuting a previously muted user
pub const BLOCK_UNMUTE_FACT_TYPE_ID: &str = "moderation:block-unmute";
/// Fact type ID for banning a user from a context or channel
pub const BLOCK_BAN_FACT_TYPE_ID: &str = "moderation:block-ban";
/// Fact type ID for unbanning a previously banned user
pub const BLOCK_UNBAN_FACT_TYPE_ID: &str = "moderation:block-unban";
/// Fact type ID for kicking a user from a channel
pub const BLOCK_KICK_FACT_TYPE_ID: &str = "moderation:block-kick";
/// Fact type ID for granting steward privileges
pub const BLOCK_GRANT_STEWARD_FACT_TYPE_ID: &str = "moderation:block-grant-steward";
/// Fact type ID for revoking steward privileges
pub const BLOCK_REVOKE_STEWARD_FACT_TYPE_ID: &str = "moderation:block-revoke-steward";
/// Fact type ID for pinning a message
pub const BLOCK_PIN_FACT_TYPE_ID: &str = "moderation:block-pin";
/// Fact type ID for unpinning a message
pub const BLOCK_UNPIN_FACT_TYPE_ID: &str = "moderation:block-unpin";
