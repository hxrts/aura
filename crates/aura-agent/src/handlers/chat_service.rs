//! Chat Service - Agent API for chat operations
//!
//! This service uses the fact-first chat path: operations emit `ChatFact` values
//! and commit them into the journal as `RelationalFact::Generic`.
//!
//! The legacy KV-backed `aura_chat::ChatHandler` remains in the `aura-chat` crate
//! as a local-only handler, but it is not used by the agent/terminal default path.

use crate::core::{AgentError, AgentResult};
use crate::runtime::AuraEffectSystem;
use aura_chat::guards::{EffectCommand, GuardOutcome, GuardSnapshot};
use aura_chat::types::{ChatMember, ChatRole};
use aura_chat::{
    ChatFactService, ChatGroup, ChatGroupId, ChatMessage, ChatMessageId, CHAT_FACT_TYPE_ID,
};
use aura_core::effects::{PhysicalTimeEffects, RandomEffects};
use aura_core::hash::hash;
use aura_core::identifiers::{AuthorityId, ChannelId, ContextId};
use aura_core::time::{PhysicalTime, TimeStamp};
use aura_journal::DomainFact;
use aura_protocol::guards::GuardContextProvider;
use uuid::Uuid;

/// Chat service for the agent layer.
///
/// The service commits chat facts into the agent's canonical fact store
/// (`AuraEffectSystem::commit_generic_fact_bytes`) so reactive views and sync
/// pipelines can observe them.
pub struct ChatService {
    effects: std::sync::Arc<AuraEffectSystem>,
    facts: ChatFactService,
}

impl ChatService {
    /// Create a new chat service.
    pub fn new(effects: std::sync::Arc<AuraEffectSystem>) -> Self {
        Self {
            effects,
            facts: ChatFactService::new(),
        }
    }

    fn channel_id_for_group(group_id: &ChatGroupId) -> ChannelId {
        // Deterministic mapping: embed the group UUID twice into a 32-byte ChannelId.
        // This makes the mapping stable across runs without consuming entropy.
        let mut bytes = [0u8; 32];
        bytes[..16].copy_from_slice(group_id.0.as_bytes());
        bytes[16..].copy_from_slice(group_id.0.as_bytes());
        ChannelId::from_bytes(bytes)
    }

    fn context_id_for_group(group_id: &ChatGroupId) -> ContextId {
        ContextId::from_uuid(group_id.0)
    }

    async fn build_snapshot(
        &self,
        authority_id: AuthorityId,
        context_id: ContextId,
    ) -> AgentResult<GuardSnapshot> {
        let now = self
            .effects
            .physical_time()
            .await
            .map_err(|e| AgentError::effects(format!("time error: {e}")))?;

        // NOTE: Authorization/capability integration for chat is not yet wired to Biscuit/WoT.
        // The current runtime treats guard capabilities as permissive; we provide the required
        // strings so guards can evolve without breaking call sites.
        let capabilities = vec![
            aura_chat::guards::costs::CAP_CHAT_CHANNEL_CREATE.to_string(),
            aura_chat::guards::costs::CAP_CHAT_MESSAGE_SEND.to_string(),
        ];

        Ok(GuardSnapshot::new(
            authority_id,
            context_id,
            u32::MAX,
            capabilities,
            now.ts_ms,
        ))
    }

    async fn execute_outcome(&self, outcome: GuardOutcome) -> AgentResult<()> {
        if outcome.is_denied() {
            let reason = outcome
                .decision
                .denial_reason()
                .unwrap_or("Operation denied");
            return Err(AgentError::effects(format!(
                "Guard denied operation: {reason}"
            )));
        }

        for effect in outcome.effects {
            match effect {
                EffectCommand::ChargeFlowBudget { cost } => {
                    // Chat is local-first: flow budget charging happens at transport-layer
                    // send-time via the guard-chain (CapGuard → FlowGuard → JournalCoupler).
                    // Local fact commits don't require flow charging since there's no peer
                    // recipient at commit time. The cost is tracked here for observability
                    // but actual budget deduction occurs when facts sync to peers.
                    tracing::trace!(
                        cost,
                        "Chat fact commit - flow cost tracked for sync-time charging"
                    );
                }
                EffectCommand::JournalAppend { fact } => {
                    self.effects
                        .commit_generic_fact_bytes(
                            fact.context_id(),
                            CHAT_FACT_TYPE_ID,
                            fact.to_bytes(),
                        )
                        .await
                        .map_err(AgentError::from)?;
                }
            }
        }

        Ok(())
    }

    async fn load_group_facts(
        &self,
        group_id: &ChatGroupId,
    ) -> AgentResult<Vec<aura_chat::ChatFact>> {
        let context_id = Self::context_id_for_group(group_id);
        let channel_id = Self::channel_id_for_group(group_id);

        let typed = self
            .effects
            .load_committed_facts(self.effects.authority_id())
            .await
            .map_err(AgentError::from)?;

        let mut out = Vec::new();
        for fact in typed {
            let aura_journal::fact::FactContent::Relational(
                aura_journal::fact::RelationalFact::Generic {
                    context_id: ctx,
                    binding_type,
                    binding_data,
                },
            ) = fact.content
            else {
                continue;
            };

            if ctx != context_id || binding_type != CHAT_FACT_TYPE_ID {
                continue;
            }

            let Some(chat_fact) = aura_chat::ChatFact::from_bytes(&binding_data) else {
                continue;
            };

            // Restrict to the single channel for this group mapping.
            match &chat_fact {
                aura_chat::ChatFact::ChannelCreated { channel_id: c, .. }
                | aura_chat::ChatFact::ChannelClosed { channel_id: c, .. }
                | aura_chat::ChatFact::ChannelUpdated { channel_id: c, .. }
                | aura_chat::ChatFact::MessageSentSealed { channel_id: c, .. }
                | aura_chat::ChatFact::MessageRead { channel_id: c, .. }
                | aura_chat::ChatFact::MessageDelivered { channel_id: c, .. }
                | aura_chat::ChatFact::DeliveryAcknowledged { channel_id: c, .. } => {
                    if *c != channel_id {
                        continue;
                    }
                }
            }

            out.push(chat_fact);
        }

        Ok(out)
    }

    // =========================================================================
    // Public API (terminal/tests)
    // =========================================================================

    /// Create a new chat group.
    ///
    /// This commits a `ChatFact::ChannelCreated` fact and returns a `ChatGroup`
    /// object for convenience.
    pub async fn create_group(
        &self,
        name: &str,
        creator_id: AuthorityId,
        initial_members: Vec<AuthorityId>,
    ) -> AgentResult<ChatGroup> {
        let group_uuid = self.effects.random_uuid().await;
        let group_id = ChatGroupId::from_uuid(group_uuid);

        let context_id = Self::context_id_for_group(&group_id);
        let channel_id = Self::channel_id_for_group(&group_id);

        let snapshot = self.build_snapshot(creator_id, context_id).await?;
        let outcome =
            self.facts
                .prepare_create_channel(&snapshot, channel_id, name.to_string(), None, false);
        self.execute_outcome(outcome).await?;

        let created_at = TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: snapshot.now_ms,
            uncertainty: None,
        });

        let mut members = Vec::new();
        members.push(ChatMember {
            authority_id: creator_id,
            display_name: creator_id.to_string(),
            joined_at: created_at.clone(),
            role: ChatRole::Admin,
        });
        for member in initial_members {
            if member == creator_id {
                continue;
            }
            members.push(ChatMember {
                authority_id: member,
                display_name: member.to_string(),
                joined_at: created_at.clone(),
                role: ChatRole::Member,
            });
        }

        Ok(ChatGroup {
            id: group_id,
            name: name.to_string(),
            description: String::new(),
            created_at,
            created_by: creator_id,
            members,
            metadata: Default::default(),
        })
    }

    /// Send a message to a group.
    ///
    /// Commits a `ChatFact::MessageSentSealed` fact (payload is opaque bytes).
    pub async fn send_message(
        &self,
        group_id: &ChatGroupId,
        sender_id: AuthorityId,
        content: String,
    ) -> AgentResult<ChatMessage> {
        let context_id = Self::context_id_for_group(group_id);
        let channel_id = Self::channel_id_for_group(group_id);

        let snapshot = self.build_snapshot(sender_id, context_id).await?;

        let message_uuid = self.effects.random_uuid().await;
        let message_id = message_uuid.to_string();

        let outcome = self.facts.prepare_send_message_sealed(
            &snapshot,
            channel_id,
            message_id.clone(),
            sender_id.to_string(),
            content.clone().into_bytes(),
            None,
        );
        self.execute_outcome(outcome).await?;

        Ok(ChatMessage::new_text(
            ChatMessageId(message_uuid),
            group_id.clone(),
            sender_id,
            content,
            TimeStamp::PhysicalClock(PhysicalTime {
                ts_ms: snapshot.now_ms,
                uncertainty: None,
            }),
        ))
    }

    /// Get message history for a group.
    pub async fn get_history(
        &self,
        group_id: &ChatGroupId,
        limit: Option<usize>,
        before: Option<TimeStamp>,
    ) -> AgentResult<Vec<ChatMessage>> {
        let facts = self.load_group_facts(group_id).await?;

        let mut messages = Vec::new();
        for fact in facts {
            let aura_chat::ChatFact::MessageSentSealed {
                message_id,
                sender_id,
                payload,
                sent_at,
                reply_to,
                ..
            } = fact
            else {
                continue;
            };

            let timestamp = TimeStamp::PhysicalClock(sent_at);
            if let Some(before_ts) = &before {
                if timestamp.to_index_ms() >= before_ts.to_index_ms() {
                    continue;
                }
            }

            let msg_uuid = Uuid::parse_str(&message_id).unwrap_or_else(|_| {
                let h = hash(message_id.as_bytes());
                let mut uuid_bytes = [0u8; 16];
                uuid_bytes.copy_from_slice(&h[..16]);
                Uuid::from_bytes(uuid_bytes)
            });

            let payload_len = payload.len();
            let content = String::from_utf8(payload)
                .unwrap_or_else(|_| format!("[sealed: {} bytes]", payload_len));

            let mut msg = ChatMessage::new_text(
                ChatMessageId(msg_uuid),
                group_id.clone(),
                sender_id,
                content,
                timestamp,
            );

            if let Some(reply_str) = reply_to {
                if let Ok(reply_uuid) = Uuid::parse_str(&reply_str) {
                    msg = msg.set_reply_to(ChatMessageId(reply_uuid));
                }
            }

            messages.push(msg);
        }

        // Sort by timestamp for stable history
        messages.sort_by_key(|m| m.timestamp.to_index_ms());

        if let Some(limit) = limit {
            if messages.len() > limit {
                messages = messages.into_iter().rev().take(limit).collect();
                messages.reverse();
            }
        }

        Ok(messages)
    }

    /// Get a chat group by ID.
    ///
    /// Reconstructs a minimal view from the `ChannelCreated` fact.
    pub async fn get_group(&self, group_id: &ChatGroupId) -> AgentResult<Option<ChatGroup>> {
        let facts = self.load_group_facts(group_id).await?;

        let mut created: Option<(String, AuthorityId, PhysicalTime)> = None;
        for fact in facts {
            if let aura_chat::ChatFact::ChannelCreated {
                name,
                creator_id,
                created_at,
                ..
            } = fact
            {
                created = Some((name, creator_id, created_at));
            }
        }

        let Some((name, creator_id, created_at)) = created else {
            return Ok(None);
        };

        let created_at_ts = TimeStamp::PhysicalClock(created_at);
        Ok(Some(ChatGroup {
            id: group_id.clone(),
            name,
            description: String::new(),
            created_at: created_at_ts.clone(),
            created_by: creator_id,
            members: vec![ChatMember {
                authority_id: creator_id,
                display_name: creator_id.to_string(),
                joined_at: created_at_ts,
                role: ChatRole::Admin,
            }],
            metadata: Default::default(),
        }))
    }

    /// List groups that this authority has created/observed locally.
    pub async fn list_user_groups(
        &self,
        _authority_id: &AuthorityId,
    ) -> AgentResult<Vec<ChatGroup>> {
        let typed = self
            .effects
            .load_committed_facts(self.effects.authority_id())
            .await
            .map_err(AgentError::from)?;

        let mut by_group: std::collections::HashMap<
            ChatGroupId,
            (String, AuthorityId, PhysicalTime),
        > = std::collections::HashMap::new();

        for fact in typed {
            let aura_journal::fact::FactContent::Relational(
                aura_journal::fact::RelationalFact::Generic {
                    context_id,
                    binding_type,
                    binding_data,
                },
            ) = fact.content
            else {
                continue;
            };
            if binding_type != CHAT_FACT_TYPE_ID {
                continue;
            }
            let Some(chat_fact) = aura_chat::ChatFact::from_bytes(&binding_data) else {
                continue;
            };
            let aura_chat::ChatFact::ChannelCreated {
                context_id: fact_ctx,
                name,
                creator_id,
                created_at,
                ..
            } = chat_fact
            else {
                continue;
            };
            if context_id != fact_ctx {
                continue;
            }

            let group_uuid = Uuid::from_bytes(fact_ctx.to_bytes());
            let group_id = ChatGroupId::from_uuid(group_uuid);
            by_group.insert(group_id, (name, creator_id, created_at));
        }

        let mut groups: Vec<ChatGroup> = by_group
            .into_iter()
            .map(|(id, (name, creator_id, created_at))| {
                let created_at_ts = TimeStamp::PhysicalClock(created_at);
                ChatGroup {
                    id,
                    name,
                    description: String::new(),
                    created_at: created_at_ts.clone(),
                    created_by: creator_id,
                    members: vec![ChatMember {
                        authority_id: creator_id,
                        display_name: creator_id.to_string(),
                        joined_at: created_at_ts,
                        role: ChatRole::Admin,
                    }],
                    metadata: Default::default(),
                }
            })
            .collect();

        groups.sort_by_key(|g| g.created_at.to_index_ms());
        Ok(groups)
    }

    // =========================================================================
    // Legacy operations (not yet fact-backed)
    // =========================================================================

    pub async fn add_member(
        &self,
        _group_id: &ChatGroupId,
        _authority_id: AuthorityId,
        _new_member: AuthorityId,
    ) -> AgentResult<()> {
        Err(AgentError::effects(
            "Chat membership operations are not yet fact-backed",
        ))
    }

    pub async fn remove_member(
        &self,
        _group_id: &ChatGroupId,
        _authority_id: AuthorityId,
        _member_to_remove: AuthorityId,
    ) -> AgentResult<()> {
        Err(AgentError::effects(
            "Chat membership operations are not yet fact-backed",
        ))
    }

    pub async fn get_message(
        &self,
        _message_id: &ChatMessageId,
    ) -> AgentResult<Option<ChatMessage>> {
        Err(AgentError::effects(
            "Message lookup by ID is not yet fact-backed",
        ))
    }

    pub async fn edit_message(
        &self,
        _group_id: &ChatGroupId,
        _editor: AuthorityId,
        _message_id: &ChatMessageId,
        _new_content: &str,
    ) -> AgentResult<ChatMessage> {
        Err(AgentError::effects("Message edits are not yet fact-backed"))
    }

    pub async fn delete_message(
        &self,
        _group_id: &ChatGroupId,
        _requester: AuthorityId,
        _message_id: &ChatMessageId,
    ) -> AgentResult<()> {
        Err(AgentError::effects(
            "Message deletion is not yet fact-backed",
        ))
    }

    pub async fn search_messages(
        &self,
        _group_id: &ChatGroupId,
        _query: &str,
        _limit: usize,
        _sender: Option<&AuthorityId>,
    ) -> AgentResult<Vec<ChatMessage>> {
        Err(AgentError::effects("Message search is not yet fact-backed"))
    }

    pub async fn update_group_details(
        &self,
        _group_id: &ChatGroupId,
        _requester: AuthorityId,
        _name: Option<String>,
        _description: Option<String>,
        _metadata: Option<std::collections::HashMap<String, String>>,
    ) -> AgentResult<ChatGroup> {
        Err(AgentError::effects(
            "Group metadata updates are not yet fact-backed",
        ))
    }
}
