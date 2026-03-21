//! Chat Service - Agent API for chat operations
//!
//! This service uses the fact-first chat path: operations emit `ChatFact` values
//! and commit them into the journal as `RelationalFact::Generic`.
//!
//! All chat operations go through `ChatFactService` which provides guard chain
//! integration (capability checks, flow budget charging, fact emission).

use crate::core::{AgentError, AgentResult};
use crate::handlers::shared::context_commitment_from_journal;
use crate::runtime::consensus::build_consensus_params;
use crate::runtime::vm_host_bridge::AuraVmRoundDisposition;
use crate::runtime::{
    handle_owned_vm_round, open_owned_manifest_vm_session_admitted, AuraEffectSystem,
};
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
use aura_core::threshold::{policy_for, AgreementMode, CeremonyFlow};
use aura_core::time::{OrderingPolicy, PhysicalTime, TimeOrdering, TimeStamp};
use aura_core::types::identifiers::{AuthorityId, ChannelId, ContextId};
use aura_core::util::serialization::to_vec;
use aura_core::{Hash32, Prestate};
use aura_guards::{types::CapabilityId, GuardContextProvider};
use aura_journal::fact::{ChannelBumpReason, ProposedChannelEpochBump};
use aura_journal::DomainFact;
use aura_protocol::amp::{
    commit_bump_with_consensus, emit_proposed_bump, get_channel_state, AmpChannelCoordinator,
    AmpJournalEffects,
};
use aura_protocol::amp::{AmpMessage, AmpReceipt};
use aura_protocol::effects::TreeEffects;
use aura_protocol::effects::{ChoreographicRole, RoleIndex};
use aura_social::{is_user_banned, is_user_muted};
use uuid::Uuid;

/// Chat service API for the agent layer.
///
/// The service commits chat facts into the agent's canonical fact store
/// (`AuraEffectSystem::commit_generic_fact_bytes`) so reactive views and sync
/// pipelines can observe them.
#[derive(Clone)]
pub struct ChatServiceApi {
    effects: std::sync::Arc<AuraEffectSystem>,
    facts: ChatFactService,
}

impl std::fmt::Debug for ChatServiceApi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChatServiceApi").finish_non_exhaustive()
    }
}

impl ChatServiceApi {
    /// Create a new chat service.
    pub fn new(effects: std::sync::Arc<AuraEffectSystem>) -> AgentResult<Self> {
        Ok(Self {
            effects,
            facts: ChatFactService::new(),
        })
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

    fn amp_role(authority_id: AuthorityId) -> ChoreographicRole {
        ChoreographicRole::for_authority(authority_id, RoleIndex::new(0).expect("role index"))
    }

    async fn run_amp_transport_vm(
        &self,
        session_uuid: Uuid,
        roles: Vec<ChoreographicRole>,
        peer_roles: std::collections::BTreeMap<String, ChoreographicRole>,
        active_role: &str,
        initial_payloads: Vec<Vec<u8>>,
    ) -> AgentResult<()> {
        let result = async {
            let manifest =
                aura_protocol::amp::choreography::telltale_session_types_amp_transport::vm_artifacts::composition_manifest();
            let global_type =
                aura_protocol::amp::choreography::telltale_session_types_amp_transport::vm_artifacts::global_type();
            let local_types =
                aura_protocol::amp::choreography::telltale_session_types_amp_transport::vm_artifacts::local_types();
            let mut session = open_owned_manifest_vm_session_admitted(
                self.effects.clone(),
                session_uuid,
                roles,
                &manifest,
                active_role,
                &global_type,
                &local_types,
                crate::runtime::AuraVmSchedulerSignals::default(),
            )
            .await
            .map_err(|error| AgentError::internal(error.to_string()))?;

            for payload in initial_payloads {
                session.queue_send_bytes(payload);
            }

            let loop_result = loop {
                let round = session
                    .advance_round(active_role, &peer_roles)
                    .await
                    .map_err(|error| AgentError::internal(error.to_string()))?;

                match handle_owned_vm_round(&mut session, round, &format!("AMP {active_role} VM"))
                    .map_err(|error| AgentError::internal(error.to_string()))?
                {
                    AuraVmRoundDisposition::Continue => {}
                    AuraVmRoundDisposition::Complete => break Ok(()),
                }
            };

            let _ = session.close().await;
            loop_result
        }
        .await;
        result
    }

    async fn build_amp_prestate(
        &self,
        authority_id: AuthorityId,
        context_id: ContextId,
    ) -> AgentResult<Prestate> {
        let tree_state = self
            .effects
            .get_current_state()
            .await
            .map_err(|e| AgentError::effects(format!("Tree state lookup failed: {e}")))?;
        let journal = self
            .effects
            .fetch_context_journal(context_id)
            .await
            .map_err(|e| AgentError::effects(format!("Context journal lookup failed: {e}")))?;
        let context_commitment = context_commitment_from_journal(context_id, &journal)?;
        Prestate::new(
            vec![(authority_id, Hash32(tree_state.root_commitment))],
            context_commitment,
        )
        .map_err(|e| AgentError::effects(format!("Invalid AMP prestate: {e}")))
    }

    async fn propose_and_finalize_amp_bump(
        &self,
        authority_id: AuthorityId,
        context_id: ContextId,
        channel_id: ChannelId,
        reason: ChannelBumpReason,
    ) -> AgentResult<()> {
        let state = get_channel_state(self.effects.as_ref(), context_id, channel_id)
            .await
            .map_err(|e| AgentError::effects(format!("AMP state lookup failed: {e}")))?;
        let bump_nonce = self.effects.random_uuid().await.as_bytes().to_vec();
        let bump_id = Hash32(hash(&bump_nonce));
        let proposal = ProposedChannelEpochBump {
            context: context_id,
            channel: channel_id,
            parent_epoch: state.chan_epoch,
            new_epoch: state.chan_epoch + 1,
            bump_id,
            reason,
        };

        emit_proposed_bump(self.effects.as_ref(), proposal.clone())
            .await
            .map_err(|e| AgentError::effects(format!("AMP proposal failed: {e}")))?;

        let policy = policy_for(CeremonyFlow::AmpEpochBump);
        if policy.allows_mode(AgreementMode::ConsensusFinalized) && !self.effects.is_testing() {
            let prestate = self.build_amp_prestate(authority_id, context_id).await?;
            let params = build_consensus_params(
                context_id,
                self.effects.as_ref(),
                authority_id,
                self.effects.as_ref(),
            )
            .await
            .map_err(|e| AgentError::effects(format!("Consensus params failed: {e}")))?;

            let transcript_ref = self
                .effects
                .latest_dkg_transcript_commit(authority_id, context_id)
                .await
                .map_err(|e| AgentError::effects(format!("AMP transcript lookup failed: {e}")))?;
            let transcript_ref =
                transcript_ref.and_then(|commit| commit.blob_ref.or(Some(commit.transcript_hash)));

            commit_bump_with_consensus(
                self.effects.as_ref(),
                &prestate,
                &proposal,
                params.key_packages,
                params.group_public_key,
                transcript_ref,
            )
            .await
            .map_err(|e| AgentError::effects(format!("AMP finalize failed: {e}")))?;
        }

        Ok(())
    }

    async fn build_snapshot(
        &self,
        authority_id: AuthorityId,
        context_id: ContextId,
        channel_id: Option<ChannelId>,
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
            CapabilityId::from(aura_chat::guards::costs::CAP_CHAT_CHANNEL_CREATE),
            CapabilityId::from(aura_chat::guards::costs::CAP_CHAT_MESSAGE_SEND),
        ];
        let committed_facts = self
            .effects
            .load_committed_facts(self.effects.authority_id())
            .await
            .map_err(|e| AgentError::effects(format!("moderation fact load failed: {e}")))?;
        let sender_is_banned = is_user_banned(
            &committed_facts,
            &context_id,
            &authority_id,
            now.ts_ms,
            channel_id.as_ref(),
        );
        let sender_is_muted = is_user_muted(
            &committed_facts,
            &context_id,
            &authority_id,
            now.ts_ms,
            channel_id.as_ref(),
        );

        Ok(GuardSnapshot::new(
            authority_id,
            context_id,
            aura_core::FlowCost::new(u32::MAX),
            capabilities,
            now.ts_ms,
        )
        .with_moderation_status(sender_is_banned, sender_is_muted))
    }

    async fn execute_outcome(&self, outcome: GuardOutcome) -> AgentResult<()> {
        if outcome.is_denied() {
            let reason = outcome
                .decision
                .denial_reason()
                .map(ToString::to_string)
                .unwrap_or_else(|| "Operation denied".to_string());
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
                        cost = cost.value(),
                        "Chat fact commit - flow cost tracked for sync-time charging"
                    );
                }
                EffectCommand::JournalAppend { fact } => {
                    self.effects
                        .commit_generic_fact_bytes(
                            fact.context_id(),
                            CHAT_FACT_TYPE_ID.into(),
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
                    envelope,
                },
            ) = fact.content
            else {
                continue;
            };

            if ctx != context_id || envelope.type_id.as_str() != CHAT_FACT_TYPE_ID {
                continue;
            }

            let Some(chat_fact) = aura_chat::ChatFact::from_envelope(&envelope) else {
                continue;
            };

            // Restrict to the single channel for this group mapping.
            match &chat_fact {
                aura_chat::ChatFact::ChannelCreated { channel_id: c, .. }
                | aura_chat::ChatFact::ChannelClosed { channel_id: c, .. }
                | aura_chat::ChatFact::ChannelUpdated { channel_id: c, .. }
                | aura_chat::ChatFact::MessageSentSealed { channel_id: c, .. }
                | aura_chat::ChatFact::MessageDeliveryUpdated { channel_id: c, .. }
                | aura_chat::ChatFact::MessageRead { channel_id: c, .. }
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

    async fn load_message_facts(
        &self,
        message_id: &ChatMessageId,
    ) -> AgentResult<Option<(ChatGroupId, Vec<aura_chat::ChatFact>)>> {
        let typed = self
            .effects
            .load_committed_facts(self.effects.authority_id())
            .await
            .map_err(AgentError::from)?;

        let target_message_id = message_id.to_string();
        let mut group_id = None;
        let mut out = Vec::new();

        for fact in typed {
            let aura_journal::fact::FactContent::Relational(
                aura_journal::fact::RelationalFact::Generic { envelope, .. },
            ) = fact.content
            else {
                continue;
            };

            if envelope.type_id.as_str() != CHAT_FACT_TYPE_ID {
                continue;
            }

            let Some(chat_fact) = aura_chat::ChatFact::from_envelope(&envelope) else {
                continue;
            };

            let matched_context = match &chat_fact {
                aura_chat::ChatFact::MessageSentSealed {
                    context_id,
                    message_id,
                    ..
                }
                | aura_chat::ChatFact::MessageRead {
                    context_id,
                    message_id,
                    ..
                }
                | aura_chat::ChatFact::MessageEdited {
                    context_id,
                    message_id,
                    ..
                }
                | aura_chat::ChatFact::MessageDeliveryUpdated {
                    context_id,
                    message_id,
                    ..
                }
                | aura_chat::ChatFact::MessageDeleted {
                    context_id,
                    message_id,
                    ..
                } if *message_id == target_message_id => Some(*context_id),
                _ => None,
            };

            let Some(context_id) = matched_context else {
                continue;
            };

            group_id.get_or_insert_with(|| {
                ChatGroupId::from_uuid(Uuid::from_bytes(context_id.to_bytes()))
            });
            out.push(chat_fact);
        }

        Ok(group_id.map(|group_id| (group_id, out)))
    }

    fn physical_timestamp(physical_time: PhysicalTime) -> TimeStamp {
        TimeStamp::PhysicalClock(physical_time)
    }

    fn parse_message_id(message_id: &str) -> ChatMessageId {
        let message_uuid = Uuid::parse_str(message_id).unwrap_or_else(|_| {
            let digest = hash(message_id.as_bytes());
            let mut uuid_bytes = [0u8; 16];
            uuid_bytes.copy_from_slice(&digest[..16]);
            Uuid::from_bytes(uuid_bytes)
        });
        ChatMessageId(message_uuid)
    }

    fn decode_payload(payload: Vec<u8>) -> String {
        let payload_len = payload.len();
        String::from_utf8(payload).unwrap_or_else(|_| format!("[sealed: {payload_len} bytes]"))
    }

    fn ordered_chat_facts(facts: Vec<aura_chat::ChatFact>) -> Vec<(usize, aura_chat::ChatFact)> {
        let mut ordered: Vec<_> = facts.into_iter().enumerate().collect();
        ordered.sort_by(|(left_idx, left_fact), (right_idx, right_fact)| {
            left_fact
                .timestamp_ms()
                .cmp(&right_fact.timestamp_ms())
                .then(left_idx.cmp(right_idx))
        });
        ordered
    }

    fn reduce_group_messages(
        group_id: &ChatGroupId,
        facts: Vec<aura_chat::ChatFact>,
    ) -> Vec<ChatMessage> {
        let mut messages = std::collections::BTreeMap::<String, ChatMessage>::new();

        for (_, fact) in Self::ordered_chat_facts(facts) {
            match fact {
                aura_chat::ChatFact::MessageSentSealed {
                    message_id,
                    sender_id,
                    payload,
                    sent_at,
                    reply_to,
                    ..
                } => {
                    let mut message = ChatMessage::new_text(
                        Self::parse_message_id(&message_id),
                        group_id.clone(),
                        sender_id,
                        Self::decode_payload(payload),
                        Self::physical_timestamp(sent_at),
                    );
                    if let Some(reply_to) = reply_to {
                        if let Ok(reply_uuid) = Uuid::parse_str(&reply_to) {
                            message = message.set_reply_to(ChatMessageId(reply_uuid));
                        }
                    }
                    messages.insert(message_id, message);
                }
                aura_chat::ChatFact::MessageEdited {
                    message_id,
                    editor_id,
                    new_payload,
                    edited_at,
                    ..
                } => {
                    let edited_content = Self::decode_payload(new_payload);
                    let timestamp = Self::physical_timestamp(edited_at);
                    if let Some(existing) = messages.get_mut(&message_id) {
                        existing.content = edited_content;
                        existing.timestamp = timestamp;
                        existing.message_type = aura_chat::types::MessageType::Edit;
                    } else {
                        messages.insert(
                            message_id.clone(),
                            ChatMessage {
                                id: Self::parse_message_id(&message_id),
                                group_id: group_id.clone(),
                                sender_id: editor_id,
                                content: edited_content,
                                message_type: aura_chat::types::MessageType::Edit,
                                timestamp,
                                reply_to: None,
                                metadata: std::collections::HashMap::default(),
                            },
                        );
                    }
                }
                aura_chat::ChatFact::MessageDeleted { message_id, .. } => {
                    messages.remove(&message_id);
                }
                _ => {}
            }
        }

        let mut messages: Vec<_> = messages.into_values().collect();
        messages.sort_by(|left, right| {
            left.timestamp
                .sort_compare(&right.timestamp, OrderingPolicy::DeterministicTieBreak)
        });
        messages
    }

    fn reduce_group_view(
        group_id: &ChatGroupId,
        facts: Vec<aura_chat::ChatFact>,
    ) -> Option<ChatGroup> {
        let mut group = None;
        let mut closed = false;

        for (_, fact) in Self::ordered_chat_facts(facts) {
            match fact {
                aura_chat::ChatFact::ChannelCreated {
                    name,
                    topic,
                    created_at,
                    creator_id,
                    ..
                } => {
                    let created_at = Self::physical_timestamp(created_at);
                    group = Some(ChatGroup {
                        id: group_id.clone(),
                        name,
                        description: topic.unwrap_or_default(),
                        created_at: created_at.clone(),
                        created_by: creator_id,
                        members: vec![ChatMember {
                            authority_id: creator_id,
                            nickname_suggestion: creator_id.to_string(),
                            joined_at: created_at,
                            role: ChatRole::Admin,
                        }],
                        metadata: std::collections::HashMap::default(),
                    });
                }
                aura_chat::ChatFact::ChannelUpdated {
                    name,
                    topic,
                    member_ids,
                    updated_at,
                    ..
                } => {
                    let Some(group) = group.as_mut() else {
                        continue;
                    };
                    if let Some(name) = name {
                        group.name = name;
                    }
                    if let Some(topic) = topic {
                        group.description = topic;
                    }
                    if let Some(member_ids) = member_ids {
                        let joined_at = Self::physical_timestamp(updated_at);
                        let mut members = vec![ChatMember {
                            authority_id: group.created_by,
                            nickname_suggestion: group.created_by.to_string(),
                            joined_at: group.created_at.clone(),
                            role: ChatRole::Admin,
                        }];
                        for member_id in member_ids {
                            if member_id == group.created_by {
                                continue;
                            }
                            members.push(ChatMember {
                                authority_id: member_id,
                                nickname_suggestion: member_id.to_string(),
                                joined_at: joined_at.clone(),
                                role: ChatRole::Member,
                            });
                        }
                        group.members = members;
                    }
                }
                aura_chat::ChatFact::ChannelClosed { .. } => {
                    closed = true;
                }
                _ => {}
            }
        }

        if closed {
            None
        } else {
            group
        }
    }

    fn reduce_message(
        group_id: &ChatGroupId,
        facts: Vec<aura_chat::ChatFact>,
    ) -> Option<ChatMessage> {
        let mut reduced = Self::reduce_group_messages(group_id, facts);
        if reduced.len() == 1 {
            reduced.pop()
        } else {
            None
        }
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
        let policy =
            aura_core::threshold::policy_for(aura_core::threshold::CeremonyFlow::GroupHomeCreation);
        if !policy.allows_mode(aura_core::threshold::AgreementMode::Provisional) {
            return Err(AgentError::invalid(
                "group creation policy does not allow provisional AMP channel",
            ));
        }

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

        let snapshot = self
            .build_snapshot(creator_id, context_id, Some(channel_id))
            .await?;
        let outcome =
            self.facts
                .prepare_create_channel(&snapshot, channel_id, name.to_string(), None, false);
        self.execute_outcome(outcome).await?;

        if policy.allows_mode(AgreementMode::ConsensusFinalized) {
            self.propose_and_finalize_amp_bump(
                creator_id,
                context_id,
                channel_id,
                ChannelBumpReason::Routine,
            )
            .await?;
        }

        let created_at = TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: snapshot.now_ms,
            uncertainty: None,
        });

        let mut members = Vec::new();
        members.push(ChatMember {
            authority_id: creator_id,
            nickname_suggestion: creator_id.to_string(),
            joined_at: created_at.clone(),
            role: ChatRole::Admin,
        });
        for member in initial_members {
            if member == creator_id {
                continue;
            }
            members.push(ChatMember {
                authority_id: member,
                nickname_suggestion: member.to_string(),
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
            metadata: std::collections::HashMap::default(),
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

        let snapshot = self
            .build_snapshot(sender_id, context_id, Some(channel_id))
            .await?;

        let message_uuid = self.effects.random_uuid().await;
        let message_id = message_uuid.to_string();

        // Get AMP channel state for epoch tracking (for consensus finalization)
        let channel_state = get_channel_state(self.effects.as_ref(), context_id, channel_id)
            .await
            .map_err(|e| AgentError::effects(format!("AMP state lookup failed: {e}")))?;
        let epoch_hint = Some(channel_state.chan_epoch as u32);

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
            epoch_hint,
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
        let mut messages = Self::reduce_group_messages(group_id, facts);

        if let Some(before_ts) = &before {
            messages.retain(|message| {
                matches!(
                    message.timestamp.compare(before_ts, OrderingPolicy::Native),
                    TimeOrdering::Before
                )
            });
        }

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
    /// Reconstructs a fact-backed view from the committed channel facts.
    pub async fn get_group(&self, group_id: &ChatGroupId) -> AgentResult<Option<ChatGroup>> {
        let facts = self.load_group_facts(group_id).await?;
        Ok(Self::reduce_group_view(group_id, facts))
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
                    envelope,
                },
            ) = fact.content
            else {
                continue;
            };
            if envelope.type_id.as_str() != CHAT_FACT_TYPE_ID {
                continue;
            }
            let Some(chat_fact) = aura_chat::ChatFact::from_envelope(&envelope) else {
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
                        nickname_suggestion: creator_id.to_string(),
                        joined_at: created_at_ts,
                        role: ChatRole::Admin,
                    }],
                    metadata: std::collections::HashMap::default(),
                }
            })
            .collect();

        groups.sort_by(|a, b| {
            a.created_at
                .sort_compare(&b.created_at, OrderingPolicy::DeterministicTieBreak)
        });
        Ok(groups)
    }

    // =========================================================================
    // Operations requiring ceremony infrastructure (per docs/109_operation_categories.md):
    // - add_member: Category C (ceremony required for group key rotation)
    // - remove_member: Category B/C depending on context
    // - edit_message: Category A (emit EditFact)
    // - delete_message: Category B (deferred approval)
    // =========================================================================

    /// Add a member to a chat group (Category C operation)
    ///
    /// Per docs/109_operation_categories.md, membership changes are Category C:
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
    /// Per docs/109_operation_categories.md, membership changes are Category C.
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

        // Key rotation ceremony: bump the channel epoch so the removed member
        // cannot decrypt messages sent after this point.
        self.propose_and_finalize_amp_bump(
            _requester,
            context_id,
            channel_id,
            ChannelBumpReason::Routine,
        )
        .await?;

        tracing::info!(
            group_id = %group_id,
            removed_member = %member_to_remove,
            new_epoch = "rotated",
            "Member removed from chat group with key rotation"
        );

        Ok(())
    }

    pub async fn get_message(
        &self,
        message_id: &ChatMessageId,
    ) -> AgentResult<Option<ChatMessage>> {
        let Some((group_id, facts)) = self.load_message_facts(message_id).await? else {
            return Ok(None);
        };
        Ok(Self::reduce_message(&group_id, facts))
    }

    /// Edit a message (Category A operation - fact-backed)
    ///
    /// Per docs/109_operation_categories.md, message edits are Category A:
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
            .commit_generic_fact_bytes(context_id, CHAT_FACT_TYPE_ID.into(), edit_fact.to_bytes())
            .await
            .map_err(|e| AgentError::effects(format!("Failed to commit edit fact: {e}")))?;

        self.get_message(message_id).await?.ok_or_else(|| {
            AgentError::effects("Edited message was committed but could not be reduced")
        })
    }

    /// Delete a message (Category B operation - may require deferred approval)
    ///
    /// Per docs/109_operation_categories.md, message deletion is Category B:
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
            .commit_generic_fact_bytes(context_id, CHAT_FACT_TYPE_ID.into(), delete_fact.to_bytes())
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

    /// Update group details (Category A operation - fact-backed)
    ///
    /// Per docs/109_operation_categories.md, topic/name updates are Category A:
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
            None,
            None,
            now.ts_ms,
            requester,
        );

        self.effects
            .commit_generic_fact_bytes(context_id, CHAT_FACT_TYPE_ID.into(), update_fact.to_bytes())
            .await
            .map_err(|e| AgentError::effects(format!("Failed to commit update fact: {e}")))?;

        self.get_group(group_id).await?.ok_or_else(|| {
            AgentError::effects("Updated group was committed but could not be reduced")
        })
    }

    // =========================================================================
    // AmpTransport Choreography (execute_as)
    // =========================================================================

    /// Execute AmpTransport choreography as the Sender role.
    ///
    /// Sends an AMP message to a receiver and receives the acknowledgment receipt.
    ///
    /// # Arguments
    /// * `sender_id` - AuthorityId of the sender
    /// * `receiver_id` - AuthorityId of the receiver
    /// * `context_id` - ContextId for the AMP channel
    /// * `message` - The AMP message to send
    ///
    /// # Returns
    /// Ok(()) on successful protocol completion
    pub async fn execute_amp_transport_as_sender(
        &self,
        sender_id: AuthorityId,
        receiver_id: AuthorityId,
        context_id: ContextId,
        message: AmpMessage,
    ) -> AgentResult<()> {
        let session_uuid = amp_session_uuid(&context_id, &sender_id, &receiver_id);
        self.run_amp_transport_vm(
            session_uuid,
            vec![Self::amp_role(sender_id), Self::amp_role(receiver_id)],
            std::collections::BTreeMap::from([(
                "Receiver".to_string(),
                Self::amp_role(receiver_id),
            )]),
            "Sender",
            vec![to_vec(&message).map_err(|error| {
                AgentError::internal(format!("AMP message encode failed: {error}"))
            })?],
        )
        .await
    }

    /// Execute AmpTransport choreography as the Receiver role.
    ///
    /// Receives an AMP message from a sender and sends back an acknowledgment receipt.
    ///
    /// # Arguments
    /// * `sender_id` - AuthorityId of the sender
    /// * `receiver_id` - AuthorityId of the receiver
    /// * `context_id` - ContextId for the AMP channel
    /// * `channel_id` - ChannelId for the receipt
    /// * `chan_epoch` - Current channel epoch for the receipt
    /// * `ratchet_gen` - Current ratchet generation for the receipt
    ///
    /// # Returns
    /// Ok(()) on successful protocol completion
    pub async fn execute_amp_transport_as_receiver(
        &self,
        sender_id: AuthorityId,
        receiver_id: AuthorityId,
        context_id: ContextId,
        channel_id: ChannelId,
        chan_epoch: u64,
        ratchet_gen: u64,
    ) -> AgentResult<()> {
        let receipt = AmpReceipt {
            context: context_id,
            channel: channel_id,
            chan_epoch,
            ratchet_gen,
        };
        let session_uuid = amp_session_uuid(&context_id, &sender_id, &receiver_id);
        self.run_amp_transport_vm(
            session_uuid,
            vec![Self::amp_role(sender_id), Self::amp_role(receiver_id)],
            std::collections::BTreeMap::from([("Sender".to_string(), Self::amp_role(sender_id))]),
            "Receiver",
            vec![to_vec(&receipt).map_err(|error| {
                AgentError::internal(format!("AMP receipt encode failed: {error}"))
            })?],
        )
        .await
    }
}

fn amp_session_uuid(context_id: &ContextId, sender: &AuthorityId, receiver: &AuthorityId) -> Uuid {
    // Create deterministic session ID from context + sender + receiver
    let key = format!("amp_transport:{}:{}:{}", context_id.0, sender, receiver);
    let digest = hash(key.as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    Uuid::from_bytes(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::AgentConfig;
    use aura_journal::DomainFact;
    use std::sync::Arc;

    fn authority(byte: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([byte; 32])
    }

    fn group_id(byte: u8) -> ChatGroupId {
        ChatGroupId::from_uuid(Uuid::from_bytes([byte; 16]))
    }

    async fn commit_chat_fact(
        effects: &Arc<AuraEffectSystem>,
        context_id: ContextId,
        fact: aura_chat::ChatFact,
    ) {
        effects
            .commit_generic_fact_bytes(context_id, CHAT_FACT_TYPE_ID.into(), fact.to_bytes())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn get_history_reduces_edits_and_deletes_from_committed_facts() {
        let config = AgentConfig::default();
        let effects = Arc::new(AuraEffectSystem::simulation_for_test(&config).unwrap());
        let service = ChatServiceApi::new(effects.clone()).unwrap();
        let group_id = group_id(7);
        let context_id = ChatServiceApi::context_id_for_group(&group_id);
        let channel_id = ChatServiceApi::channel_id_for_group(&group_id);
        let creator = authority(1);
        let message_one = ChatMessageId::from_uuid(Uuid::from_bytes([8; 16]));
        let message_two = ChatMessageId::from_uuid(Uuid::from_bytes([9; 16]));

        commit_chat_fact(
            &effects,
            context_id,
            aura_chat::ChatFact::channel_created_ms(
                context_id,
                channel_id,
                "general".to_string(),
                Some("Initial topic".to_string()),
                false,
                100,
                creator,
            ),
        )
        .await;
        commit_chat_fact(
            &effects,
            context_id,
            aura_chat::ChatFact::message_sent_sealed_ms(
                context_id,
                channel_id,
                message_one.to_string(),
                creator,
                creator.to_string(),
                b"hello".to_vec(),
                200,
                None,
                None,
            ),
        )
        .await;
        commit_chat_fact(
            &effects,
            context_id,
            aura_chat::ChatFact::message_sent_sealed_ms(
                context_id,
                channel_id,
                message_two.to_string(),
                creator,
                creator.to_string(),
                b"goodbye".to_vec(),
                250,
                None,
                None,
            ),
        )
        .await;
        commit_chat_fact(
            &effects,
            context_id,
            aura_chat::ChatFact::message_edited_ms(
                context_id,
                channel_id,
                message_one.to_string(),
                creator,
                b"hello, edited".to_vec(),
                300,
            ),
        )
        .await;
        commit_chat_fact(
            &effects,
            context_id,
            aura_chat::ChatFact::message_deleted_ms(
                context_id,
                channel_id,
                message_two.to_string(),
                creator,
                350,
            ),
        )
        .await;

        let history = service.get_history(&group_id, None, None).await.unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].id, message_one);
        assert_eq!(history[0].content, "hello, edited");
        assert_eq!(history[0].message_type, aura_chat::types::MessageType::Edit);
        assert_eq!(service.get_message(&message_two).await.unwrap(), None);
    }

    #[tokio::test]
    async fn edit_message_returns_refetched_fact_backed_message() {
        let config = AgentConfig::default();
        let effects = Arc::new(AuraEffectSystem::simulation_for_test(&config).unwrap());
        let service = ChatServiceApi::new(effects.clone()).unwrap();
        let group_id = group_id(11);
        let context_id = ChatServiceApi::context_id_for_group(&group_id);
        let channel_id = ChatServiceApi::channel_id_for_group(&group_id);
        let creator = authority(2);
        let message_id = ChatMessageId::from_uuid(Uuid::from_bytes([12; 16]));

        commit_chat_fact(
            &effects,
            context_id,
            aura_chat::ChatFact::channel_created_ms(
                context_id,
                channel_id,
                "general".to_string(),
                None,
                false,
                100,
                creator,
            ),
        )
        .await;
        commit_chat_fact(
            &effects,
            context_id,
            aura_chat::ChatFact::message_sent_sealed_ms(
                context_id,
                channel_id,
                message_id.to_string(),
                creator,
                creator.to_string(),
                b"before".to_vec(),
                200,
                None,
                None,
            ),
        )
        .await;

        let edited = service
            .edit_message(&group_id, creator, &message_id, "after")
            .await
            .unwrap();

        assert_eq!(edited.id, message_id);
        assert_eq!(edited.content, "after");
        assert_eq!(edited.message_type, aura_chat::types::MessageType::Edit);
        assert_eq!(
            service
                .get_message(&message_id)
                .await
                .unwrap()
                .unwrap()
                .content,
            "after"
        );
    }

    #[tokio::test]
    async fn update_group_details_returns_refetched_fact_backed_group() {
        let config = AgentConfig::default();
        let effects = Arc::new(AuraEffectSystem::simulation_for_test(&config).unwrap());
        let service = ChatServiceApi::new(effects.clone()).unwrap();
        let group_id = group_id(15);
        let context_id = ChatServiceApi::context_id_for_group(&group_id);
        let channel_id = ChatServiceApi::channel_id_for_group(&group_id);
        let creator = authority(3);
        let participant = authority(4);

        commit_chat_fact(
            &effects,
            context_id,
            aura_chat::ChatFact::channel_created_ms(
                context_id,
                channel_id,
                "general".to_string(),
                Some("Initial topic".to_string()),
                false,
                100,
                creator,
            ),
        )
        .await;
        commit_chat_fact(
            &effects,
            context_id,
            aura_chat::ChatFact::channel_updated_ms(
                context_id,
                channel_id,
                None,
                None,
                Some(2),
                Some(vec![participant]),
                150,
                creator,
            ),
        )
        .await;

        let updated = service
            .update_group_details(
                &group_id,
                creator,
                Some("renamed".to_string()),
                Some("Updated topic".to_string()),
                None,
            )
            .await
            .unwrap();

        assert_eq!(updated.name, "renamed");
        assert_eq!(updated.description, "Updated topic");
        assert_eq!(updated.created_by, creator);
        assert_eq!(updated.members.len(), 2);
        assert!(updated
            .members
            .iter()
            .any(|member| member.authority_id == participant));
        assert_eq!(
            service.get_group(&group_id).await.unwrap().unwrap(),
            updated
        );
    }
}
