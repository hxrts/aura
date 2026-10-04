use crate::model::UiController;
use aura_app::ui::signals::CHAT_SIGNAL;
use aura_app::ui::types::ChatState;
use aura_app::ui_contract::observed_chat_projection;
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
            _ => left
                .name
                .cmp(&right.name)
                .then_with(|| left.id.cmp(&right.id)),
        }
    });

    let active = selected_channel_id
        .and_then(|channel_id| {
            channels
                .iter()
                .find(|channel| channel.id.eq_ignore_ascii_case(channel_id))
        })
        .or_else(|| channels.first());
    let active_channel = active
        .map(|channel| channel.name.clone())
        .unwrap_or_default();

    let messages = chat
        .all_channels()
        .find(|channel| active.is_some_and(|selected| channel.id.to_string() == selected.id))
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
    let chat = {
        let core = controller.app_core().read().await;
        super::observed_snapshot_or_report(
            core.read(&*CHAT_SIGNAL).await,
            &controller,
            CHAT_SIGNAL.id(),
        )
    };
    let Some(chat) = chat else {
        return ChatRuntimeView::default();
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
    let runtime_facts = vec![observed_chat_projection(
        &chat,
        selected_channel_id.as_deref(),
    )];
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

#[cfg(test)]
mod projection_tests {
    use super::*;
    use aura_app::ui_contract::RuntimeFact;
    use aura_app::views::chat::{Channel, Message, MessageDeliveryStatus};
    use aura_core::{AuthorityId, ChannelId};

    #[test]
    fn duplicate_names_preserve_selected_identity_and_deterministic_default() {
        let first = ChannelId::from_bytes([1; 32]);
        let second = ChannelId::from_bytes([2; 32]);
        let mut chat = ChatState::from_channels([second, first].map(|id| Channel {
            id,
            name: "Duplicate".into(),
            ..Channel::default()
        }));
        chat.apply_message(
            second,
            Message {
                id: "second-only".into(),
                channel_id: second,
                sender_id: AuthorityId::new_from_entropy([3; 32]),
                sender_name: "Sender".into(),
                content: "Selected channel message".into(),
                timestamp: 0,
                reply_to: None,
                is_own: false,
                is_read: false,
                delivery_status: MessageDeliveryStatus::Sent,
                epoch_hint: None,
                is_finalized: false,
            },
        );
        let default = build_chat_runtime_view(chat.clone(), None);
        assert_eq!(default.channels[0].id, first.to_string());
        assert!(default.messages.is_empty());
        let selected = second.to_string();
        let view = build_chat_runtime_view(chat.clone(), Some(&selected));
        assert_eq!(view.messages.len(), 1);
        assert_eq!(view.messages[0].channel_id, selected);
        for (selection, expected) in [(None, 0), (Some(selected.as_str()), 1)] {
            assert_eq!(
                observed_chat_projection(&chat, selection),
                RuntimeFact::ChatSignalUpdated {
                    active_channel: "Duplicate".into(),
                    channel_count: 2,
                    message_count: expected,
                }
            );
        }
    }
}
