# Reactive Signals Guide

## Overview

The Aura TUI uses `futures-signals` for fine-grained reactive state management. This provides automatic change propagation without manual lock management or explicit notification code.

## Core Concepts

### Signal Types

#### `Mutable<T>` - Single Reactive Values
A `Mutable<T>` holds a single value that can change over time. When the value changes, all subscribers are automatically notified.

```rust
use futures_signals::signal::Mutable;

let count = Mutable::new(0);
count.set(5);                    // Update value
let value = count.get_cloned();  // Read current value
```

#### `MutableVec<T>` - Reactive Collections
A `MutableVec<T>` is a reactive vector that notifies subscribers when items are added, removed, or modified.

```rust
use futures_signals::signal_vec::MutableVec;

let items = MutableVec::new();
items.lock_mut().push_cloned("item1");
items.lock_mut().remove(0);
```

### Wrapper Types

We provide two wrapper types that simplify common patterns:

#### `ReactiveState<T>`
Wraps `Mutable<T>` with convenient methods:

```rust
use crate::tui::reactive::signals::ReactiveState;

let state = ReactiveState::new(42);

// Reading
let value = state.get();  // Returns cloned value

// Writing
state.set(100);           // Replace entire value
state.update(|v| *v += 1); // Update in-place

// Signal exposure
let signal = state.signal();  // Get signal for subscriptions
```

#### `ReactiveVec<T>`
Wraps `MutableVec<T>` with convenient methods:

```rust
use crate::tui::reactive::signals::ReactiveVec;

let vec = ReactiveVec::new();

// Mutations
vec.push(item);
vec.remove(index);
vec.replace(new_items);
vec.update_at(index, |item| item.status = Active);

// Reading
let items = vec.get_cloned();
let len = vec.len();

// Signal exposure
let signal = vec.signal_vec();      // Get signal vec
let count = vec.count_signal();     // Get signal for count only
```

## View Architecture

### View Pattern

Each view uses `ReactiveState` and `ReactiveVec` for its state:

```rust
#[derive(Clone)]
pub struct ChatView {
    // Reactive collections
    channels: ReactiveVec<Channel>,
    messages: ReactiveVec<Message>,

    // Reactive values
    selected_channel: ReactiveState<Option<String>>,
}

impl ChatView {
    pub fn new() -> Self {
        Self {
            channels: ReactiveVec::new(),
            messages: ReactiveVec::new(),
            selected_channel: ReactiveState::new(None),
        }
    }

    // Synchronous delta application
    pub fn apply_delta(&self, delta: ChatDelta) {
        match delta {
            ChatDelta::ChannelAdded { channel } => {
                self.channels.push(channel);
                // Signals automatically notify subscribers!
            }
            ChatDelta::MessageReceived { channel_id, message } => {
                if self.selected_channel.get() == Some(channel_id) {
                    self.messages.push(message);
                }
            }
        }
    }

    // Getters
    pub fn get_channels(&self) -> Vec<Channel> {
        self.channels.get_cloned()
    }

    pub fn get_selected_channel(&self) -> Option<String> {
        self.selected_channel.get()
    }

    // Signal exposure for reactive UI
    pub fn channels_signal(&self) -> impl SignalVec<Item = Channel> {
        self.channels.signal_vec()
    }

    pub fn selected_channel_signal(&self) -> impl Signal<Item = Option<String>> {
        self.selected_channel.signal()
    }
}
```

### ReactiveViewModel - Cross-View State

The `ReactiveViewModel` aggregates all views and provides cross-view derived state:

```rust
pub struct ReactiveViewModel {
    pub chat: ChatView,
    pub guardians: GuardiansView,
    pub recovery: RecoveryView,
    pub invitations: InvitationsView,
    pub block: BlockView,
}

impl ReactiveViewModel {
    // Cross-view derived state
    pub fn pending_notifications_count(&self) -> usize {
        self.invitations.pending_count() +
        if matches!(self.recovery.get_status().state, RecoveryState::Initiated) { 1 } else { 0 }
    }

    pub fn has_critical_notifications(&self) -> bool {
        matches!(self.recovery.get_status().state, RecoveryState::ThresholdMet)
    }

    pub fn get_dashboard_stats(&self) -> DashboardStats {
        DashboardStats {
            total_channels: self.chat.get_channels().len(),
            total_guardians: self.guardians.get_guardians().len(),
            pending_invitations: self.invitations.pending_count(),
            block_residents: self.block.get_residents().len(),
            storage_used_percent: /* calculate from block.get_storage() */,
        }
    }
}
```

## Best Practices

### 1. Synchronous Delta Application

Delta application should be **synchronous** (not async):

```rust
// ✅ GOOD: Synchronous
pub fn apply_delta(&self, delta: ChatDelta) {
    match delta {
        ChatDelta::ChannelAdded { channel } => {
            self.channels.push(channel);
        }
    }
}

// ❌ BAD: Async (unnecessary)
pub async fn apply_delta(&self, delta: ChatDelta) {
    // No awaits needed!
}
```

**Why?** Signals automatically notify subscribers. No async operations needed.

### 2. Clone for Reading, Update for Mutation

Use `.get()` for reading, `.set()` or `.update()` for mutations:

```rust
// Reading
let channels = self.channels.get_cloned();
let selected = self.selected_channel.get();

// Mutation - replace entire value
self.selected_channel.set(Some("channel-1".to_string()));

// Mutation - update in-place
self.selected_channel.update(|val| {
    *val = Some("channel-1".to_string());
});
```

### 3. Expose Signals for Reactive UI

Provide signal accessors for UI components that need reactive updates:

```rust
impl ChatView {
    // Expose signal for selected channel
    pub fn selected_channel_signal(&self) -> impl Signal<Item = Option<String>> {
        self.selected_channel.signal()
    }

    // Expose signal vec for channels list
    pub fn channels_signal(&self) -> impl SignalVec<Item = Channel> {
        self.channels.signal_vec()
    }

    // Expose derived signal (count only)
    pub fn channel_count_signal(&self) -> impl Signal<Item = usize> {
        self.channels.count_signal()
    }
}
```

### 4. Use Derived State for Computed Values

Create methods that compute values from multiple signals:

```rust
impl ChatView {
    // Synchronous computed value (sampled)
    pub fn get_unread_count(&self) -> usize {
        self.get_channels()
            .iter()
            .filter(|c| c.has_unread)
            .count()
    }

    // For filters and sorts, compute on-demand
    pub fn get_active_channels(&self) -> Vec<Channel> {
        self.get_channels()
            .into_iter()
            .filter(|c| !c.is_archived)
            .collect()
    }
}
```

### 5. Avoid Holding Locks

Never hold a lock guard across an await point or for extended periods:

```rust
// ❌ BAD: Holding lock across operations
let mut items = vec.as_mutable_vec().lock_mut();
items.push_cloned(item1);
expensive_computation();  // Lock still held!
items.push_cloned(item2);

// ✅ GOOD: Short-lived lock scopes
vec.push(item1);
expensive_computation();
vec.push(item2);
```

## Common Patterns

### Pattern 1: Simple State Updates

```rust
// Update single value
self.selected_channel.set(Some(channel_id));

// Update collection
self.messages.push(new_message);
self.channels.remove(index);
```

### Pattern 2: Batch Updates

```rust
// Replace entire collection
let new_channels = compute_channels();
self.channels.replace(new_channels);

// Update multiple items
for (index, update) in updates.iter().enumerate() {
    self.items.update_at(index, |item| {
        item.status = update.status;
    });
}
```

### Pattern 3: Conditional Updates

```rust
pub fn apply_delta(&self, delta: ChatDelta) {
    match delta {
        ChatDelta::MessageReceived { channel_id, message } => {
            // Only update if this is the selected channel
            if self.selected_channel.get() == Some(channel_id.clone()) {
                self.messages.push(message);
            }
        }
    }
}
```

### Pattern 4: Cross-View Queries

```rust
impl ReactiveViewModel {
    pub fn get_notification_summary(&self) -> NotificationSummary {
        NotificationSummary {
            pending_invitations: self.invitations.pending_count(),
            recovery_awaiting: if matches!(
                self.recovery.get_status().state,
                RecoveryState::Initiated
            ) { 1 } else { 0 },
            storage_critical: self.block.get_storage_percentage() > 90.0,
            recovery_ready: matches!(
                self.recovery.get_status().state,
                RecoveryState::ThresholdMet
            ),
        }
    }
}
```

## Migration from Arc<RwLock<T>>

### Before (Manual State Management)

```rust
pub struct ChatView {
    channels: Arc<RwLock<Vec<Channel>>>,
    update_tx: broadcast::Sender<ViewUpdate>,
}

impl ChatView {
    pub async fn apply_delta(&self, delta: ChatDelta) {
        match delta {
            ChatDelta::ChannelAdded { channel } => {
                let mut channels = self.channels.write().await;
                channels.push(channel);
                drop(channels);  // Release lock

                let _ = self.update_tx.send(ViewUpdate::ChannelsChanged);
            }
        }
    }

    pub async fn get_channels(&self) -> Vec<Channel> {
        self.channels.read().await.clone()
    }
}
```

### After (Reactive Signals)

```rust
pub struct ChatView {
    channels: ReactiveVec<Channel>,
    // No update_tx needed!
}

impl ChatView {
    pub fn apply_delta(&self, delta: ChatDelta) {
        match delta {
            ChatDelta::ChannelAdded { channel } => {
                self.channels.push(channel);
                // Automatically notifies subscribers!
            }
        }
    }

    pub fn get_channels(&self) -> Vec<Channel> {
        self.channels.get_cloned()
    }
}
```

### Benefits

1. **Less Boilerplate**: No manual lock acquisition, no manual notifications
2. **No Async Overhead**: Delta application is synchronous
3. **Automatic Propagation**: Signals handle notification automatically
4. **Better Composability**: Easy to create derived state and cross-view queries
5. **Type Safety**: Signal types enforce proper usage patterns

## Testing

### Testing View State

```rust
#[test]
fn test_channel_addition() {
    let view = ChatView::new();

    // Apply delta
    view.apply_delta(ChatDelta::ChannelAdded {
        channel: Channel {
            id: "ch1".to_string(),
            name: "general".to_string(),
        },
    });

    // Verify state updated
    let channels = view.get_channels();
    assert_eq!(channels.len(), 1);
    assert_eq!(channels[0].name, "general");
}
```

### Testing Derived State

```rust
#[test]
fn test_notification_count() {
    let view_model = ReactiveViewModel::new();

    // Add pending invitation
    view_model.invitations.apply_delta(InvitationDelta::Received { /* ... */ });

    // Check derived state
    assert_eq!(view_model.pending_notifications_count(), 1);
}
```

## Further Reading

- **futures-signals documentation**: https://docs.rs/futures-signals/
- **Aura reactive architecture**: `work/signals.md`
- **View implementations**: `crates/aura-terminal/src/tui/reactive/views.rs`
- **Signal utilities**: `crates/aura-terminal/src/tui/reactive/signals.rs`
