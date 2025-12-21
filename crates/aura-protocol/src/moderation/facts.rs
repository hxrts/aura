use aura_core::identifiers::{AuthorityId, ChannelId, ContextId};
use aura_core::time::PhysicalTime;
use aura_journal::{
    reduction::{RelationalBinding, RelationalBindingType},
    DomainFact, FactReducer, FactRegistry,
};
use serde::{Deserialize, Serialize};

pub const BLOCK_MUTE_FACT_TYPE_ID: &str = "moderation:block-mute";
pub const BLOCK_UNMUTE_FACT_TYPE_ID: &str = "moderation:block-unmute";
pub const BLOCK_BAN_FACT_TYPE_ID: &str = "moderation:block-ban";
pub const BLOCK_UNBAN_FACT_TYPE_ID: &str = "moderation:block-unban";
pub const BLOCK_KICK_FACT_TYPE_ID: &str = "moderation:block-kick";
pub const BLOCK_GRANT_STEWARD_FACT_TYPE_ID: &str = "moderation:block-grant-steward";
pub const BLOCK_REVOKE_STEWARD_FACT_TYPE_ID: &str = "moderation:block-revoke-steward";

/// Fact representing a block-wide mute or channel-specific mute.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockMuteFact {
    pub context_id: ContextId,
    pub channel_id: Option<ChannelId>,
    pub muted_authority: AuthorityId,
    pub actor_authority: AuthorityId,
    pub duration_secs: Option<u64>,
    pub muted_at: PhysicalTime,
    pub expires_at: Option<PhysicalTime>,
}

impl BlockMuteFact {
    /// Backward-compat accessor for muted_at timestamp in milliseconds.
    pub fn muted_at_ms(&self) -> u64 {
        self.muted_at.ts_ms
    }

    /// Backward-compat accessor for expires_at timestamp in milliseconds.
    pub fn expires_at_ms(&self) -> Option<u64> {
        self.expires_at.as_ref().map(|t| t.ts_ms)
    }

    /// Backward-compat constructor using raw millisecond timestamps.
    pub fn new_ms(
        context_id: ContextId,
        channel_id: Option<ChannelId>,
        muted_authority: AuthorityId,
        actor_authority: AuthorityId,
        duration_secs: Option<u64>,
        muted_at_ms: u64,
        expires_at_ms: Option<u64>,
    ) -> Self {
        Self {
            context_id,
            channel_id,
            muted_authority,
            actor_authority,
            duration_secs,
            muted_at: PhysicalTime {
                ts_ms: muted_at_ms,
                uncertainty: None,
            },
            expires_at: expires_at_ms.map(|ts_ms| PhysicalTime {
                ts_ms,
                uncertainty: None,
            }),
        }
    }
}

impl DomainFact for BlockMuteFact {
    fn type_id(&self) -> &'static str {
        BLOCK_MUTE_FACT_TYPE_ID
    }

    fn context_id(&self) -> ContextId {
        self.context_id
    }

    fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("serialize block mute fact")
    }

    fn from_bytes(bytes: &[u8]) -> Option<Self> {
        serde_json::from_slice(bytes).ok()
    }
}

/// Fact representing the removal of a block mute.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockUnmuteFact {
    pub context_id: ContextId,
    pub channel_id: Option<ChannelId>,
    pub unmuted_authority: AuthorityId,
    pub actor_authority: AuthorityId,
    pub unmuted_at: PhysicalTime,
}

impl BlockUnmuteFact {
    /// Backward-compat accessor for unmuted_at timestamp in milliseconds.
    pub fn unmuted_at_ms(&self) -> u64 {
        self.unmuted_at.ts_ms
    }

    /// Backward-compat constructor using raw millisecond timestamps.
    pub fn new_ms(
        context_id: ContextId,
        channel_id: Option<ChannelId>,
        unmuted_authority: AuthorityId,
        actor_authority: AuthorityId,
        unmuted_at_ms: u64,
    ) -> Self {
        Self {
            context_id,
            channel_id,
            unmuted_authority,
            actor_authority,
            unmuted_at: PhysicalTime {
                ts_ms: unmuted_at_ms,
                uncertainty: None,
            },
        }
    }
}

impl DomainFact for BlockUnmuteFact {
    fn type_id(&self) -> &'static str {
        BLOCK_UNMUTE_FACT_TYPE_ID
    }

    fn context_id(&self) -> ContextId {
        self.context_id
    }

    fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("serialize block unmute fact")
    }

    fn from_bytes(bytes: &[u8]) -> Option<Self> {
        serde_json::from_slice(bytes).ok()
    }
}

/// Fact representing a block-wide ban or channel-specific ban.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockBanFact {
    pub context_id: ContextId,
    pub channel_id: Option<ChannelId>,
    pub banned_authority: AuthorityId,
    pub actor_authority: AuthorityId,
    pub reason: String,
    pub banned_at: PhysicalTime,
    pub expires_at: Option<PhysicalTime>,
}

impl BlockBanFact {
    /// Backward-compat accessor for banned_at timestamp in milliseconds.
    pub fn banned_at_ms(&self) -> u64 {
        self.banned_at.ts_ms
    }

    /// Backward-compat accessor for expires_at timestamp in milliseconds.
    pub fn expires_at_ms(&self) -> Option<u64> {
        self.expires_at.as_ref().map(|t| t.ts_ms)
    }

    /// Backward-compat constructor using raw millisecond timestamps.
    pub fn new_ms(
        context_id: ContextId,
        channel_id: Option<ChannelId>,
        banned_authority: AuthorityId,
        actor_authority: AuthorityId,
        reason: String,
        banned_at_ms: u64,
        expires_at_ms: Option<u64>,
    ) -> Self {
        Self {
            context_id,
            channel_id,
            banned_authority,
            actor_authority,
            reason,
            banned_at: PhysicalTime {
                ts_ms: banned_at_ms,
                uncertainty: None,
            },
            expires_at: expires_at_ms.map(|ts_ms| PhysicalTime {
                ts_ms,
                uncertainty: None,
            }),
        }
    }
}

impl DomainFact for BlockBanFact {
    fn type_id(&self) -> &'static str {
        BLOCK_BAN_FACT_TYPE_ID
    }

    fn context_id(&self) -> ContextId {
        self.context_id
    }

    fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("serialize block ban fact")
    }

    fn from_bytes(bytes: &[u8]) -> Option<Self> {
        serde_json::from_slice(bytes).ok()
    }
}

/// Fact representing the removal of a block ban.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockUnbanFact {
    pub context_id: ContextId,
    pub channel_id: Option<ChannelId>,
    pub unbanned_authority: AuthorityId,
    pub actor_authority: AuthorityId,
    pub unbanned_at: PhysicalTime,
}

impl BlockUnbanFact {
    /// Backward-compat accessor for unbanned_at timestamp in milliseconds.
    pub fn unbanned_at_ms(&self) -> u64 {
        self.unbanned_at.ts_ms
    }

    /// Backward-compat constructor using raw millisecond timestamps.
    pub fn new_ms(
        context_id: ContextId,
        channel_id: Option<ChannelId>,
        unbanned_authority: AuthorityId,
        actor_authority: AuthorityId,
        unbanned_at_ms: u64,
    ) -> Self {
        Self {
            context_id,
            channel_id,
            unbanned_authority,
            actor_authority,
            unbanned_at: PhysicalTime {
                ts_ms: unbanned_at_ms,
                uncertainty: None,
            },
        }
    }
}

impl DomainFact for BlockUnbanFact {
    fn type_id(&self) -> &'static str {
        BLOCK_UNBAN_FACT_TYPE_ID
    }

    fn context_id(&self) -> ContextId {
        self.context_id
    }

    fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("serialize block unban fact")
    }

    fn from_bytes(bytes: &[u8]) -> Option<Self> {
        serde_json::from_slice(bytes).ok()
    }
}

/// Fact representing a kick from a channel (audit log entry).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockKickFact {
    pub context_id: ContextId,
    pub channel_id: ChannelId,
    pub kicked_authority: AuthorityId,
    pub actor_authority: AuthorityId,
    pub reason: String,
    pub kicked_at: PhysicalTime,
}

impl BlockKickFact {
    /// Backward-compat accessor for kicked_at timestamp in milliseconds.
    pub fn kicked_at_ms(&self) -> u64 {
        self.kicked_at.ts_ms
    }

    /// Backward-compat constructor using raw millisecond timestamps.
    pub fn new_ms(
        context_id: ContextId,
        channel_id: ChannelId,
        kicked_authority: AuthorityId,
        actor_authority: AuthorityId,
        reason: String,
        kicked_at_ms: u64,
    ) -> Self {
        Self {
            context_id,
            channel_id,
            kicked_authority,
            actor_authority,
            reason,
            kicked_at: PhysicalTime {
                ts_ms: kicked_at_ms,
                uncertainty: None,
            },
        }
    }
}

impl DomainFact for BlockKickFact {
    fn type_id(&self) -> &'static str {
        BLOCK_KICK_FACT_TYPE_ID
    }

    fn context_id(&self) -> ContextId {
        self.context_id
    }

    fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("serialize block kick fact")
    }

    fn from_bytes(bytes: &[u8]) -> Option<Self> {
        serde_json::from_slice(bytes).ok()
    }
}

/// Fact representing granting steward (admin) privileges to a user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockGrantStewardFact {
    /// Block context where steward is being granted
    pub context_id: ContextId,
    /// Authority being granted steward status
    pub target_authority: AuthorityId,
    /// Authority performing the grant (must be existing steward or owner)
    pub actor_authority: AuthorityId,
    /// When steward was granted
    pub granted_at: PhysicalTime,
}

impl BlockGrantStewardFact {
    /// Accessor for granted_at timestamp in milliseconds.
    pub fn granted_at_ms(&self) -> u64 {
        self.granted_at.ts_ms
    }

    /// Constructor using raw millisecond timestamps.
    pub fn new_ms(
        context_id: ContextId,
        target_authority: AuthorityId,
        actor_authority: AuthorityId,
        granted_at_ms: u64,
    ) -> Self {
        Self {
            context_id,
            target_authority,
            actor_authority,
            granted_at: PhysicalTime {
                ts_ms: granted_at_ms,
                uncertainty: None,
            },
        }
    }
}

impl DomainFact for BlockGrantStewardFact {
    fn type_id(&self) -> &'static str {
        BLOCK_GRANT_STEWARD_FACT_TYPE_ID
    }

    fn context_id(&self) -> ContextId {
        self.context_id
    }

    fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("serialize block grant steward fact")
    }

    fn from_bytes(bytes: &[u8]) -> Option<Self> {
        serde_json::from_slice(bytes).ok()
    }
}

/// Fact representing revoking steward (admin) privileges from a user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockRevokeStewardFact {
    /// Block context where steward is being revoked
    pub context_id: ContextId,
    /// Authority having steward status revoked
    pub target_authority: AuthorityId,
    /// Authority performing the revocation (must be existing steward or owner)
    pub actor_authority: AuthorityId,
    /// When steward was revoked
    pub revoked_at: PhysicalTime,
}

impl BlockRevokeStewardFact {
    /// Accessor for revoked_at timestamp in milliseconds.
    pub fn revoked_at_ms(&self) -> u64 {
        self.revoked_at.ts_ms
    }

    /// Constructor using raw millisecond timestamps.
    pub fn new_ms(
        context_id: ContextId,
        target_authority: AuthorityId,
        actor_authority: AuthorityId,
        revoked_at_ms: u64,
    ) -> Self {
        Self {
            context_id,
            target_authority,
            actor_authority,
            revoked_at: PhysicalTime {
                ts_ms: revoked_at_ms,
                uncertainty: None,
            },
        }
    }
}

impl DomainFact for BlockRevokeStewardFact {
    fn type_id(&self) -> &'static str {
        BLOCK_REVOKE_STEWARD_FACT_TYPE_ID
    }

    fn context_id(&self) -> ContextId {
        self.context_id
    }

    fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("serialize block revoke steward fact")
    }

    fn from_bytes(bytes: &[u8]) -> Option<Self> {
        serde_json::from_slice(bytes).ok()
    }
}

struct BlockMuteFactReducer;

impl FactReducer for BlockMuteFactReducer {
    fn handles_type(&self) -> &'static str {
        BLOCK_MUTE_FACT_TYPE_ID
    }

    fn reduce(
        &self,
        context_id: ContextId,
        binding_type: &str,
        binding_data: &[u8],
    ) -> Option<RelationalBinding> {
        if binding_type != BLOCK_MUTE_FACT_TYPE_ID {
            return None;
        }

        let fact = BlockMuteFact::from_bytes(binding_data)?;
        Some(RelationalBinding {
            binding_type: RelationalBindingType::Generic(BLOCK_MUTE_FACT_TYPE_ID.to_string()),
            context_id,
            data: fact.to_bytes(),
        })
    }
}

struct BlockUnmuteFactReducer;

impl FactReducer for BlockUnmuteFactReducer {
    fn handles_type(&self) -> &'static str {
        BLOCK_UNMUTE_FACT_TYPE_ID
    }

    fn reduce(
        &self,
        context_id: ContextId,
        binding_type: &str,
        binding_data: &[u8],
    ) -> Option<RelationalBinding> {
        if binding_type != BLOCK_UNMUTE_FACT_TYPE_ID {
            return None;
        }

        let fact = BlockUnmuteFact::from_bytes(binding_data)?;
        Some(RelationalBinding {
            binding_type: RelationalBindingType::Generic(BLOCK_UNMUTE_FACT_TYPE_ID.to_string()),
            context_id,
            data: fact.to_bytes(),
        })
    }
}

struct BlockBanFactReducer;

impl FactReducer for BlockBanFactReducer {
    fn handles_type(&self) -> &'static str {
        BLOCK_BAN_FACT_TYPE_ID
    }

    fn reduce(
        &self,
        context_id: ContextId,
        binding_type: &str,
        binding_data: &[u8],
    ) -> Option<RelationalBinding> {
        if binding_type != BLOCK_BAN_FACT_TYPE_ID {
            return None;
        }

        let fact = BlockBanFact::from_bytes(binding_data)?;
        Some(RelationalBinding {
            binding_type: RelationalBindingType::Generic(BLOCK_BAN_FACT_TYPE_ID.to_string()),
            context_id,
            data: fact.to_bytes(),
        })
    }
}

struct BlockUnbanFactReducer;

impl FactReducer for BlockUnbanFactReducer {
    fn handles_type(&self) -> &'static str {
        BLOCK_UNBAN_FACT_TYPE_ID
    }

    fn reduce(
        &self,
        context_id: ContextId,
        binding_type: &str,
        binding_data: &[u8],
    ) -> Option<RelationalBinding> {
        if binding_type != BLOCK_UNBAN_FACT_TYPE_ID {
            return None;
        }

        let fact = BlockUnbanFact::from_bytes(binding_data)?;
        Some(RelationalBinding {
            binding_type: RelationalBindingType::Generic(BLOCK_UNBAN_FACT_TYPE_ID.to_string()),
            context_id,
            data: fact.to_bytes(),
        })
    }
}

struct BlockKickFactReducer;

impl FactReducer for BlockKickFactReducer {
    fn handles_type(&self) -> &'static str {
        BLOCK_KICK_FACT_TYPE_ID
    }

    fn reduce(
        &self,
        context_id: ContextId,
        binding_type: &str,
        binding_data: &[u8],
    ) -> Option<RelationalBinding> {
        if binding_type != BLOCK_KICK_FACT_TYPE_ID {
            return None;
        }

        let fact = BlockKickFact::from_bytes(binding_data)?;
        Some(RelationalBinding {
            binding_type: RelationalBindingType::Generic(BLOCK_KICK_FACT_TYPE_ID.to_string()),
            context_id,
            data: fact.to_bytes(),
        })
    }
}

/// Register moderation domain facts with the journal registry.
pub fn register_moderation_facts(registry: &mut FactRegistry) {
    registry.register::<BlockMuteFact>(BLOCK_MUTE_FACT_TYPE_ID, Box::new(BlockMuteFactReducer));
    registry
        .register::<BlockUnmuteFact>(BLOCK_UNMUTE_FACT_TYPE_ID, Box::new(BlockUnmuteFactReducer));
    registry.register::<BlockBanFact>(BLOCK_BAN_FACT_TYPE_ID, Box::new(BlockBanFactReducer));
    registry.register::<BlockUnbanFact>(BLOCK_UNBAN_FACT_TYPE_ID, Box::new(BlockUnbanFactReducer));
    registry.register::<BlockKickFact>(BLOCK_KICK_FACT_TYPE_ID, Box::new(BlockKickFactReducer));
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::identifiers::AuthorityId;
    use aura_journal::reduction::RelationalBindingType;
    use aura_journal::FactRegistry;

    fn test_context_id() -> ContextId {
        ContextId::new_from_entropy([7u8; 32])
    }

    fn test_authority_id(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    fn pt(ts_ms: u64) -> PhysicalTime {
        PhysicalTime {
            ts_ms,
            uncertainty: None,
        }
    }

    #[test]
    fn moderation_facts_register_with_registry() {
        let mut registry = FactRegistry::new();
        register_moderation_facts(&mut registry);

        assert!(registry.is_registered(BLOCK_MUTE_FACT_TYPE_ID));
        assert!(registry.is_registered(BLOCK_UNMUTE_FACT_TYPE_ID));

        let context_id = test_context_id();
        let block_mute = BlockMuteFact {
            context_id,
            channel_id: None,
            muted_authority: test_authority_id(1),
            actor_authority: test_authority_id(2),
            duration_secs: Some(30),
            muted_at: pt(1000),
            expires_at: Some(pt(31000)),
        };

        let binding = registry.reduce_generic(
            block_mute.context_id,
            BLOCK_MUTE_FACT_TYPE_ID,
            &block_mute.to_bytes(),
        );

        assert_eq!(
            binding.binding_type,
            RelationalBindingType::Generic(BLOCK_MUTE_FACT_TYPE_ID.to_string())
        );
        assert_eq!(binding.data, block_mute.to_bytes());

        let block_unmute = BlockUnmuteFact {
            context_id,
            channel_id: None,
            unmuted_authority: block_mute.muted_authority,
            actor_authority: block_mute.actor_authority,
            unmuted_at: pt(2000),
        };

        let binding = registry.reduce_generic(
            block_unmute.context_id,
            BLOCK_UNMUTE_FACT_TYPE_ID,
            &block_unmute.to_bytes(),
        );

        assert_eq!(
            binding.binding_type,
            RelationalBindingType::Generic(BLOCK_UNMUTE_FACT_TYPE_ID.to_string())
        );
        assert_eq!(binding.data, block_unmute.to_bytes());
    }
}
