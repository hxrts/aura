//! # Intent Context and Traits
//!
//! Provides context for mapping commands to intents and the `IntoIntent` trait
//! for decoupled command-to-intent conversion.

use aura_app::Intent;
use aura_core::identifiers::{AuthorityId, ContextId};

/// Context information for command-to-intent mapping.
///
/// This provides the current block, recovery, and authority context needed to
/// properly construct Intents that require context (moderation commands,
/// recovery commands, etc.).
#[derive(Debug, Clone, Default)]
pub struct IntentContext {
    /// Current block ID for moderation commands (BanUser, MuteUser, etc.)
    pub block_id: Option<ContextId>,

    /// Active recovery context ID for recovery commands (CompleteRecovery, ApproveRecovery, etc.)
    pub recovery_context_id: Option<ContextId>,

    /// Current authority ID for commands that need the user's authority
    pub authority_id: Option<AuthorityId>,

    /// Current channel ID for channel-scoped commands
    pub channel_id: Option<String>,
}

impl IntentContext {
    /// Create an empty context (uses nil values for all fields)
    pub fn empty() -> Self {
        Self::default()
    }

    /// Create context from an AppCore StateSnapshot
    pub fn from_snapshot(snapshot: &aura_app::StateSnapshot) -> Self {
        // Prefer the selected block's explicit context_id when available.
        let block_id = snapshot
            .blocks
            .current_block()
            .and_then(|b| b.context_id.parse::<ContextId>().ok())
            .or_else(|| snapshot.block.context_id.parse::<ContextId>().ok())
            // Legacy fallback: derive a deterministic ContextId from a ChannelId.
            .or_else(|| {
                snapshot.neighborhood.position.as_ref().map(|p| {
                    let hash_bytes = p.current_block_id.as_bytes();
                    let mut uuid_bytes = [0u8; 16];
                    uuid_bytes.copy_from_slice(&hash_bytes[..16]);
                    ContextId::from(uuid::Uuid::from_bytes(uuid_bytes))
                })
            });

        // Extract recovery context ID from active recovery
        let recovery_context_id = snapshot.recovery.active_recovery.as_ref().map(|r| {
            // Try to parse as UUID first, then hash the string
            if let Ok(uuid) = uuid::Uuid::parse_str(&r.id) {
                ContextId::from(uuid)
            } else {
                // Hash the string for deterministic ID
                let hash = aura_core::hash::hash(r.id.as_bytes());
                let mut bytes = [0u8; 16];
                bytes.copy_from_slice(&hash[..16]);
                ContextId::from(uuid::Uuid::from_bytes(bytes))
            }
        });

        Self {
            block_id,
            recovery_context_id,
            authority_id: None, // Authority is typically obtained from AppCore
            channel_id: None,   // Channel is command-specific
        }
    }

    /// Get the current block ID, falling back to a nil context if not set
    pub fn block_id_or_nil(&self) -> ContextId {
        self.block_id.unwrap_or_else(Self::nil_context)
    }

    /// Get the recovery context ID, falling back to a nil context if not set
    pub fn recovery_id_or_nil(&self) -> ContextId {
        self.recovery_context_id.unwrap_or_else(Self::nil_context)
    }

    /// Create a nil/default ContextId for placeholder purposes.
    ///
    /// Used when a command doesn't specify a context but the Intent requires one.
    /// The actual context is resolved at dispatch time in AppCore.
    fn nil_context() -> ContextId {
        ContextId::new_from_entropy([0u8; 32])
    }
}

/// Trait for converting commands to domain Intents.
///
/// Implement this trait for command types that can be mapped to Intents.
/// Commands that don't map to Intents (operational commands) return `None`.
///
/// # Example
///
/// ```rust,ignore
/// use crate::tui::effects::intent_context::{IntoIntent, IntentContext};
///
/// let ctx = IntentContext::from_snapshot(&snapshot);
/// if let Some(intent) = command.into_intent(&ctx) {
///     app_core.dispatch(intent)?;
/// }
/// ```
pub trait IntoIntent {
    /// Convert this command into an Intent, if applicable.
    ///
    /// Returns `Some(Intent)` for commands that should be journaled via AppCore.
    /// Returns `None` for operational commands handled elsewhere.
    #[allow(clippy::wrong_self_convention)]
    fn into_intent(&self, ctx: &IntentContext) -> Option<Intent>;

    /// Check if this command can be converted to an Intent.
    ///
    /// Returns true if `into_intent` would return `Some`.
    fn is_intent_command(&self) -> bool {
        self.into_intent(&IntentContext::empty()).is_some()
    }
}

/// Parse a channel/block ID string into a ContextId.
///
/// Uses deterministic hashing for consistent IDs across sessions.
pub fn parse_context_id(id_str: &str) -> ContextId {
    // Try to parse as UUID first
    if let Ok(uuid) = uuid::Uuid::parse_str(id_str) {
        return ContextId::from(uuid);
    }

    // Fall back to deterministic hashing for named channels
    let hash = aura_core::hash::hash(id_str.as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&hash[..16]);
    ContextId::from(uuid::Uuid::from_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_context() {
        let ctx = IntentContext::empty();
        assert!(ctx.block_id.is_none());
        assert!(ctx.recovery_context_id.is_none());
        assert!(ctx.authority_id.is_none());
        assert!(ctx.channel_id.is_none());
    }

    #[test]
    fn test_block_id_or_nil() {
        let mut ctx = IntentContext::empty();
        let nil_id = ctx.block_id_or_nil();

        let block_id =
            ContextId::from(uuid::Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap());
        ctx.block_id = Some(block_id);

        assert_eq!(ctx.block_id_or_nil(), block_id);
        assert_ne!(nil_id, block_id);
    }

    #[test]
    fn test_context_id_parsing() {
        // Named channel - deterministic
        let id1 = parse_context_id("general");
        let id2 = parse_context_id("general");
        assert_eq!(id1, id2);

        // Different channel
        let id3 = parse_context_id("random");
        assert_ne!(id1, id3);

        // UUID format
        let uuid_str = "550e8400-e29b-41d4-a716-446655440000";
        let id4 = parse_context_id(uuid_str);
        assert_eq!(
            id4,
            ContextId::from(uuid::Uuid::parse_str(uuid_str).unwrap())
        );
    }
}
