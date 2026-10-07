use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use iocraft::prelude::*;
use parking_lot::RwLock;

use aura_app::ui::signals::{SettingsState, CHAT_SIGNAL, NEIGHBORHOOD_SIGNAL};
use aura_app::ui::types::ChatState;
use aura_app::ui_contract::observed_chat_projection;
use aura_core::AuthorityId;

use super::contracts::subscribe_observed_projection_signal;
use super::{bump_projection_version, SharedAuthorityId, ShellSignalFanout};
use crate::tui::channel_selection::{CommittedChannelSelection, SharedCommittedChannelSelection};
use crate::tui::chat_scope::{active_home_scope_id, effective_home_scope_id, scoped_channels};
use crate::tui::hooks::AppCoreContext;
use crate::tui::tasks::UiTaskOwner;
use crate::tui::types::{Channel, Message};
use crate::tui::updates::{spawn_ui_update, UiUpdate, UiUpdatePublication, UiUpdateSender};

/// Shared messages of every channel (each tagged with its channel id) that can be read by closures without re-rendering.
///
/// This uses Arc<RwLock<Vec<Message>>> instead of State<T> because:
/// 1. Dispatch handler closures need to look up messages by ID (e.g., for retry).
/// 2. We do not want every message update to trigger shell re-renders.
/// 3. The closure captures the Arc, not the data, so it always reads fresh data.
pub type SharedMessages = Arc<RwLock<Vec<Message>>>;

/// Shared channels state that can be read by closures without re-rendering.
///
/// Used to map selected channel index -> channel ID for send operations.
pub type SharedChannels = Arc<RwLock<Vec<Channel>>>;

#[derive(Clone, Debug)]
struct ScopedChannelProjection {
    channels: Vec<Channel>,
    message_count: usize,
    channel_signature: String,
}

fn compute_scoped_channel_projection(
    chat_state: &ChatState,
    active_scope: Option<&str>,
    selected_channel_id: Option<&str>,
) -> ScopedChannelProjection {
    let effective_scope = effective_home_scope_id(chat_state, active_scope, selected_channel_id);
    let scoped = scoped_channels(chat_state, effective_scope.as_deref());
    let message_count = scoped
        .iter()
        .map(|channel| chat_state.messages_for_channel(&channel.id).len())
        .sum();
    let channel_list = scoped
        .iter()
        .copied()
        .map(Channel::from)
        .collect::<Vec<_>>();
    let channel_signature = channel_list
        .iter()
        .map(|channel| channel.id.as_str())
        .collect::<Vec<_>>()
        .join("|");

    ScopedChannelProjection {
        channels: channel_list,
        message_count,
        channel_signature,
    }
}

#[cfg(test)]
fn scoped_channel_snapshot(
    chat_state: &ChatState,
    active_scope: Option<&str>,
) -> (Vec<Channel>, usize) {
    let projection = compute_scoped_channel_projection(chat_state, active_scope, None);
    (projection.channels, projection.message_count)
}

#[derive(Clone)]
struct ChannelProjectionCoordinator {
    channels: SharedChannels,
    selected_channel_id: SharedCommittedChannelSelection,
    active_scope: Arc<RwLock<Option<String>>>,
    latest_chat_state: Arc<RwLock<ChatState>>,
    shared_authority_id: SharedAuthorityId,
    tasks: Arc<UiTaskOwner>,
    update_tx: Option<UiUpdateSender>,
    last_channel_count: Arc<AtomicUsize>,
    last_message_count: Arc<AtomicUsize>,
    last_channel_signature: Arc<RwLock<Option<String>>>,
    projection_version: State<usize>,
}

impl ChannelProjectionCoordinator {
    fn publish_current_projection(&self) {
        let chat_state = self.latest_chat_state.read().clone();
        let scope = self.active_scope.read().clone();
        let selected_channel = self.selected_channel_id.read().clone();
        let projection = compute_scoped_channel_projection(
            &chat_state,
            scope.as_deref(),
            selected_channel
                .as_ref()
                .map(CommittedChannelSelection::channel_id),
        );
        let channel_count = projection.channels.len();
        let message_count = projection.message_count;

        *self.channels.write() = projection.channels;

        if let Some(tx) = self.update_tx.as_ref() {
            spawn_ui_update(
                &self.tasks,
                tx,
                UiUpdate::RuntimeFactObserved(observed_chat_projection(
                    &chat_state,
                    selected_channel
                        .as_ref()
                        .map(CommittedChannelSelection::channel_id),
                )),
                UiUpdatePublication::RequiredUnordered,
            );
            let channel_signature_changed = {
                let mut guard = self.last_channel_signature.write();
                let changed = guard.as_deref() != Some(projection.channel_signature.as_str());
                *guard = Some(projection.channel_signature);
                changed
            };
            let channel_changed = self
                .last_channel_count
                .swap(channel_count, Ordering::Relaxed)
                != channel_count;
            let message_changed = self
                .last_message_count
                .swap(message_count, Ordering::Relaxed)
                != message_count;

            if channel_changed || message_changed || channel_signature_changed {
                spawn_ui_update(
                    &self.tasks,
                    tx,
                    UiUpdate::ChatStateUpdated {
                        channel_count,
                        message_count,
                        selected_index: None,
                    },
                    UiUpdatePublication::RequiredUnordered,
                );
            }
        }

        let mut projection_version = self.projection_version.clone();
        bump_projection_version(&mut projection_version);
    }

    fn update_chat_state(&self, chat_state: ChatState) {
        tracing::debug!(
            "CHAT_SIGNAL_UPDATE: incoming={}",
            chat_state.channel_count(),
        );
        let channel_summary = chat_state
            .all_channels()
            .map(|channel| {
                format!(
                    "{}|is_dm={}|name={}|topic={}",
                    channel.id,
                    channel.is_dm,
                    channel.name,
                    channel.topic.clone().unwrap_or_default()
                )
            })
            .collect::<Vec<_>>()
            .join(" ; ");
        tracing::debug!("CHAT_SIGNAL_CHANNELS: {channel_summary}");

        *self.latest_chat_state.write() = chat_state;
        self.publish_current_projection();
    }

    fn update_authority_id(&self, authority_id: Option<AuthorityId>) {
        *self.shared_authority_id.write() = authority_id;
        self.publish_current_projection();
    }

    fn update_active_scope(&self, scope: Option<String>) {
        *self.active_scope.write() = scope;
        self.publish_current_projection();
    }
}

/// Create shared channel and message holders from one CHAT_SIGNAL subscription.
pub fn use_channels_subscription(
    hooks: &mut Hooks,
    app_ctx: &AppCoreContext,
    shared_authority_id: SharedAuthorityId,
    selected_channel_id: SharedCommittedChannelSelection,
    update_tx: Option<UiUpdateSender>,
    projection_version: State<usize>,
    settings_fanout: &ShellSignalFanout<SettingsState>,
) -> (SharedChannels, SharedMessages) {
    let shared_channels_ref = hooks.use_ref(|| Arc::new(RwLock::new(Vec::new())));
    let shared_channels: SharedChannels = shared_channels_ref.read().clone();
    let shared_messages_ref = hooks.use_ref(|| Arc::new(RwLock::new(Vec::new())));
    let shared_messages: SharedMessages = shared_messages_ref.read().clone();
    let tasks = app_ctx.tasks();
    let active_scope_ref = hooks.use_ref(|| Arc::new(RwLock::new(None::<String>)));
    let active_scope: Arc<RwLock<Option<String>>> = active_scope_ref.read().clone();
    let latest_chat_state_ref = hooks.use_ref(|| Arc::new(RwLock::new(ChatState::default())));
    let latest_chat_state: Arc<RwLock<ChatState>> = latest_chat_state_ref.read().clone();
    let last_channel_count_ref = hooks.use_ref(|| Arc::new(AtomicUsize::new(usize::MAX)));
    let last_channel_count = last_channel_count_ref.read().clone();
    let last_message_count_ref = hooks.use_ref(|| Arc::new(AtomicUsize::new(usize::MAX)));
    let last_message_count = last_message_count_ref.read().clone();
    let last_channel_signature_ref = hooks.use_ref(|| Arc::new(RwLock::new(None::<String>)));
    let last_channel_signature = last_channel_signature_ref.read().clone();
    let coordinator = ChannelProjectionCoordinator {
        channels: shared_channels.clone(),
        selected_channel_id,
        active_scope,
        latest_chat_state,
        shared_authority_id,
        tasks,
        update_tx,
        last_channel_count,
        last_message_count,
        last_channel_signature,
        projection_version: projection_version.clone(),
    };

    hooks.use_future({
        let app_core = app_ctx.clone();
        let coordinator = coordinator.clone();
        let messages = shared_messages.clone();
        let mut message_projection_version = projection_version.clone();
        async move {
            subscribe_observed_projection_signal(app_core, &*CHAT_SIGNAL, move |chat_state| {
                // Every channel's messages; readers select by the current
                // channel selection (`selected_channel_messages`).
                let message_list: Vec<Message> = chat_state
                    .all_channels()
                    .flat_map(|channel| chat_state.messages_for_channel(&channel.id))
                    .map(Message::from)
                    .collect();
                *messages.write() = message_list;
                bump_projection_version(&mut message_projection_version);
                coordinator.update_chat_state(chat_state);
            })
            .await;
        }
    });

    let settings_coordinator = coordinator.clone();
    settings_fanout.register("channel_authority", move |settings_state| {
        settings_coordinator
            .update_authority_id(settings_state.authority_id.parse::<AuthorityId>().ok());
    });

    hooks.use_future({
        let app_core = app_ctx.clone();
        let coordinator = coordinator;
        async move {
            subscribe_observed_projection_signal(
                app_core,
                &*NEIGHBORHOOD_SIGNAL,
                move |neighborhood| {
                    coordinator.update_active_scope(Some(active_home_scope_id(&neighborhood)));
                },
            )
            .await;
        }
    });

    (shared_channels, shared_messages)
}

#[cfg(test)]
mod tests {
    use super::{compute_scoped_channel_projection, scoped_channel_snapshot};
    use aura_app::ui::types::{
        Channel as AppChannel, ChannelType, ChatState, Message, MessageDeliveryStatus,
    };
    use aura_core::crypto::hash::hash;
    use aura_core::types::identifiers::{AuthorityId, ChannelId};
    use std::path::Path;
    fn test_channel_id(seed: &str) -> ChannelId {
        ChannelId::from_bytes(hash(seed.as_bytes()))
    }

    fn test_channel(id: ChannelId, name: &str) -> AppChannel {
        AppChannel {
            id,
            context_id: None,
            name: name.to_string(),
            topic: None,
            channel_type: ChannelType::Home,
            unread_count: 0,
            is_dm: false,
            member_ids: Vec::new(),
            member_count: 1,
            last_message: None,
            last_message_time: None,
            last_activity: 0,
            last_finalized_epoch: 0,
        }
    }

    fn test_dm_channel(id: ChannelId, name: &str) -> AppChannel {
        AppChannel {
            id,
            context_id: None,
            name: name.to_string(),
            topic: None,
            channel_type: ChannelType::DirectMessage,
            unread_count: 0,
            is_dm: true,
            member_ids: Vec::new(),
            member_count: 2,
            last_message: None,
            last_message_time: None,
            last_activity: 0,
            last_finalized_epoch: 0,
        }
    }

    fn test_dm_like_channel(id: ChannelId, name: &str) -> AppChannel {
        AppChannel {
            id,
            context_id: None,
            name: name.to_string(),
            topic: Some("Direct messages with peer".to_string()),
            channel_type: ChannelType::Home,
            unread_count: 0,
            is_dm: false,
            member_ids: Vec::new(),
            member_count: 2,
            last_message: None,
            last_message_time: None,
            last_activity: 0,
            last_finalized_epoch: 0,
        }
    }

    fn test_message(channel_id: ChannelId, id: &str, timestamp: u64) -> Message {
        Message {
            id: id.to_string(),
            channel_id,
            sender_id: AuthorityId::new_from_entropy([3u8; 32]),
            sender_name: "tester".to_string(),
            content: "hello".to_string(),
            timestamp,
            reply_to: None,
            is_own: false,
            is_read: false,
            delivery_status: MessageDeliveryStatus::Sent,
            epoch_hint: None,
            is_finalized: false,
        }
    }

    #[test]
    fn message_subscription_requires_explicit_selected_channel_identity() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let source_path = repo_root
            .join("crates/aura-terminal/src/tui/screens/app/subscriptions/chat_projection.rs");
        let source = std::fs::read_to_string(&source_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", source_path.display()));

        assert!(source.contains("selected_channel_id: SharedCommittedChannelSelection"));
        assert!(source.contains("let channel_id = selected_channel_id"));
        assert!(source.contains(".map(|selection| selection.channel_id().to_string())"));
    }

    #[test]
    fn scoped_snapshot_returns_all_channels_without_scope() {
        let home_a = test_channel_id("home-a");
        let home_b = test_channel_id("home-b");
        let mut state = ChatState::from_channels([
            test_channel(home_a, "Home A"),
            test_channel(home_b, "Home B"),
        ]);

        state.apply_message(home_a, test_message(home_a, "m1", 1));
        state.apply_message(home_b, test_message(home_b, "m2", 2));
        state.apply_message(home_b, test_message(home_b, "m3", 3));

        let (channels, message_count) = scoped_channel_snapshot(&state, None);
        assert_eq!(channels.len(), 2);
        assert_eq!(message_count, 3);
    }

    #[test]
    fn observed_chat_uses_selected_canonical_channel_without_inferred_readiness() {
        use aura_app::views::chat::NOTE_TO_SELF_CHANNEL_NAME;
        let home = test_channel_id("home-observation");
        let notes = test_channel_id("notes-observation");
        let mut chat = ChatState::from_channels([
            test_channel(home, "Home"),
            test_channel(notes, NOTE_TO_SELF_CHANNEL_NAME),
        ]);
        chat.apply_message(home, test_message(home, "observed-message", 1));
        assert_eq!(
            aura_app::ui_contract::observed_chat_projection(&chat, Some(&home.to_string())),
            aura_app::ui_contract::RuntimeFact::ChatSignalUpdated {
                active_channel: "Home".to_string(),
                channel_count: 2,
                message_count: 1,
            }
        );
        assert_eq!(
            aura_app::ui_contract::observed_chat_projection(&chat, Some("absent")),
            aura_app::ui_contract::RuntimeFact::ChatSignalUpdated {
                active_channel: NOTE_TO_SELF_CHANNEL_NAME.to_string(),
                channel_count: 2,
                message_count: 0,
            }
        );
    }

    #[test]
    fn scoped_channel_projection_excludes_removed_selected_dm() {
        let alpha = test_channel_id("alpha");
        let zulu = test_channel_id("zulu");
        let dm_like = test_channel_id("dm-like-contact");
        let state =
            ChatState::from_channels([test_channel(zulu, "Zulu"), test_channel(alpha, "Alpha")]);
        let selected = dm_like.to_string();

        let projection = compute_scoped_channel_projection(&state, None, Some(selected.as_str()));

        assert_eq!(
            projection
                .channels
                .iter()
                .map(|channel| channel.name.as_str())
                .collect::<Vec<_>>(),
            vec!["Alpha", "Zulu"]
        );
    }

    #[test]
    fn scoped_projection_cannot_fabricate_missing_shared_channel_metadata() {
        let home = test_channel_id("home");
        let missing_shared = test_channel_id("missing-shared");
        let state = ChatState::from_channels([test_channel(home, "Home")]);
        let selected = missing_shared.to_string();

        let projection = compute_scoped_channel_projection(&state, None, Some(selected.as_str()));

        assert_eq!(projection.channels.len(), 1);
        assert!(projection
            .channels
            .iter()
            .all(|channel| channel.id != selected));
    }

    #[test]
    fn scoped_snapshot_filters_to_active_home_channel() {
        let home_a = test_channel_id("home-a");
        let home_b = test_channel_id("home-b");
        let mut state = ChatState::from_channels([
            test_channel(home_a, "Home A"),
            test_channel(home_b, "Home B"),
        ]);

        state.apply_message(home_a, test_message(home_a, "m1", 1));
        state.apply_message(home_b, test_message(home_b, "m2", 2));
        state.apply_message(home_b, test_message(home_b, "m3", 3));

        let scope = home_b.to_string();
        let (channels, message_count) = scoped_channel_snapshot(&state, Some(scope.as_str()));
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].id, home_b.to_string());
        assert_eq!(message_count, 2);
    }

    #[test]
    fn scoped_snapshot_keeps_dm_channels_visible_across_scopes() {
        let home_a = test_channel_id("home-a");
        let home_b = test_channel_id("home-b");
        let dm = test_channel_id("dm-contact");
        let mut state = ChatState::from_channels([
            test_channel(home_a, "Home A"),
            test_channel(home_b, "Home B"),
            test_dm_channel(dm, "DM"),
        ]);

        state.apply_message(home_a, test_message(home_a, "m1", 1));
        state.apply_message(home_b, test_message(home_b, "m2", 2));
        state.apply_message(dm, test_message(dm, "m3", 3));

        let scope = home_b.to_string();
        let (channels, message_count) = scoped_channel_snapshot(&state, Some(scope.as_str()));
        assert_eq!(channels.len(), 2);
        assert!(channels.iter().any(|c| c.id == home_b.to_string()));
        assert!(channels.iter().any(|c| c.id == dm.to_string()));
        assert_eq!(message_count, 2);
    }

    #[test]
    fn scoped_snapshot_keeps_dm_like_channels_visible_across_scopes() {
        let home_a = test_channel_id("home-a");
        let home_b = test_channel_id("home-b");
        let dm_like = test_channel_id("dm-like-contact");
        let mut state = ChatState::from_channels([
            test_channel(home_a, "Home A"),
            test_channel(home_b, "Home B"),
            test_dm_like_channel(dm_like, "DM: Contact"),
        ]);

        state.apply_message(home_b, test_message(home_b, "m1", 1));
        state.apply_message(dm_like, test_message(dm_like, "m2", 2));

        let scope = home_b.to_string();
        let (channels, message_count) = scoped_channel_snapshot(&state, Some(scope.as_str()));
        assert_eq!(channels.len(), 2);
        assert!(channels.iter().any(|c| c.id == home_b.to_string()));
        assert!(channels.iter().any(|c| c.id == dm_like.to_string()));
        assert_eq!(message_count, 2);
    }

    #[test]
    fn removed_dm_like_channel_disappears_from_authoritative_projection() {
        let dm_like = test_channel_id("dm-like-contact");
        let incoming = ChatState::default();
        let selected = dm_like.to_string();
        let projection = compute_scoped_channel_projection(&incoming, None, Some(&selected));
        assert!(projection.channels.is_empty());
    }

    #[test]
    fn channel_projection_subscription_uses_single_projection_coordinator() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let source_path = repo_root
            .join("crates/aura-terminal/src/tui/screens/app/subscriptions/chat_projection.rs");
        let source = std::fs::read_to_string(&source_path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", source_path.display()));
        let start = source
            .find("pub fn use_channels_subscription(")
            .unwrap_or_else(|| panic!("missing use_channels_subscription"));
        let end = source[start..]
            .find("#[cfg(test)]")
            .map(|offset| start + offset)
            .unwrap_or_else(|| panic!("missing use_channels_subscription terminator"));
        let section = &source[start..end];

        assert!(section.contains("let coordinator = ChannelProjectionCoordinator"));
        assert!(section.contains("&*CHAT_SIGNAL"));
        assert!(section.contains("settings_fanout.register(\"channel_authority\""));
        assert!(!section.contains("&*SETTINGS_SIGNAL"));
        assert!(section.contains("&*NEIGHBORHOOD_SIGNAL"));
        assert!(!section.contains("&*CONTACTS_SIGNAL"));
        assert!(!section.contains("&*HOMES_SIGNAL"));
        assert!(!section.contains("&*TRANSPORT_PEERS_SIGNAL"));
        assert!(!section.contains("&*DISCOVERED_PEERS_SIGNAL"));
    }
}
