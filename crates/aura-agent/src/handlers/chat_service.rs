//! Chat Service - Agent API for chat operations
//!
//! This service uses the fact-first chat path: operations emit `ChatFact` values
//! and commit them into the journal as `RelationalFact::Generic`.
//!
//! All chat operations go through `ChatFactService` which provides guard chain
//! integration (capability checks, flow budget charging, fact emission).

use crate::core::{AgentError, AgentResult};
use crate::runtime::AuraEffectSystem;
use aura_chat::guards::{EffectCommand, GuardOutcome, GuardSnapshot};
use aura_chat::types::{ChatMember, ChatRole};
use aura_chat::{
    ChatFactService, ChatGroup, ChatGroupId, ChatMessage, ChatMessageId, CHAT_FACT_TYPE_ID,
};
use aura_core::effects::amp::{
    AmpChannelEffects, ChannelCreateParams, ChannelJoinParams, ChannelLeaveParams,
    ChannelSendParams,
};
use aura_core::effects::{PhysicalTimeEffects, RandomExtendedEffects};
use aura_core::hash::hash;
use aura_core::identifiers::{AuthorityId, ChannelId, ContextId};
use aura_core::time::{PhysicalTime, TimeStamp};
use aura_guards::GuardContextProvider;
use aura_journal::fact::{CommittedChannelEpochBump, RelationalFact};
use aura_journal::DomainFact;
use aura_protocol::amp::{get_channel_state, AmpChannelCoordinator, AmpJournalEffects};
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

    /// Create an AMP channel coordinator for this service.
    ///
    /// The coordinator handles AMP channel lifecycle and encryption.
    fn amp_coordinator(&self) -> AmpChannelCoordinator<std::sync::Arc<AuraEffectSystem>> {
        AmpChannelCoordinator::new(self.effects.clone())
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
                | aura_chat::ChatFact::DeliveryAcknowledged { channel_id: c, .. }
                | aura_chat::ChatFact::MessageEdited { channel_id: c, .. }
                | aura_chat::ChatFact::MessageDeleted { channel_id: c, .. } => {
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
    /// This creates an AMP channel for encryption and commits a `ChatFact::ChannelCreated`
    /// fact for the chat layer.
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

        // Create the AMP channel for message encryption
        let amp = self.amp_coordinator();
        amp.create_channel(ChannelCreateParams {
            context: context_id,
            channel: Some(channel_id),
            skip_window: None,
            topic: Some(name.to_string()),
        })
        .await
        .map_err(|e| AgentError::effects(format!("AMP channel creation failed: {e}")))?;

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
    /// Validates the AMP channel exists and commits a `ChatFact::MessageSentSealed` fact.
    /// Note: AMP transport encryption happens at sync time (not local storage).
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

        // Validate the AMP channel exists (created in create_group)
        // Transport-layer encryption will use AMP when syncing to peers
        let amp = self.amp_coordinator();
        let _amp_ciphertext = amp
            .send_message(ChannelSendParams {
                context: context_id,
                channel: channel_id,
                sender: sender_id,
                plaintext: content.clone().into_bytes(),
                reply_to: None,
            })
            .await
            .map_err(|e| AgentError::effects(format!("AMP channel validation failed: {e}")))?;

        // Store plaintext locally; encryption happens at transport/sync time
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
    // Operations requiring ceremony infrastructure (per docs/117_operation_categories.md):
    // - add_member: Category C (ceremony required for group key rotation)
    // - remove_member: Category B/C depending on context
    // - edit_message: Category A (emit EditFact)
    // - delete_message: Category B (deferred approval)
    // =========================================================================

    /// Add a member to a chat group (Category C operation)
    ///
    /// Per docs/117_operation_categories.md, membership changes are Category C:
    /// they require AMP channel membership updates.
    ///
    /// Key distribution is implicit: When a new member syncs their journal with
    /// the group, they receive the `ChannelCheckpoint` and `CommittedChannelEpochBump`
    /// facts that define the current channel state. The AMP keystream derivation
    /// uses the channel ID, epoch, and ratchet generation from these facts, so
    /// new members can decrypt messages once their journal is synchronized.
    ///
    /// Flow:
    /// 1. Record membership fact via AMP channel join
    /// 2. New member syncs journal and receives channel state facts
    /// 3. Keystream derivation works automatically from shared channel state
    pub async fn add_member(
        &self,
        group_id: &ChatGroupId,
        _requester: AuthorityId,
        new_member: AuthorityId,
    ) -> AgentResult<()> {
        let context_id = Self::context_id_for_group(group_id);
        let channel_id = Self::channel_id_for_group(group_id);

        // Record membership via AMP channel join
        // Key distribution happens implicitly when the new member syncs their
        // journal - they receive the channel state facts needed to derive keystream
        let amp = self.amp_coordinator();
        amp.join_channel(ChannelJoinParams {
            context: context_id,
            channel: channel_id,
            participant: new_member,
        })
        .await
        .map_err(|e| AgentError::effects(format!("Failed to add member: {e}")))?;

        tracing::info!(
            group_id = %group_id,
            new_member = %new_member,
            "Member added to chat group (key access via journal sync)"
        );

        Ok(())
    }

    /// Remove a member from a chat group (Category C operation)
    ///
    /// Per docs/117_operation_categories.md, membership changes are Category C.
    /// Member removal triggers key rotation via epoch bump so the removed member
    /// cannot decrypt future messages.
    ///
    /// Flow:
    /// 1. Record membership change via AMP channel leave
    /// 2. Bump channel epoch to rotate encryption key
    pub async fn remove_member(
        &self,
        group_id: &ChatGroupId,
        _requester: AuthorityId,
        member_to_remove: AuthorityId,
    ) -> AgentResult<()> {
        let context_id = Self::context_id_for_group(group_id);
        let channel_id = Self::channel_id_for_group(group_id);

        // Use AMP channel leave to record membership change
        let amp = self.amp_coordinator();
        amp.leave_channel(ChannelLeaveParams {
            context: context_id,
            channel: channel_id,
            participant: member_to_remove,
        })
        .await
        .map_err(|e| AgentError::effects(format!("Failed to remove member: {e}")))?;

        // Key rotation ceremony: Bump the channel epoch so the removed member
        // cannot decrypt messages sent after this point. The keystream derivation
        // uses chan_epoch as input, so advancing the epoch effectively rotates
        // the encryption key.
        let state = get_channel_state(self.effects.as_ref(), context_id, channel_id)
            .await
            .map_err(|e| AgentError::effects(format!("Failed to get channel state: {e}")))?;

        let committed = CommittedChannelEpochBump {
            context: context_id,
            channel: channel_id,
            parent_epoch: state.chan_epoch,
            new_epoch: state.chan_epoch + 1,
            // For membership-driven bumps, use zeroed identifiers (no consensus proposal)
            chosen_bump_id: Default::default(),
            consensus_id: Default::default(),
        };

        self.effects
            .insert_relational_fact(RelationalFact::AmpCommittedChannelEpochBump(committed))
            .await
            .map_err(|e| AgentError::effects(format!("Failed to commit epoch bump: {e}")))?;

        tracing::info!(
            group_id = %group_id,
            removed_member = %member_to_remove,
            new_epoch = state.chan_epoch + 1,
            "Member removed from chat group with key rotation"
        );

        Ok(())
    }

    pub async fn get_message(
        &self,
        _message_id: &ChatMessageId,
    ) -> AgentResult<Option<ChatMessage>> {
        Err(AgentError::effects(
            "Message lookup by ID is not yet fact-backed",
        ))
    }

    /// Edit a message (Category A operation - optimistic)
    ///
    /// Per docs/117_operation_categories.md, message edits are Category A:
    /// just emit a MessageEdited fact. The original message remains in the journal;
    /// clients display the latest edit for each message_id.
    pub async fn edit_message(
        &self,
        group_id: &ChatGroupId,
        editor: AuthorityId,
        message_id: &ChatMessageId,
        new_content: &str,
    ) -> AgentResult<ChatMessage> {
        let context_id = Self::context_id_for_group(group_id);
        let channel_id = Self::channel_id_for_group(group_id);

        // Get current time
        let now = self
            .effects
            .physical_time()
            .await
            .map_err(|e| AgentError::effects(format!("time error: {e}")))?;

        // Create and commit the edit fact
        let edit_fact = aura_chat::ChatFact::message_edited_ms(
            context_id,
            channel_id,
            message_id.to_string(),
            editor,
            new_content.as_bytes().to_vec(),
            now.ts_ms,
        );

        self.effects
            .commit_generic_fact_bytes(context_id, CHAT_FACT_TYPE_ID, edit_fact.to_bytes())
            .await
            .map_err(|e| AgentError::effects(format!("Failed to commit edit fact: {e}")))?;

        // Return the updated message representation
        Ok(ChatMessage {
            id: message_id.clone(),
            group_id: group_id.clone(),
            sender_id: editor,
            content: new_content.to_string(),
            message_type: aura_chat::types::MessageType::Edit,
            timestamp: TimeStamp::PhysicalClock(now),
            reply_to: None,
            metadata: Default::default(),
        })
    }

    /// Delete a message (Category B operation - may require deferred approval)
    ///
    /// Per docs/117_operation_categories.md, message deletion is Category B:
    /// emit a MessageDeleted fact. Depending on channel policy, this may
    /// require approval from channel moderators.
    pub async fn delete_message(
        &self,
        group_id: &ChatGroupId,
        requester: AuthorityId,
        message_id: &ChatMessageId,
    ) -> AgentResult<()> {
        let context_id = Self::context_id_for_group(group_id);
        let channel_id = Self::channel_id_for_group(group_id);

        // Get current time
        let now = self
            .effects
            .physical_time()
            .await
            .map_err(|e| AgentError::effects(format!("time error: {e}")))?;

        // Create and commit the delete fact
        let delete_fact = aura_chat::ChatFact::message_deleted_ms(
            context_id,
            channel_id,
            message_id.to_string(),
            requester,
            now.ts_ms,
        );

        self.effects
            .commit_generic_fact_bytes(context_id, CHAT_FACT_TYPE_ID, delete_fact.to_bytes())
            .await
            .map_err(|e| AgentError::effects(format!("Failed to commit delete fact: {e}")))?;

        Ok(())
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

    /// Update group details (Category A operation - optimistic)
    ///
    /// Per docs/117_operation_categories.md, topic/name updates are Category A:
    /// CRDT semantics with last-write-wins resolution.
    pub async fn update_group_details(
        &self,
        group_id: &ChatGroupId,
        requester: AuthorityId,
        name: Option<String>,
        description: Option<String>,
        _metadata: Option<std::collections::HashMap<String, String>>,
    ) -> AgentResult<ChatGroup> {
        let context_id = Self::context_id_for_group(group_id);
        let channel_id = Self::channel_id_for_group(group_id);

        // Get current time
        let now = self
            .effects
            .physical_time()
            .await
            .map_err(|e| AgentError::effects(format!("time error: {e}")))?;

        // Create and commit the update fact
        // Note: ChatFact::ChannelUpdated uses "topic" for what UI calls "description"
        let update_fact = aura_chat::ChatFact::channel_updated_ms(
            context_id,
            channel_id,
            name.clone(),
            description.clone(), // Maps to topic in the fact
            now.ts_ms,
            requester,
        );

        self.effects
            .commit_generic_fact_bytes(context_id, CHAT_FACT_TYPE_ID, update_fact.to_bytes())
            .await
            .map_err(|e| AgentError::effects(format!("Failed to commit update fact: {e}")))?;

        // Return the updated group representation
        // Note: This returns the requested changes; actual state comes from reducing all facts
        Ok(ChatGroup {
            id: group_id.clone(),
            name: name.unwrap_or_default(),
            description: description.unwrap_or_default(),
            created_at: TimeStamp::PhysicalClock(now), // Would need to fetch actual created_at
            created_by: requester,                     // Would need to fetch actual creator
            members: vec![],                           // Would need to fetch actual members
            metadata: Default::default(),
        })
    }
}
