use crate::model::UiController;
use aura_app::ui::signals::CHAT_SIGNAL;
use aura_app::ui::types::ChatState;
use aura_app::ui_contract::{ChannelFactKey, RuntimeFact};
use aura_app::views::chat::is_note_to_self_channel_name;
use aura_core::effects::reactive::ReactiveEffects;
use std::sync::Arc;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::app) struct ChatRuntimeChannel {
    pub(in crate::app) id: String,
    pub(in crate::app) name: String,
    pub(in crate::app) topic: String,
    pub(in crate::app) unread_count: u32,
    pub(in crate::app) last_message: Option<String>,
    pub(in crate::app) member_count: u32,
    pub(in crate::app) is_dm: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::app) struct ChatRuntimeMessage {
    pub(in crate::app) id: String,
    pub(in crate::app) channel_id: String,
    pub(in crate::app) sender_name: String,
    pub(in crate::app) content: String,
    pub(in crate::app) is_own: bool,
    pub(in crate::app) delivery_status: String,
    pub(in crate::app) can_retry: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::app) struct ChatRuntimeView {
    pub(in crate::app) loaded: bool,
    pub(in crate::app) active_channel: String,
    pub(in crate::app) channels: Vec<ChatRuntimeChannel>,
    pub(in crate::app) messages: Vec<ChatRuntimeMessage>,
}

fn build_chat_runtime_view(chat: ChatState, selected_channel_id: Option<&str>) -> ChatRuntimeView {
    let mut channels: Vec<_> = chat
        .all_channels()
        .map(|channel| ChatRuntimeChannel {
            id: channel.id.to_string(),
            name: channel.name.clone(),
            topic: channel.topic.clone().unwrap_or_default(),
            unread_count: channel.unread_count,
            last_message: channel.last_message.clone(),
            member_count: channel.member_count,
            is_dm: channel.is_dm,
        })
        .collect();
    channels.sort_by(|left, right| {
        match (
            is_note_to_self_channel_name(&left.name),
            is_note_to_self_channel_name(&right.name),
        ) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => left.name.cmp(&right.name),
        }
    });

    let active_channel = selected_channel_id
        .and_then(|channel_id| {
            channels
                .iter()
                .find(|channel| channel.id.eq_ignore_ascii_case(channel_id))
                .map(|channel| channel.name.clone())
        })
        .or_else(|| channels.first().map(|channel| channel.name.clone()))
        .unwrap_or_default();

    let messages = chat
        .all_channels()
        .find(|channel| channel.name.eq_ignore_ascii_case(&active_channel))
        .map(|channel| {
            chat.messages_for_channel(&channel.id)
                .iter()
                .map(|message| ChatRuntimeMessage {
                    id: message.id.clone(),
                    channel_id: channel.id.to_string(),
                    sender_name: message.sender_name.clone(),
                    content: message.content.clone(),
                    is_own: message.is_own,
                    delivery_status: message.delivery_status.description().to_string(),
                    can_retry: message.delivery_status.can_retry(),
                })
                .collect()
        })
        .unwrap_or_default();

    ChatRuntimeView {
        loaded: true,
        active_channel,
        channels,
        messages,
    }
}

pub(in crate::app) async fn load_chat_runtime_view(
    controller: Arc<UiController>,
) -> ChatRuntimeView {
    fn saturating_u32(value: usize) -> u32 {
        u32::try_from(value).unwrap_or(u32::MAX)
    }

    let (chat, authority_id) = {
        let core = controller.app_core().read().await;
        let merged = core.read(&*CHAT_SIGNAL).await.unwrap_or_default();
        let authority_id = core.authority().cloned();
        (merged, authority_id)
    };
    let selected_channel_id = controller
        .ui_model()
        .and_then(|model| model.selected_channel_id().map(str::to_string));
    let runtime = build_chat_runtime_view(chat.clone(), selected_channel_id.as_deref());
    controller.push_log(&format!(
        "load_chat_runtime_view: selected={:?} active={} channels={}",
        selected_channel_id,
        runtime.active_channel,
        runtime.channels.len()
    ));
    let mut runtime_facts = vec![RuntimeFact::ChatSignalUpdated {
        active_channel: runtime.active_channel.clone(),
        channel_count: saturating_u32(runtime.channels.len()),
        message_count: saturating_u32(runtime.messages.len()),
    }];
    if let (Some(channel), Some(authority_id)) = (
        chat.all_channels()
            .find(|channel| channel.name.eq_ignore_ascii_case(&runtime.active_channel)),
        authority_id,
    ) {
        let resolved_recipient_count = channel
            .member_ids
            .iter()
            .filter(|member_id| **member_id != authority_id)
            .count();
        let resolved_member_count = channel
            .member_count
            .max((resolved_recipient_count.saturating_add(1)) as u32);
        runtime_facts.push(RuntimeFact::ChannelMembershipReady {
            channel: ChannelFactKey::named(channel.name.clone()),
            member_count: Some(resolved_member_count),
        });
        if resolved_recipient_count > 0 {
            let channel_key = ChannelFactKey::named(channel.name.clone());
            runtime_facts.push(RuntimeFact::RecipientPeersResolved {
                channel: channel_key.clone(),
                member_count: resolved_member_count,
            });
            runtime_facts.push(RuntimeFact::MessageDeliveryReady {
                channel: channel_key,
                member_count: resolved_member_count,
            });
        }
    }
    controller.publish_runtime_channels_projection(
        runtime
            .channels
            .iter()
            .map(|channel| {
                (
                    channel.id.clone(),
                    channel.name.clone(),
                    channel.topic.clone(),
                )
            })
            .collect(),
        runtime_facts,
    );
    runtime
}
