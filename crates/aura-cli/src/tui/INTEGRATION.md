# TUI Effect System Integration Guide

This document describes the completed infrastructure for TUI effect system integration and the roadmap for connecting to the Aura backend.

## Completed Infrastructure (Phase 8 Tasks 8.1-8.4)

### 1. IRC Command System

**Files Created/Modified:**
- `crates/aura-cli/src/tui/commands.rs` - 19 IRC commands with validation
- `crates/aura-cli/src/tui/input.rs` - Added `InputAction::Command(IrcCommand)` and `InputAction::Error(String)`
- `crates/aura-cli/src/tui/screens/chat.rs:147-168` - Command detection and parsing

**Features:**
- Complete IRC-style command parser (`/msg`, `/kick`, `/ban`, `/mute`, etc.)
- Command validation and help system
- Capability annotations for authorization
- 50+ unit tests

**Usage Example:**
```rust
use aura_cli::tui::commands::{parse_command, IrcCommand};

// In chat screen, when user types "/msg alice hello"
let input = "/msg alice hello";
match parse_command(input) {
    Ok(IrcCommand::Msg { target, text }) => {
        // Returns: target="alice", text="hello"
    }
    Err(e) => {
        // Handle parse error
    }
}
```

### 2. Command Dispatcher

**Files Created:**
- `crates/aura-cli/src/tui/effects/dispatcher.rs` - Maps IRC commands to effect commands

**Features:**
- Maps `IrcCommand` → `EffectCommand` with capability checking
- Context management (e.g., current channel for `/me`, `/kick`)
- Stub capability checking ready for Biscuit integration
- 7 comprehensive unit tests

**Usage Example:**
```rust
use aura_cli::tui::effects::{CommandDispatcher, EffectCommand};
use aura_cli::tui::commands::IrcCommand;

let mut dispatcher = CommandDispatcher::new();
dispatcher.set_current_channel("general");

let cmd = IrcCommand::Me { action: "waves".to_string() };
let effect_cmd = dispatcher.dispatch(cmd)?;
// Returns: EffectCommand::SendAction { channel: "general", action: "waves" }
```

### 3. Effect Bridge

**Files Created/Modified:**
- `crates/aura-cli/src/tui/effects/bridge.rs` - Command dispatch and event emission

**Features:**
- Background async command consumer loop
- Retry logic with exponential backoff (3 retries, configurable)
- Event broadcasting with type-safe filters
- 19 effect command variants
- Comprehensive error handling
- 10+ unit tests

**Architecture:**
```
User Input → IRC Parser → CommandDispatcher → EffectBridge → [Effect Handlers]
                                                     ↓
                                              AuraEvent emission
                                                     ↓
                                              Screen updates
```

**Usage Example:**
```rust
use aura_cli::tui::effects::{EffectBridge, EffectCommand, EventFilter};

// Create bridge
let bridge = EffectBridge::new();

// Subscribe to events
let mut subscription = bridge.subscribe(EventFilter::all());

// Dispatch command (fire-and-forget)
bridge.dispatch(EffectCommand::SendMessage {
    channel: "general".to_string(),
    content: "Hello!".to_string(),
}).await?;

// Or dispatch and wait for completion
bridge.dispatch_and_wait(EffectCommand::Ping).await?;

// Receive events
while let Some(event) = subscription.recv().await {
    match event {
        AuraEvent::MessageReceived { channel, from, content, .. } => {
            println!("New message in {}: {} says {}", channel, from, content);
        }
        _ => {}
    }
}
```

### 4. TUI Context

**Files Created:**
- `crates/aura-cli/src/tui/context.rs` - Centralized access to EffectBridge

**Features:**
- Wrapper around EffectBridge for easy access
- Authority ID management
- Connection state tracking
- Convenient dispatch methods
- 4 unit tests

**Usage Example:**
```rust
use aura_cli::tui::context::TuiContext;
use aura_cli::tui::effects::EffectCommand;

// Create context (typically done once at app startup)
let ctx = TuiContext::with_defaults();

// Set current user's authority
ctx.set_authority(authority_id).await;

// Dispatch commands from anywhere in the TUI
ctx.dispatch(EffectCommand::Ping).await?;

// Subscribe to events
let mut events = ctx.subscribe_all();
```

## Integration Requirements (Tasks 8.5-8.10)

The remaining Phase 8 tasks require backend infrastructure that isn't currently running in the demo. Here's what each task needs:

### Task 8.5: Wire reactive queries to journal via Biscuit

**Requirement:** Journal system with Biscuit Datalog query execution

**What's needed:**
1. Journal instance with fact storage (`aura-journal`)
2. Biscuit Authorizer for executing Datalog queries
3. Query executor that:
   - Takes a `TuiQuery` (e.g., `ChannelsQuery`, `MessagesQuery`)
   - Calls `query.to_datalog()` to get Datalog rules
   - Executes against journal via Biscuit
   - Parses results into typed data (e.g., `Vec<Channel>`)
   - Subscribes to predicate changes for reactivity

**Infrastructure already in place:**
- ✅ Query types with `to_datalog()` methods (`crates/aura-cli/src/tui/reactive/queries.rs`)
- ✅ `TuiQuery` trait for type-safe queries
- ✅ Predicate dependencies via `predicates()` method

**Example of what needs to be implemented:**
```rust
// This is what needs to be built
pub struct QueryExecutor {
    journal: Arc<Journal>,
    authorizer: Arc<BiscuitAuthorizer>,
}

impl QueryExecutor {
    pub async fn execute<Q: TuiQuery>(&self, query: Q) -> Result<Q::Result> {
        // 1. Generate Datalog query
        let datalog = query.to_datalog();

        // 2. Execute against journal via Biscuit
        let facts = self.authorizer.query(&datalog, &self.journal)?;

        // 3. Parse results into typed data
        // (type-specific parsing logic)

        Ok(results)
    }

    pub fn subscribe<Q: TuiQuery>(&self, query: Q) -> QuerySubscription<Q::Result> {
        // Subscribe to changes in query.predicates()
        // Re-execute query when predicates change
    }
}
```

### Task 8.6: Connect ViewState updates to screen rendering

**Requirement:** Active QueryExecutor subscribing to journal changes

**What's needed:**
1. QueryExecutor from Task 8.5
2. Wire ViewState to QuerySubscription
3. Update screens when ViewState changes

**Infrastructure already in place:**
- ✅ `ViewState<T>` wrapper (`crates/aura-cli/src/tui/reactive/views.rs`)
- ✅ View types: `ChatView`, `GuardiansView`, `InvitationsView`, `RecoveryView`
- ✅ Update broadcasting via `tokio::sync::broadcast`

**Example:**
```rust
// This is what needs to be built
pub struct ChatView {
    query_executor: Arc<QueryExecutor>,
    channels: Arc<RwLock<ViewState<Vec<Channel>>>>,
    update_tx: broadcast::Sender<ChatViewUpdate>,
}

impl ChatView {
    pub async fn start_subscriptions(&self) {
        let query = ChannelsQuery::new();
        let mut subscription = self.query_executor.subscribe(query);

        while let Some(channels) = subscription.recv().await {
            let mut state = self.channels.write().await;
            state.set_data(channels);
            let _ = self.update_tx.send(ChatViewUpdate::ChannelsUpdated);
        }
    }
}
```

### Tasks 8.7-8.9: Populate screens with real data

**Requirement:** Completed Tasks 8.5 and 8.6

**What's needed:**
1. Connect each screen to its ViewState
2. Replace mock data with reactive queries
3. Update rendering when ViewState changes

**Infrastructure already in place:**
- ✅ All screen types with component rendering
- ✅ Query types for each screen (ChannelsQuery, MessagesQuery, GuardiansQuery, etc.)
- ✅ ViewState wrappers ready to hold data

### Task 8.10: Implement Settings screen

**Requirement:** Backend preference storage (likely LocalStore)

**What's needed:**
1. Settings data model
2. LocalStore integration for persistence
3. Settings screen UI
4. Wire settings to TUI behavior

**Infrastructure already in place:**
- ✅ LocalStore module (`crates/aura-cli/src/tui/local_store.rs`)
- ✅ Screen trait implementation pattern

## Integration Roadmap

### Phase 1: Query Executor (Task 8.5)

```rust
// 1. Create query executor in TuiContext
impl TuiContext {
    pub fn with_journal(bridge: EffectBridge, journal: Arc<Journal>) -> Self {
        let query_executor = Arc::new(QueryExecutor::new(journal));
        Self {
            bridge: Arc::new(bridge),
            query_executor: Some(query_executor),
            authority_id: Arc::new(RwLock::new(None)),
        }
    }
}

// 2. Implement QueryExecutor
// See example above in Task 8.5 section
```

### Phase 2: View Integration (Task 8.6)

```rust
// 1. Initialize views with QueryExecutor
let ctx = TuiContext::with_journal(bridge, journal);
let chat_view = ChatView::new(ctx.query_executor());

// 2. Start subscriptions
tokio::spawn(async move {
    chat_view.start_subscriptions().await;
});

// 3. Update screens on ViewState changes
impl ChatScreen {
    fn update(&mut self) {
        // Check if ViewState has new data
        // Re-render if needed
    }
}
```

### Phase 3: Screen Population (Tasks 8.7-8.9)

For each screen:
1. Replace `mock_data()` with `view_state.data()`
2. Add loading indicators when `view_state.is_loading()`
3. Show errors when `view_state.error()` is Some
4. React to ViewState updates

### Phase 4: Settings (Task 8.10)

1. Define settings model
2. Wire LocalStore for persistence
3. Build settings UI
4. Apply settings to TUI

## Testing the Infrastructure

Even without backend integration, you can test the infrastructure:

### Test Command Parsing
```bash
cargo test -p aura-cli --lib commands::tests
```

### Test Command Dispatch
```bash
cargo test -p aura-cli --lib effects::dispatcher::tests
```

### Test Effect Bridge
```bash
cargo test -p aura-cli --lib effects::bridge::tests
```

### Manual Integration Test
```rust
use aura_cli::tui::{TuiContext, EffectCommand, EventFilter};

#[tokio::test]
async fn test_full_flow() {
    let ctx = TuiContext::with_defaults();

    // Subscribe to events
    let mut events = ctx.subscribe(EventFilter::all());

    // Dispatch command
    ctx.dispatch(EffectCommand::Ping).await.unwrap();

    // Verify event received
    match events.try_recv() {
        Some(AuraEvent::Pong { latency_ms }) => {
            assert_eq!(latency_ms, 10); // Stub returns 10ms
        }
        _ => panic!("Expected Pong event"),
    }
}
```

## Current Limitations

1. **No Journal Integration**: QueryExecutor needs a running journal instance
2. **Stub Effect Handlers**: `EffectBridge::execute_command()` uses stubs that emit mock events
3. **No Biscuit Queries**: Reactive queries generate Datalog but can't execute yet
4. **No Capability Enforcement**: `CommandDispatcher::check_capability()` is a stub
5. **Mock Data Only**: Screens use static mock data instead of reactive queries

## Next Steps

To complete Phase 8 integration:

1. **Option A: Mock Data Layer** (Recommended for immediate demo)
   - Implement QueryExecutor with in-memory mock data
   - Allow testing full TUI flow without backend
   - Provides path for incremental real data integration

2. **Option B: Simulator Integration**
   - Wire EffectBridge to aura-simulator effect handlers
   - Use simulator's journal for queries
   - Full integration but requires simulator setup

3. **Option C: Wait for Backend**
   - Leave stubs as-is
   - Integrate when journal/effect runtime is ready
   - Cleanest separation but delays testing

## Files Modified/Created

### Created
- `crates/aura-cli/src/tui/context.rs` - TuiContext wrapper
- `crates/aura-cli/src/tui/effects/dispatcher.rs` - Command dispatcher

### Modified
- `crates/aura-cli/src/tui/input.rs` - Added Command/Error actions
- `crates/aura-cli/src/tui/screens/chat.rs` - IRC command detection
- `crates/aura-cli/src/tui/effects/bridge.rs` - Added documentation
- `crates/aura-cli/src/tui/effects/mod.rs` - Export dispatcher
- `crates/aura-cli/src/tui/mod.rs` - Export TuiContext

### Documentation References
- Effect system: `docs/106_effect_system_and_runtime.md`
- Capability system: `docs/109_authorization.md`
- Journal system: `docs/102_journal.md`
- Guard chain: `docs/001_system_architecture.md`
