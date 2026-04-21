//! Shared channel reference helpers for workflows.

use aura_core::crypto::hash::hash;
use aura_core::types::identifiers::ChannelId;
use aura_core::AuraError;

/// Reference to a channel identifier or name.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(
    not(feature = "signals"),
    allow(dead_code) // Shared parsing stays available for non-signals builds until every caller is feature-gated.
)]
pub enum ChannelRef {
    /// Canonical channel id.
    Id(ChannelId),
    /// Human-friendly name (hashed deterministically).
    Name(String),
}

impl ChannelRef {
    #[cfg_attr(
        not(feature = "signals"),
        allow(dead_code) // Non-signals command paths still reuse the parser during the staged messaging split.
    )]
    pub fn parse(input: &str) -> Self {
        let normalized = normalize_channel_str(input);
        match normalized.parse::<ChannelId>() {
            Ok(id) => ChannelRef::Id(id),
            Err(_) => ChannelRef::Name(normalized.to_string()),
        }
    }

    #[cfg_attr(
        not(feature = "signals"),
        allow(dead_code) // Name-to-id normalization remains the shared fallback for non-signals send/query flows.
    )]
    pub fn to_channel_id(&self) -> ChannelId {
        match self {
            ChannelRef::Id(id) => *id,
            ChannelRef::Name(name) => ChannelId::from_bytes(hash(name.to_lowercase().as_bytes())),
        }
    }
}

#[cfg_attr(
    not(feature = "signals"),
    allow(dead_code) // Keep normalization colocated with ChannelRef while legacy non-signals selectors remain supported.
)]
fn normalize_channel_str(channel: &str) -> &str {
    channel
}

/// Strict typed selector for channel/home references in command and workflow paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelSelector {
    /// Canonical channel/home ID.
    Id(ChannelId),
    /// User-facing channel name (`#name` or `name`).
    Name(String),
}

impl ChannelSelector {
    pub fn parse(input: &str) -> Result<Self, AuraError> {
        let raw = input.trim();
        if raw.is_empty() {
            return Err(AuraError::invalid("Channel selector cannot be empty"));
        }

        if let Ok(id) = raw.parse::<ChannelId>() {
            return Ok(Self::Id(id));
        }

        if raw.starts_with("channel:") {
            return Err(AuraError::invalid(format!(
                "Invalid canonical channel ID selector: {raw}"
            )));
        }

        let normalized_name = raw.trim_start_matches('#').trim();
        if normalized_name.is_empty() {
            return Err(AuraError::invalid("Channel name cannot be empty"));
        }

        if normalized_name.starts_with("home:") || normalized_name.starts_with("channel:") {
            return Err(AuraError::invalid(format!(
                "Invalid channel selector: {raw}"
            )));
        }

        Ok(Self::Name(normalized_name.to_string()))
    }

    #[allow(dead_code)] // Strict selectors retain deterministic hashing even before every caller migrates off ChannelRef.
    pub fn to_channel_id(&self) -> ChannelId {
        match self {
            Self::Id(id) => *id,
            Self::Name(name) => ChannelId::from_bytes(hash(name.to_lowercase().as_bytes())),
        }
    }
}

/// Strict selector for home-targeting commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HomeSelector {
    /// Local home anchor.
    Home,
    /// Current traversal position.
    Current,
    /// Canonical home/channel identifier.
    Id(ChannelId),
}

impl HomeSelector {
    pub fn parse(input: &str) -> Result<Self, AuraError> {
        let raw = input.trim();
        if raw.eq_ignore_ascii_case("home") {
            return Ok(Self::Home);
        }
        if raw.eq_ignore_ascii_case("current") {
            return Ok(Self::Current);
        }
        if let Some(home_encoded) = raw.strip_prefix("home:") {
            let encoded = home_encoded.trim();
            if encoded.is_empty() {
                return Err(AuraError::invalid("Home selector 'home:' is missing an ID"));
            }
            return encoded
                .parse::<ChannelId>()
                .map(Self::Id)
                .map_err(|_| AuraError::invalid(format!("Invalid home selector: {raw}")));
        }
        raw.parse::<ChannelId>()
            .map(Self::Id)
            .map_err(|_| AuraError::invalid(format!("Invalid home selector: {raw}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_selector_accepts_name_and_canonical_id() -> Result<(), AuraError> {
        let named = ChannelSelector::parse("#general")?;
        assert!(matches!(named, ChannelSelector::Name(name) if name == "general"));

        let canonical = ChannelId::from_bytes([7u8; 32]);
        let parsed = ChannelSelector::parse(&canonical.to_string())?;
        assert_eq!(parsed, ChannelSelector::Id(canonical));
        Ok(())
    }

    #[test]
    fn channel_selector_rejects_home_selectors() {
        let error = match ChannelSelector::parse("home:not-a-channel-id") {
            Ok(value) => panic!("malformed home selector must fail: {value:?}"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("Invalid channel selector"));
    }

    #[test]
    fn home_selector_rejects_malformed_id() {
        let error = match HomeSelector::parse("home:not-a-channel-id") {
            Ok(value) => panic!("malformed home selector must fail: {value:?}"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("Invalid home selector"));
    }
}
