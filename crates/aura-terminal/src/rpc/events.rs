//! RPC event streaming.
//!
//! `{"id":1,"method":"subscribe","params":{"topics":["messages","sync"]}}`
//! answers with a subscription id and a snapshot of those topics, then
//! interleaves event lines with responses:
//!
//! ```json
//! {"type":"event","subscription":1,"topic":"messages","kind":"message","data":{..}}
//! ```
//!
//! Events are derived from the same reactive signals the TUI observes
//! (chat, invitations, contacts, sync and connection status, authoritative
//! semantic facts). Message drops and supervised task failures are runtime
//! diagnostics without a signal; they are compared whenever a signal fires.
//!
//! Each subscription has a bounded queue. A subscriber that falls more than
//! [`QUEUE_CAPACITY`] events behind loses its queued events and receives one
//! `resync` event carrying a fresh snapshot instead; signal streams that lag
//! resume from a newer snapshot, so diffs against the last seen state stay
//! complete.

use super::CommandError;
use crate::command::execute::{channel_view, message_view};
use async_lock::RwLock;
use aura_app::ui::contract::bridged_operation_statuses;
use aura_app::ui::signals::{
    ConnectionStatus, SyncStatus, AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL, CHAT_SIGNAL,
    CONNECTION_STATUS_SIGNAL, CONTACTS_SIGNAL, INVITATIONS_SIGNAL, SYNC_STATUS_SIGNAL,
};
use aura_app::ui::types::{AppCore, ChatState, ContactsState, InvitationsState};
use aura_app::ui::workflows::runtime::require_runtime;
use aura_core::effects::reactive::{ReactiveEffects, SignalStream};
use aura_core::types::identifiers::{AuthorityId, ChannelId};
use futures::stream::{self, BoxStream, SelectAll, StreamExt};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};
use std::sync::Arc;

/// Events a subscription may hold before it is resynchronized.
pub const QUEUE_CAPACITY: usize = 256;

/// What a subscription receives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Topic {
    /// Semantic operation lifecycle and terminal outcomes.
    Operations,
    /// New chat messages.
    Messages,
    /// Invitation arrivals.
    Invitations,
    /// Contact and channel membership changes.
    Membership,
    /// Journal sync and connection status.
    Sync,
    /// Dropped inbound or undeliverable outbound messages.
    Drops,
    /// Supervised runtime task failures.
    TaskFailures,
}

impl Topic {
    /// Every topic, in wire order.
    pub const ALL: [Topic; 7] = [
        Topic::Operations,
        Topic::Messages,
        Topic::Invitations,
        Topic::Membership,
        Topic::Sync,
        Topic::Drops,
        Topic::TaskFailures,
    ];

    /// Wire name.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Topic::Operations => "operations",
            Topic::Messages => "messages",
            Topic::Invitations => "invitations",
            Topic::Membership => "membership",
            Topic::Sync => "sync",
            Topic::Drops => "drops",
            Topic::TaskFailures => "task_failures",
        }
    }

    fn parse(raw: &str) -> Option<Topic> {
        Topic::ALL.into_iter().find(|t| t.name() == raw)
    }
}

/// Parse `params.topics`; absent means every topic.
pub fn parse_topics(params: &Value) -> Result<Vec<Topic>, CommandError> {
    match params.get("topics") {
        None | Some(Value::Null) => Ok(Topic::ALL.to_vec()),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| {
                item.as_str().and_then(Topic::parse).ok_or_else(|| {
                    CommandError::invalid(format!(
                        "unknown topic {item}; expected one of {}",
                        Topic::ALL.map(Topic::name).join(", ")
                    ))
                })
            })
            .collect(),
        Some(other) => Err(CommandError::invalid(format!(
            "topics must be an array, got {other}"
        ))),
    }
}

/// The response to `subscribe`.
#[must_use]
pub fn subscribe_response(id: &Value, outcome: Result<(u64, Value), CommandError>) -> Value {
    match outcome {
        Ok((subscription, snapshot)) => json!({
            "type": "response", "id": id, "ok": true,
            "result": {"type": "subscribed", "data": {"subscription": subscription, "snapshot": snapshot}},
        }),
        Err(error) => json!({"type": "response", "id": id, "ok": false, "error": error}),
    }
}

/// The response to `unsubscribe`.
#[must_use]
pub fn unsubscribe_response(id: &Value, outcome: Result<u64, CommandError>) -> Value {
    match outcome {
        Ok(subscription) => json!({
            "type": "response", "id": id, "ok": true,
            "result": {"type": "unsubscribed", "data": {"subscription": subscription}},
        }),
        Err(error) => json!({"type": "response", "id": id, "ok": false, "error": error}),
    }
}

/// One observed signal update.
enum Change {
    Chat(Box<ChatState>),
    Invitations(InvitationsState),
    Contacts(ContactsState),
    Sync(SyncStatus),
    Connection(ConnectionStatus),
    Facts(Vec<aura_app::ui::signals::AuthoritativeSemanticFact>),
}

fn changes<T, F>(stream: SignalStream<T>, wrap: F) -> BoxStream<'static, Change>
where
    T: Clone + Send + 'static,
    F: Fn(T) -> Change + Send + 'static,
{
    stream::unfold(stream, |mut stream| async move {
        stream.recv().await.ok().map(|value| (value, stream))
    })
    .map(wrap)
    .boxed()
}

fn sync_value(status: &SyncStatus) -> Value {
    match status {
        SyncStatus::Idle => json!({"status": "idle"}),
        SyncStatus::Syncing { progress } => json!({"status": "syncing", "progress": progress}),
        SyncStatus::Synced => json!({"status": "synced"}),
        SyncStatus::Failed { message } => json!({"status": "failed", "message": message}),
    }
}

fn connection_value(status: &ConnectionStatus) -> Value {
    match status {
        ConnectionStatus::Offline => json!({"connection": "offline", "peers": 0}),
        ConnectionStatus::Connecting => json!({"connection": "connecting", "peers": 0}),
        ConnectionStatus::Online { peer_count } => {
            json!({"connection": "online", "peers": peer_count})
        }
    }
}

/// The last state observed for every topic; events are its diffs.
#[derive(Default)]
struct Observed {
    chat: ChatState,
    messages: HashSet<(ChannelId, String)>,
    invitations: InvitationsState,
    pending: HashSet<String>,
    contacts: BTreeSet<AuthorityId>,
    members: BTreeMap<ChannelId, BTreeSet<AuthorityId>>,
    sync: Value,
    connection: Value,
    operations: BTreeMap<String, Value>,
    drops: Vec<Value>,
    failures: Vec<Value>,
}

type Event = (Topic, &'static str, Value);

impl Observed {
    fn apply(&mut self, change: Change, out: &mut Vec<Event>) {
        match change {
            Change::Chat(chat) => {
                for channel in chat.all_channels() {
                    for message in chat.messages_for_channel(&channel.id) {
                        if self.messages.insert((channel.id, message.id.clone())) {
                            out.push((
                                Topic::Messages,
                                "message",
                                serde_json::to_value(message_view(message)).unwrap_or(Value::Null),
                            ));
                        }
                    }
                    let members: BTreeSet<AuthorityId> =
                        channel.member_ids.iter().copied().collect();
                    if self.members.get(&channel.id) != Some(&members) {
                        out.push((
                            Topic::Membership,
                            "channel_members",
                            json!({
                                "channel_id": channel.id.to_string(),
                                "name": channel.name,
                                "members": members.iter().map(ToString::to_string).collect::<Vec<_>>(),
                            }),
                        ));
                        self.members.insert(channel.id, members);
                    }
                }
                self.chat = *chat;
            }
            Change::Invitations(invitations) => {
                for invitation in invitations.all_pending() {
                    if self.pending.insert(invitation.id.clone()) {
                        out.push((
                            Topic::Invitations,
                            "invitation",
                            serde_json::to_value(invitation).unwrap_or(Value::Null),
                        ));
                    }
                }
                self.invitations = invitations;
            }
            Change::Contacts(contacts) => {
                let now: BTreeSet<AuthorityId> = contacts.all_contacts().map(|c| c.id).collect();
                if now != self.contacts {
                    out.push((
                        Topic::Membership,
                        "contacts",
                        json!({"contacts": now.iter().map(ToString::to_string).collect::<Vec<_>>()}),
                    ));
                    self.contacts = now;
                }
            }
            Change::Sync(status) => {
                let value = sync_value(&status);
                if value != self.sync {
                    out.push((Topic::Sync, "sync_status", value.clone()));
                    self.sync = value;
                }
            }
            Change::Connection(status) => {
                let value = connection_value(&status);
                if value != self.connection {
                    out.push((Topic::Sync, "connection", value.clone()));
                    self.connection = value;
                }
            }
            Change::Facts(facts) => {
                // The canonical operation-status projection of the facts, the
                // one the TUI and web surfaces show.
                for (operation_id, instance_id, _causality, status) in
                    bridged_operation_statuses(&facts)
                {
                    let key = format!(
                        "{}#{}",
                        operation_id.0,
                        instance_id.as_ref().map(|i| i.0.as_str()).unwrap_or("")
                    );
                    let value = json!({
                        "operation_id": operation_id,
                        "instance_id": instance_id,
                        "kind": status.kind,
                        "phase": status.phase,
                        "error": status.error,
                    });
                    if self.operations.get(&key) != Some(&value) {
                        out.push((Topic::Operations, "operation", value.clone()));
                        self.operations.insert(key, value);
                    }
                }
            }
        }
    }

    /// Compare the runtime's diagnostic logs with what was last seen.
    fn diagnostics(&mut self, drops: Vec<Value>, failures: Vec<Value>, out: &mut Vec<Event>) {
        for drop in &drops {
            if !self.drops.contains(drop) {
                out.push((Topic::Drops, "message_drop", drop.clone()));
            }
        }
        for failure in &failures {
            if !self.failures.contains(failure) {
                out.push((Topic::TaskFailures, "task_failure", failure.clone()));
            }
        }
        self.drops = drops;
        self.failures = failures;
    }

    fn snapshot(&self, topics: &BTreeSet<Topic>) -> Value {
        let mut snapshot = serde_json::Map::new();
        for topic in topics {
            let value = match topic {
                Topic::Messages => {
                    let mut channels: Vec<Value> = self
                        .chat
                        .all_channels()
                        .map(|channel| {
                            let messages = self.chat.messages_for_channel(&channel.id);
                            json!({
                                "channel": channel_view(channel, false),
                                "message_count": messages.len(),
                                "last_message": messages.last().map(message_view),
                            })
                        })
                        .collect();
                    channels.sort_by_key(|c| c["channel"]["name"].to_string());
                    json!(channels)
                }
                Topic::Invitations => {
                    serde_json::to_value(self.invitations.all_pending()).unwrap_or(Value::Null)
                }
                Topic::Membership => json!({
                    "contacts": self.contacts.iter().map(ToString::to_string).collect::<Vec<_>>(),
                    "channels": self.members.iter().map(|(id, members)| json!({
                        "channel_id": id.to_string(),
                        "members": members.iter().map(ToString::to_string).collect::<Vec<_>>(),
                    })).collect::<Vec<_>>(),
                }),
                Topic::Sync => json!({"sync": self.sync, "connection": self.connection}),
                Topic::Operations => json!(self.operations.values().collect::<Vec<_>>()),
                Topic::Drops => json!(self.drops),
                Topic::TaskFailures => json!(self.failures),
            };
            snapshot.insert(topic.name().to_string(), value);
        }
        Value::Object(snapshot)
    }
}

struct Subscription {
    id: u64,
    topics: BTreeSet<Topic>,
    queue: VecDeque<Value>,
    overflowed: bool,
}

impl Subscription {
    fn push(&mut self, event: Value) {
        if self.overflowed {
            return;
        }
        if self.queue.len() >= QUEUE_CAPACITY {
            self.queue.clear();
            self.overflowed = true;
        } else {
            self.queue.push_back(event);
        }
    }
}

/// The subscriptions of one RPC session.
pub struct Subscriptions {
    app_core: Arc<RwLock<AppCore>>,
    changes: Option<SelectAll<BoxStream<'static, Change>>>,
    observed: Observed,
    subscriptions: Vec<Subscription>,
    next_id: u64,
}

impl Subscriptions {
    /// No subscriptions yet; signal streams open on the first `subscribe`.
    #[must_use]
    pub fn new(app_core: Arc<RwLock<AppCore>>) -> Self {
        Self {
            app_core,
            changes: None,
            observed: Observed::default(),
            subscriptions: Vec::new(),
            next_id: 1,
        }
    }

    async fn open(&mut self) -> Result<(), CommandError> {
        if self.changes.is_some() {
            return Ok(());
        }
        let core = self.app_core.read().await;
        let failed = |e: aura_core::effects::reactive::ReactiveError| {
            CommandError::new(super::super::command::ErrorCode::Unavailable, e.to_string())
        };
        // Await each receiver attachment before reading the baseline, so the
        // subscribe response guarantees that the first subsequent update is observed.
        let streams = vec![
            changes(
                core.subscribe_attached(&*CHAT_SIGNAL)
                    .await
                    .map_err(failed)?,
                |chat| Change::Chat(Box::new(chat)),
            ),
            changes(
                core.subscribe_attached(&*INVITATIONS_SIGNAL)
                    .await
                    .map_err(failed)?,
                Change::Invitations,
            ),
            changes(
                core.subscribe_attached(&*CONTACTS_SIGNAL)
                    .await
                    .map_err(failed)?,
                Change::Contacts,
            ),
            changes(
                core.subscribe_attached(&*SYNC_STATUS_SIGNAL)
                    .await
                    .map_err(failed)?,
                Change::Sync,
            ),
            changes(
                core.subscribe_attached(&*CONNECTION_STATUS_SIGNAL)
                    .await
                    .map_err(failed)?,
                Change::Connection,
            ),
            changes(
                core.subscribe_attached(&*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL)
                    .await
                    .map_err(failed)?,
                |facts| Change::Facts(facts.facts),
            ),
        ];
        let mut baseline = Vec::new();
        let initial = [
            core.read(&*CHAT_SIGNAL)
                .await
                .map(|chat| Change::Chat(Box::new(chat))),
            core.read(&*INVITATIONS_SIGNAL)
                .await
                .map(Change::Invitations),
            core.read(&*CONTACTS_SIGNAL).await.map(Change::Contacts),
            core.read(&*SYNC_STATUS_SIGNAL).await.map(Change::Sync),
            core.read(&*CONNECTION_STATUS_SIGNAL)
                .await
                .map(Change::Connection),
            core.read(&*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL)
                .await
                .map(|facts| Change::Facts(facts.facts)),
        ];
        drop(core);
        for change in initial {
            self.observed.apply(change.map_err(failed)?, &mut baseline);
        }
        let (drops, failures) = self.diagnostics().await;
        self.observed.diagnostics(drops, failures, &mut baseline);
        self.changes = Some(stream::select_all(streams));
        Ok(())
    }

    /// The diagnostics read, as a future that does not borrow `self`.
    fn diagnostics(&self) -> impl std::future::Future<Output = (Vec<Value>, Vec<Value>)> + Send {
        diagnostics(self.app_core.clone())
    }
}

async fn diagnostics(app_core: Arc<RwLock<AppCore>>) -> (Vec<Value>, Vec<Value>) {
    {
        let Ok(runtime) = require_runtime(&app_core).await else {
            return (Vec::new(), Vec::new());
        };
        let values = |items: Vec<Result<Value, serde_json::Error>>| {
            items
                .into_iter()
                .map(|item| item.unwrap_or(Value::Null))
                .collect()
        };
        (
            values(
                runtime
                    .message_drops()
                    .iter()
                    .map(serde_json::to_value)
                    .collect(),
            ),
            values(
                runtime
                    .supervised_task_failures()
                    .iter()
                    .map(serde_json::to_value)
                    .collect(),
            ),
        )
    }
}

impl Subscriptions {
    /// Start a subscription; returns its id and a snapshot of its topics.
    pub async fn subscribe(&mut self, topics: Vec<Topic>) -> Result<(u64, Value), CommandError> {
        self.open().await?;
        let topics: BTreeSet<Topic> = topics.into_iter().collect();
        let id = self.next_id;
        self.next_id += 1;
        let snapshot = self.observed.snapshot(&topics);
        self.subscriptions.push(Subscription {
            id,
            topics,
            queue: VecDeque::new(),
            overflowed: false,
        });
        Ok((id, snapshot))
    }

    /// Stop a subscription.
    pub fn unsubscribe(&mut self, id: u64) -> Result<u64, CommandError> {
        let before = self.subscriptions.len();
        self.subscriptions.retain(|s| s.id != id);
        if self.subscriptions.len() == before {
            return Err(CommandError::not_found(format!("no subscription {id}")));
        }
        Ok(id)
    }

    fn pop(&mut self) -> Option<Value> {
        for subscription in &mut self.subscriptions {
            if subscription.overflowed {
                subscription.overflowed = false;
                return Some(json!({
                    "type": "event",
                    "subscription": subscription.id,
                    "topic": "resync",
                    "kind": "resync",
                    "data": self.observed.snapshot(&subscription.topics),
                }));
            }
            if let Some(event) = subscription.queue.pop_front() {
                return Some(event);
            }
        }
        None
    }

    fn fan_out(&mut self, events: Vec<Event>) {
        for (topic, kind, data) in events {
            for subscription in &mut self.subscriptions {
                if subscription.topics.contains(&topic) {
                    subscription.push(json!({
                        "type": "event",
                        "subscription": subscription.id,
                        "topic": topic.name(),
                        "kind": kind,
                        "data": data,
                    }));
                }
            }
        }
    }

    /// The next event line; pends while there is nothing to send.
    pub async fn next_event(&mut self) -> Option<Value> {
        loop {
            if let Some(event) = self.pop() {
                return Some(event);
            }
            let next = match self.changes.as_mut() {
                Some(changes) if !self.subscriptions.is_empty() => changes.next().await,
                _ => None,
            };
            let Some(change) = next else {
                return futures::future::pending().await;
            };
            let mut events = Vec::new();
            self.observed.apply(change, &mut events);
            let (drops, failures) = self.diagnostics().await;
            self.observed.diagnostics(drops, failures, &mut events);
            self.fan_out(events);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn subscribe_attaches_before_the_first_immediate_update() {
        let core = Arc::new(RwLock::new(
            AppCore::new(aura_app::AppConfig::default()).unwrap(),
        ));
        AppCore::init_signals_with_hooks(&core).await.unwrap();
        let mut subscriptions = Subscriptions::new(core.clone());
        let (id, _) = subscriptions.subscribe(vec![Topic::Sync]).await.unwrap();
        {
            let core = core.read().await;
            for signal in [
                CHAT_SIGNAL.id(),
                INVITATIONS_SIGNAL.id(),
                CONTACTS_SIGNAL.id(),
                SYNC_STATUS_SIGNAL.id(),
                CONNECTION_STATUS_SIGNAL.id(),
                AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL.id(),
            ] {
                assert_eq!(core.reactive().graph().subscriber_count(signal).await, 1);
            }
            core.emit(&*SYNC_STATUS_SIGNAL, SyncStatus::Synced)
                .await
                .unwrap();
        }
        let event = subscriptions.next_event().await.unwrap();
        assert_eq!(event["subscription"], id);
        assert_eq!(event["topic"], "sync");
        assert_eq!(event["data"]["status"], "synced");
    }

    #[tokio::test]
    async fn subscribe_rejects_unregistered_signals_without_accepting_a_subscription() {
        let core = Arc::new(RwLock::new(
            AppCore::new(aura_app::AppConfig::default()).unwrap(),
        ));
        let mut subscriptions = Subscriptions::new(core.clone());
        assert!(subscriptions.subscribe(vec![Topic::Sync]).await.is_err());
        assert!(subscriptions.subscriptions.is_empty());
        assert!(subscriptions.changes.is_none());
        assert_eq!(
            core.read()
                .await
                .reactive()
                .graph()
                .subscriber_count(CHAT_SIGNAL.id())
                .await,
            0
        );
    }

    #[test]
    fn topics_parse_by_name_and_default_to_all() {
        assert_eq!(parse_topics(&json!({})).unwrap(), Topic::ALL.to_vec());
        assert_eq!(
            parse_topics(&json!({"topics": ["messages", "sync"]})).unwrap(),
            vec![Topic::Messages, Topic::Sync]
        );
        assert!(parse_topics(&json!({"topics": ["nope"]})).is_err());
    }

    #[test]
    fn a_full_queue_overflows_into_one_resync() {
        let mut subscription = Subscription {
            id: 1,
            topics: BTreeSet::from([Topic::Messages]),
            queue: VecDeque::new(),
            overflowed: false,
        };
        for i in 0..=QUEUE_CAPACITY {
            subscription.push(json!(i));
        }
        assert!(subscription.overflowed);
        assert!(subscription.queue.is_empty());
        subscription.push(json!("late"));
        assert!(subscription.queue.is_empty());
    }

    #[test]
    fn chat_diffs_report_each_message_once() {
        let mut observed = Observed::default();
        let channel = aura_app::ui::types::Channel {
            id: ChannelId::from_bytes([1; 32]),
            name: "general".into(),
            ..Default::default()
        };
        let mut chat = ChatState::from_channels([channel.clone()]);
        let message = aura_app::ui::types::Message {
            id: "m1".into(),
            channel_id: channel.id,
            sender_id: AuthorityId::new_from_entropy([2; 32]),
            sender_name: "Alex".into(),
            content: "hi".into(),
            timestamp: 1,
            reply_to: None,
            is_own: false,
            is_read: false,
            delivery_status: aura_app::ui::types::MessageDeliveryStatus::default(),
            epoch_hint: None,
            is_finalized: false,
        };
        chat.apply_message(channel.id, message);
        let mut events = Vec::new();
        observed.apply(Change::Chat(Box::new(chat.clone())), &mut events);
        assert_eq!(events.iter().filter(|e| e.0 == Topic::Messages).count(), 1);
        events.clear();
        observed.apply(Change::Chat(Box::new(chat)), &mut events);
        assert!(events.is_empty());
    }
}
