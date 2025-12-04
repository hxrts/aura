use aura_core::identifiers::{AuthorityId, ChannelId, ContextId};
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

/// Fact representing a block-wide mute or channel-specific mute.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockMuteFact {
    pub context_id: ContextId,
    pub channel_id: Option<ChannelId>,
    pub muted_authority: AuthorityId,
    pub actor_authority: AuthorityId,
    pub duration_secs: Option<u64>,
    pub muted_at_ms: u64,
    pub expires_at_ms: Option<u64>,
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
    pub unmuted_at_ms: u64,
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
    pub banned_at_ms: u64,
    pub expires_at_ms: Option<u64>,
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
    pub unbanned_at_ms: u64,
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
    pub kicked_at_ms: u64,
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
            muted_at_ms: 1000,
            expires_at_ms: Some(31000),
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
            unmuted_at_ms: 2000,
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
