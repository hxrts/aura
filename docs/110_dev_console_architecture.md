# 110 · Aura Dev Console Architecture

## Overview

The Aura Dev Console is the primary developer interface for interacting with, testing, and understanding Aura systems. It provides a unified environment for simulating choreographic protocols, connecting to live networks, designing test scenarios, and performing time travel debugging.

Unlike traditional debugging tools that focus solely on post-mortem analysis, the Aura Dev Console is an active development environment that supports:

- **Simulation Mode**: Run deterministic simulations with full time travel and checkpoint restoration
- **Live Network Mode**: Connect to real Aura networks, send messages, and observe protocol execution
- **Scenario Design Mode**: Visually design and export TOML-based test scenarios
- **Interactive REPL**: Command-line interface for direct system interaction
- **Visual Debugging**: Network topology, timeline scrubbing, state inspection, and choreography visualization

## Architecture Layers

The console uses a **client-server architecture** with clear separation between the browser UI and backend services. This ensures optimal performance, minimal bundle sizes, and proper separation of concerns.

```
┌──────────────────────────────────────────────────────────────────┐
│ Aura Dev Console (Browser)                                       │
│                                                                  │
│  ┌────────────────────────────────────────────────────────────┐ │
│  │ UI Layer (Leptos → HTML/CSS/JS)                            │ │
│  │                                                             │ │
│  │  ┌──────────┐  ┌──────────┐  ┌──────────┐                 │ │
│  │  │ Simulate │  │   Live   │  │ Scenario │                 │ │
│  │  │   Mode   │  │   Mode   │  │ Designer │                 │ │
│  │  └──────────┘  └──────────┘  └──────────┘                 │ │
│  │                                                             │ │
│  │  Network View | Timeline | State Inspector                 │ │
│  │  Message Composer | Protocol Stepper                       │ │
│  │  REPL | Event Log | Trace Export                           │ │
│  └─────────────────┬───────────────────────────────────────────┘ │
│                    │ WebSocket / HTTP                            │
│  ┌─────────────────▼───────────────────────────────────────────┐ │
│  │ Lightweight WASM Bridge (~100-200KB)                        │ │
│  │                                                             │ │
│  │ ┌──────────────┐  ┌──────────────┐  ┌──────────────┐      │ │
│  │ │ Simulation   │  │ Live Network │  │  Analysis    │      │ │
│  │ │ Client       │  │ Client       │  │  Engine      │      │ │
│  │ │              │  │              │  │              │      │ │
│  │ │ - WS client  │  │ - WS client  │  │ - Trace      │      │ │
│  │ │ - Streaming  │  │ - DeviceAgent│  │   parser     │      │ │
│  │ │ - Commands   │  │ - Auth       │  │ - Causality  │      │ │
│  │ └──────────────┘  └──────────────┘  └──────────────┘      │ │
│  │                                                             │ │
│  │  Dynamically loaded based on mode (no monolithic bundle)   │ │
│  └─────────────────────────────────────────────────────────────┘ │
└──────────────────────────────────────────────────────────────────┘
                               │
                               │ WebSocket
                               │
        ┌──────────────────────┼──────────────────────┐
        │                      │                      │
        ▼                      ▼                      ▼
┌───────────────┐      ┌───────────────┐     ┌───────────────┐
│ Simulation    │      │ Live Aura Node│     │ Analysis      │
│ Server        │      │ + Instrument. │     │ Server        │
│ (Native Rust) │      │ (Native Rust) │     │ (Native Rust) │
│               │      │               │     │               │
│ - Full Aura   │      │ - Full Aura   │     │ - Trace       │
│   stack       │      │   stack       │     │   processing  │
│ - Simulator   │      │ - Real P2P    │     │ - Checkpoint  │
│ - Checkpoints │      │   (SBB gossip)│     │   mgmt        │
│ - State mgmt  │      │ - Observ. API │     │ - Querying    │
│               │      │               │     │               │
│ ws://         │      │ ws://         │     │ ws://         │
│ localhost:9001│      │ localhost:9003│     │ localhost:9002│
└───────────────┘      └───────────────┘     └───────────────┘
                               │
                               │ Real P2P Network
                               │ (SBB, peer discovery)
                               ▼
                       ┌───────────────┐
                       │ Other Aura    │
                       │ Nodes/Devices │
                       └───────────────┘
```

### Layer 1: UI Layer (Leptos)

The UI layer is built with Leptos, a modern Rust framework that compiles to lightweight HTML/CSS/JavaScript. Leptos provides:

- **Fine-grained reactivity**: Efficient DOM updates without virtual DOM overhead
- **Small bundle size**: ~50-100KB gzipped for the entire UI
- **Type safety**: Full Rust type checking from backend to frontend
- **Fast compilation**: Incremental rebuilds during development

Key UI components:

- **Mode Switcher**: Toggle between Simulation, Live, and Design modes
- **Network View**: Interactive graph visualization of device topology using Cytoscape.js
- **Timeline**: Horizontal timeline with scrubbing, zoom, and event markers using D3.js
- **State Inspector**: JSON tree view of device state, ledger events, and CRDT documents
- **Message Composer**: Form-based interface for crafting and sending messages
- **Protocol Stepper**: Guided workflows for initiating DKD, resharing, recovery
- **Event Log**: Real-time stream of all network events with filtering
- **REPL**: Interactive command-line interface for direct system control
- **Choreography Visualizer**: State machine diagrams for active protocols

### Layer 2: WASM Bridge (Lightweight, Mode-Specific)

The WASM bridge provides minimal client-side logic for communicating with backend services. Critically, **it does not contain the full Aura stack** - instead, it provides thin clients that connect to native Rust services.

**Key Principle**: Dynamic loading based on mode. When the user switches modes, only the relevant WASM module is loaded, keeping memory footprint minimal.

**Bundle Sizes**:
- `simulation_client.wasm`: ~100-150KB (WebSocket client + streaming)
- `live_client.wasm`: ~150-200KB (WebSocket + minimal DeviceAgent for auth)
- `analysis_engine.wasm`: ~200-300KB (Trace parsing + causality graphs)

#### Simulation Client (simulation_client.wasm)

```rust
#[wasm_bindgen]
pub struct SimulationClient {
    ws: WebSocket,
    event_stream: VecDeque<TraceEvent>,
}
```

Responsibilities:
- Connect to simulation server via WebSocket (ws://localhost:9001)
- Send control commands (step, run, seek_to_tick, checkpoint)
- Stream trace events from server to UI
- Parse and buffer events for visualization
- **Does NOT run the simulation** - only visualizes it

#### Live Network Client (live_client.wasm)

```rust
#[wasm_bindgen]
pub struct LiveNetworkClient {
    instrumentation_ws: WebSocket,  // Only for observability, not P2P traffic
}
```

Responsibilities:
- Connect to **local instrumentation server** (not relay - P2P happens natively)
- Subscribe to trace events from local Aura node
- Send inspection commands (query state, inject message)
- Stream visualization data to UI
- **Does NOT participate in P2P** - observes local node via instrumentation API
- **Does NOT replace transport layer** - full Aura node runs natively with real P2P

#### Analysis Engine (analysis_engine.wasm)

Loaded on-demand for trace analysis (shared by both modes):

```rust
#[wasm_bindgen]
pub struct AnalysisEngine {
    trace_parser: TraceParser,
    causality_index: CausalityIndex,
}
```

Responsibilities:
- Parse trace binary format (postcard)
- Build causality graph (happens-before relationships)
- Provide efficient querying over events
- Generate graph data structures for visualization
- **Does NOT reconstruct full state** - queries backend for state at specific ticks

### Layer 3: Backend Services (Native Rust)

The actual computation happens in native Rust services running locally. These services provide **full performance** without WASM limitations and **separation of concerns** between different operational modes.

#### Simulation Server (Native Rust)

**Binary**: `aura-sim-server`

**Location**: `crates/sim-server/`

```rust
pub struct SimulationServer {
    simulation: InstrumentedSimulation,
    checkpoints: BTreeMap<u64, SimulationCheckpoint>,
    trace: TraceLog,
    websocket_server: WebSocketServer,
}
```

Responsibilities:
- Run full Aura simulation with deterministic effects
- Execute protocol state machines
- Manage checkpoints for time travel
- Stream trace events to connected clients
- Handle control commands (step, run, seek, inject)
- **Full Aura stack in native code** - maximum performance

**Protocol** (WebSocket JSON-RPC):
```json
// Client → Server
{"method": "step", "params": {"count": 10}}
{"method": "run_until_idle", "params": {}}
{"method": "seek_to_tick", "params": {"tick": 1500}}
{"method": "checkpoint", "params": {"label": "before_dkd"}}
{"method": "inject_message", "params": {"to": "alice", "message": {...}}}

// Server → Client (streaming)
{"event": "tick", "data": {"tick": 1500}}
{"event": "trace_events", "data": [{"tick": 1500, "type": "MessageSent", ...}]}
{"event": "violation", "data": {"property": "commitment_validity", ...}}
```

#### Live Aura Node with Instrumentation (Native Rust)

**Binary**: `aura` (existing CLI with `--dev-console` flag)

**Location**: `crates/cli/` (enhanced with instrumentation)

This is a **full Aura node** running natively with complete P2P transport (SBB gossip, etc.). The `--dev-console` flag enables an **instrumentation API** that exposes observability without replacing any core functionality.

```rust
// crates/agent/src/instrumentation.rs
pub struct InstrumentationServer {
    node: Arc<IntegratedAgent>,  // The actual Aura node
    websocket_server: WebSocketServer,
    trace_channel: mpsc::Receiver<TraceEvent>,
}

impl InstrumentationServer {
    pub fn new(node: Arc<IntegratedAgent>) -> Self {
        // Tap into node's existing trace stream
        let trace_channel = node.subscribe_trace_events();
        // ... setup WebSocket server for console
    }

    pub async fn handle_command(&mut self, cmd: ConsoleCommand) -> Result<Response> {
        match cmd {
            ConsoleCommand::QueryState { device_id } => {
                let state = self.node.get_state(&device_id)?;
                Ok(Response::State(state))
            }
            ConsoleCommand::GetTopology => {
                let topology = self.node.get_network_topology()?;
                Ok(Response::Topology(topology))
            }
            ConsoleCommand::InjectMessage { to, message } => {
                // Use node's real transport layer
                self.node.send_message(to, message).await?;
                Ok(Response::Success)
            }
            // Note: No "send via WebSocket relay" - messages go through real P2P transport
        }
    }
}
```

**Key Points**:
- **Full P2P transport**: SBB gossip, peer discovery, all normal Aura networking
- **Instrumentation is observation only**: Taps into existing trace streams
- **Commands use real APIs**: `inject_message` calls `IntegratedAgent.send_message()` which uses P2P transport
- **No relay**: WebSocket is purely for dev console ↔ local node communication
- **Normal Aura node**: Can participate in real network, not a special "console mode"

**Running Live Mode**:
```bash
# Terminal 1: Start Aura node with instrumentation
aura --dev-console --dev-console-port 9003

# Terminal 2: Start dev console UI
cd console && trunk serve

# Browser: Connect to ws://localhost:9003
# Console observes real P2P network activity
```

#### Analysis Server (Native Rust)

**Binary**: `aura-analysis-server` (optional optimization)

**Location**: `crates/analysis-server/`

For heavy trace analysis, an optional backend service can handle expensive operations:

```rust
pub struct AnalysisServer {
    traces: HashMap<TraceId, IndexedTrace>,
    websocket_server: WebSocketServer,
}
```

Responsibilities:
- Load and index large trace files
- Reconstruct state at arbitrary ticks (expensive operation)
- Compute causal chains
- Pattern matching over traces
- Export minimal reproductions

**Protocol**:
```json
{"method": "load_trace", "params": {"path": "./trace.bin"}}
{"method": "get_state_at_tick", "params": {"tick": 1500, "device": "alice"}}
{"method": "causality_path", "params": {"event_id": 42}}
{"method": "find_violations", "params": {"property": "threshold_safety"}}
```

## Data Flow

### Simulation Mode

```
User clicks "Step" button
  ↓
Leptos component calls console.step_simulation()
  ↓
WASM SimulationRuntime.step()
  ↓
InstrumentedSimulation executes one tick
  - Processes pending messages
  - Advances protocol state machines
  - Records trace events
  - Captures state snapshots
  ↓
Returns Vec<TraceEvent> to JavaScript
  ↓
Leptos reactive signals update
  ↓
UI re-renders affected components:
  - Timeline adds new event markers
  - Network view animates message flow
  - Event log appends entries
  - State inspector updates device state
```

### Live Network Mode

```
User clicks "Send Message" button
  ↓
Leptos MessageComposer calls console.send_message(to, msg)
  ↓
WASM LiveNetworkClient.send_command()
  - Serialize command to JSON-RPC
  - Send via instrumentation WebSocket
  ↓
WebSocket → Instrumentation Server (ws://localhost:9003)
  ↓
InstrumentationServer.handle_command(InjectMessage { to, msg })
  ↓
IntegratedAgent.send_message(to, msg)
  - Uses REAL P2P transport (SBB gossip)
  - Signs with node's credentials
  - Discovers recipient via peer discovery
  - Sends through Aura's transport layer
  ↓
Message propagates through real P2P network
  ↓
Recipient node receives via SBB
  ↓
Both sender and recipient emit trace events
  ↓
InstrumentationServer streams trace events over WebSocket
  ↓
WASM LiveNetworkClient receives trace events
  ↓
Leptos reactive signals update
  ↓
UI re-renders:
  - Event Log shows message sent/received
  - Network view animates message flow
  - State inspector updates if state changed
```

### Time Travel Debugging

```
User drags timeline scrubber to tick 1000
  ↓
Timeline component calls on_seek(1000)
  ↓
WASM SimulationRuntime.restore_checkpoint_at_tick(1000)
  ↓
Find nearest checkpoint ≤ 1000 (e.g., checkpoint at tick 900)
  ↓
Restore simulation state from checkpoint 900
  ↓
Replay trace events from 900 to 1000
  - Apply each TraceEvent in order
  - Update device states
  - Reconstruct network topology
  ↓
Return final state at tick 1000
  ↓
Leptos signals update with reconstructed state
  ↓
All views re-render to show state at tick 1000:
  - Network topology reflects connections at tick 1000
  - State inspector shows device state at tick 1000
  - Choreography view shows protocol states at tick 1000
```

## Operating Modes

### 1. Simulation Mode

**Purpose**: Deterministic testing and time travel debugging of choreographic protocols.

**Features**:
- Load scenario from TOML or design visually
- Step-by-step execution with pause/play controls
- Speed control (0.1x to 10x)
- Checkpoint creation and restoration
- Time travel to any historical tick
- Trace export for bug reports
- Property violation detection with automatic bisection

**Use Cases**:
- Validate protocol correctness
- Test Byzantine fault scenarios
- Reproduce bugs from CI failures
- Understand choreography execution
- Generate minimal reproductions

**UI Controls**:
```
┌─────────────────────────────────────────────────────┐
│ ▶ Play | ⏸ Pause | ⏭ Step | ⏮ Reset              │
│ Speed: [========●====] 2.5x                        │
│ Tick: 1247 / 5000                                  │
└─────────────────────────────────────────────────────┘
```

### 2. Live Network Mode

**Purpose**: Observe and interact with real Aura nodes running with full P2P networking.

**Features**:
- Connect to local Aura node's instrumentation API
- Observe real P2P network activity (SBB gossip, peer discovery)
- Send messages through node's real transport layer
- Initiate protocols (DKD, resharing, recovery)
- Query device state and ledger
- Observe protocol execution in real-time
- Export traces from live sessions

**Use Cases**:
- Integration testing across multiple devices
- Manual QA of new features
- Debugging issues in development environments
- Demonstrating protocol flows
- Performance profiling
- Observing multi-device choreographies

**Connection Flow**:
```
1. Start Aura node with: aura --dev-console --dev-console-port 9003
2. Node runs normally with full P2P transport (SBB, peer discovery)
3. Console connects to instrumentation API (ws://localhost:9003)
4. Console subscribes to node's trace event stream
5. UI displays live topology and message flow
6. User commands (send message, initiate protocol) execute through node's real APIs
7. Messages propagate via actual P2P network, not relay
```

**Key Distinction**: The console **observes** a real Aura node; it does not replace or wrap the P2P transport. All messages use SBB gossip, peer discovery, and the normal Aura transport layer.

### 3. Scenario Design Mode

**Purpose**: Visually design test scenarios and export as TOML.

**Features**:
- Drag-and-drop participant creation
- Configure thresholds, network conditions, Byzantine behaviors
- Define protocol sequences and assertions
- Live preview of scenario
- Export to TOML for use in automated tests
- Import existing scenarios for modification

**Use Cases**:
- Create new test cases without writing code
- Modify existing scenarios for regression tests
- Share scenario definitions with team
- Build scenario library for common patterns

**Workflow**:
```
1. Add participants (honest, Byzantine, offline)
2. Set threshold configuration (e.g., 2-of-3)
3. Configure network (latency, partitions, drop rate)
4. Add protocol steps (DKD, resharing, recovery)
5. Define assertions (state checks, property violations)
6. Preview simulation
7. Export as scenarios/my-test.toml
```

## Visualization Components

### Network Topology View

**Implementation**: Cytoscape.js with force-directed layout (cola algorithm)

**Features**:
- Nodes represent devices (honest, Byzantine, offline)
- Edges represent communication channels
- Color-coded by participant type and status
- Animated message flow along edges
- Partition visualization (separate clusters)
- Click node → inspect state
- Hover edge → see message details

**Visual Encoding**:
- Node color: Green (honest), Red (Byzantine), Gray (offline)
- Node size: Proportional to message activity
- Edge thickness: Message frequency
- Edge animation: Direction and timing of messages
- Dashed edges: Partitioned connections

### Timeline View

**Implementation**: Custom D3.js visualization

**Features**:
- Horizontal timeline with tick marks
- Swimlanes per participant
- Event markers color-coded by type
- Scrubber for time travel
- Zoom in/out for detail levels
- Bookmark important moments
- Jump to violations/checkpoints
- Range selection for trace export

**Visual Encoding**:
- Y-axis: Participant swimlanes
- X-axis: Simulation ticks
- Circle markers: Individual events
- Color: Event type (message sent, received, state transition)
- Size: Event importance/impact
- Vertical lines: Checkpoints
- Red highlights: Property violations

### State Inspector

**Implementation**: JSON tree view with custom rendering for Aura types

**Features**:
- Expandable/collapsible tree structure
- Syntax highlighting
- Type annotations
- Diff view between checkpoints
- Search and filter
- Copy to clipboard
- Redacted display for sensitive data (key shares)

**Sections**:
- Device metadata (ID, status, role)
- Ledger events (chronological log)
- CRDT state (Automerge document)
- Active protocols (session type states)
- Effect queue (pending side effects)
- Network buffers (inbound/outbound messages)

### Choreography Visualizer

**Implementation**: Custom state machine renderer using SVG

**Features**:
- Render session type state machines
- Current state highlighted
- Valid transitions shown
- Protocol-specific layouts
- Animate state transitions
- Show witness data at transitions
- Export as static diagram

**Protocol-Specific Views**:

**DKD Choreography**:
```
[Init] → [Commitment] → [Reveal] → [Finalize]
   ↓                                     ↓
[Abort] ← ← ← ← ← ← ← ← ← ← ← ← ← ← [Complete]
```

**FROST Signing**:
```
[Idle] → [Commitment Phase] → [Signing Phase] → [Aggregate] → [Complete]
```

**Recovery Flow**:
```
[Request] → [Guardian Approval] → [Cooldown] → [Share Submission] → [Resharing] → [Complete]
```

### Event Log

**Implementation**: Virtual scrolling list with filtering

**Features**:
- Real-time event stream
- Filter by participant, event type, protocol
- Search by content
- Auto-scroll toggle
- Export filtered view
- Syntax highlighting for message payloads

**Event Types**:
- Message sent/received/dropped
- Protocol state transitions
- CRDT merges
- Checkpoint creation
- Property violations
- Effect executions

## REPL Interface

The REPL provides a command-line interface for direct system control.

### Core Commands

```bash
# Branch management
branches                         # List all branches
checkout <branch>                # Switch to branch
fork [label]                     # Create interactive branch
commit <branch> as <scenario>    # Export branch as scenario
delete <branch>                  # Delete interactive branch

# Device management [read-only]
devices                          # List all devices
state <device_id>                # Show device state
ledger <device_id>               # Show ledger events

# Message operations [mutation - forks if on main]
send <to> <message>              # Send message to device
broadcast <message>              # Broadcast to all devices
inject <envelope_json>           # Inject raw envelope

# Protocol operations [mutation - forks if on main]
dkd <participants> <context>     # Initiate DKD
resharing <participants>         # Initiate resharing
recovery <guardians>             # Initiate recovery

# Network manipulation [mutation - forks if on main]
partition <devices>              # Create network partition
offline <device>                 # Take device offline
byzantine <device> <strategy>    # Enable Byzantine behavior

# Simulation control
step [n]                         # Step simulation n times (read-only)
run                              # Run until idle (read-only on main)
goto <tick>                      # Time travel to tick (read-only)
checkpoint [label]               # Create checkpoint
restore <label|tick>             # Restore checkpoint

# Analysis [read-only]
causality <event_id>             # Show causal chain
topology                         # Show network graph
trace [start] [end]              # Show trace segment
violations                       # List property violations

# Export
export trace <filename>          # Export full trace
export scenario <filename>       # Export current branch as scenario
export minimal <event_id>        # Export minimal reproduction

# Scenario management
load scenario <filename>         # Load scenario into main branch
load trace <filename>            # Load trace into main branch
```

### Example Session

```bash
>> devices
alice: online (honest)
bob: online (honest)
carol: online (byzantine)

>> state alice
DeviceState {
  id: "alice",
  threshold: ThresholdConfig { t: 2, n: 3 },
  session_epoch: 5,
  active_protocols: ["dkd-session-1"],
  ledger_heads: ["bafyrei..."],
}

>> dkd alice,bob,carol "app:wallet, ctx:eth"
Initiated DKD session dkd-session-1
Participants: alice, bob, carol

>> step 5
Tick 1: alice → commitment → bob, carol
Tick 2: bob → commitment → alice, carol
Tick 3: carol → invalid_commitment → alice, bob
Tick 4: alice → abort → bob
Tick 5: bob → abort_ack → alice

>> violations
PropertyViolation {
  tick: 3,
  property: "commitment_validity",
  participant: "carol",
  details: "Invalid signature on commitment message"
}

>> export minimal 3
Exported minimal reproduction to scenarios/violation-3.toml
```

## Interaction Model: Scenarios vs. Interactive Exploration

### The Branching Model

The console distinguishes between **scripted scenarios** (reproducible, deterministic) and **interactive exploration** (ad-hoc debugging). To preserve determinism while enabling exploration, the console uses a **branching model** similar to Git.

#### Core Concepts

**Main Branch (Scenario)**:
- Loaded from TOML file or designed in Scenario Designer
- Deterministic seed and scripted actions
- Reproducible across runs
- Immutable once loaded
- Forms the "source of truth" for test cases

**Interactive Branches**:
- Created when user issues REPL command that diverges from scenario
- Inherit state from parent branch at fork point
- Non-deterministic (user actions, different seed)
- Can be explored, modified, and discarded
- Can be "committed" to create new scenario

#### Branching Visualization

```
Timeline View with Branches:

Main (scenario: dkd-basic.toml, seed: 42)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━→
tick 0   tick 50   tick 100  tick 150  tick 200
  │        │          │         │         │
  ●────────●──────────●─────────●─────────● (checkpoints)
           │                    │
           │                    └─→ Interactive Branch 2 (seed: 789)
           │                        "What if alice goes offline?"
           │                        ┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈→
           │                        tick 150  tick 180
           │
           └─→ Interactive Branch 1 (seed: 456)
               "Injected malformed DKD commitment"
               ┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈→
               tick 50   tick 75   tick 100

Legend:
━━━ Main branch (scenario)
┈┈┈ Interactive branch
●   Checkpoint
```

### Interaction Rules

#### Rule 1: Scenarios Run on Main Branch

When a scenario is loaded and executed:
```bash
>> load scenario dkd-basic.toml
Loaded scenario: dkd-basic.toml (seed: 42)
Main branch active.

>> run
Running scenario on main branch...
[Tick 0-200 execute deterministically]
Scenario completed. Main branch preserved.
```

**Characteristics**:
- No user intervention during `run`
- Fully reproducible with same seed
- Can be re-run identically
- Forms baseline for comparison

#### Rule 2: REPL Commands Fork Automatically

Any REPL command that modifies simulation state creates a new branch:

```bash
>> step 50
Main branch: tick 0 → 50

>> inject alice malformed_dkd_commitment
[WARN]  This command will fork the simulation.
    Create interactive branch from tick 50? [y/n]: y

Created branch: interactive-1 (seed: 456, forked from main@tick50)
Switched to branch: interactive-1

>> step 10
Interactive-1: tick 50 → 60
[Malformed commitment causes protocol abort]

>> branches
* interactive-1 (current, forked from main@tick50)
  main (scenario: dkd-basic.toml)
```

**Commands that fork**:
- `inject <device> <message>` - Manual message injection
- `partition <devices>` - Network partition creation
- `offline <device>` - Take device offline
- `byzantine <device> <strategy>` - Enable Byzantine behavior
- Any command marked `[mutation]` in help text

**Commands that don't fork** (read-only):
- `devices`, `state`, `ledger` - Inspection
- `topology`, `causality` - Analysis
- `violations`, `trace` - Querying

#### Rule 3: Branch Switching

Users can switch between branches to explore different histories:

```bash
>> branches
  interactive-1 (forked from main@tick50)
  interactive-2 (forked from main@tick150)
* main (scenario: dkd-basic.toml)

>> checkout interactive-1
Switched to branch: interactive-1
Current tick: 60

>> step 20
Interactive-1: tick 60 → 80

>> checkout main
Switched to branch: main
Current tick: 200 (scenario completed)
```

**Branch switching**:
- Restores checkpoint at fork point
- Replays events on target branch
- Updates UI to reflect branch state
- Branch list always visible in UI sidebar

#### Rule 4: Committing Branches to Scenarios

Interactive branches can be exported as new scenarios:

```bash
>> checkout interactive-1
>> export scenario malformed-dkd-attack.toml
Exported interactive-1 as scenario: malformed-dkd-attack.toml

Changes:
  - Base: dkd-basic.toml (seed: 42)
  - Fork point: tick 50
  - Added: inject alice malformed_dkd_commitment at tick 50
  - Result: protocol abort at tick 55
  - New seed: 456 (for reproducibility)
```

**Exported scenario includes**:
- Original scenario as base
- Exact fork point
- All interactive commands as scripted actions
- New deterministic seed
- Expected outcome (if different from base)

#### Rule 5: Merging Insights

Branches can inform scenario improvements:

```bash
# Discovered interesting behavior in interactive-1
>> commit interactive-1 as test-case
Created new scenario: test-malformed-commitment.toml

# Now it's a reproducible test
>> load scenario test-malformed-commitment.toml
>> run
[Deterministically reproduces the discovered behavior]
```

### UI Presentation

#### Timeline with Branch Visualization

```
┌─────────────────────────────────────────────────────────┐
│ Timeline                        Branch: main ▼          │
├─────────────────────────────────────────────────────────┤
│                                                         │
│ Main ━━━━━━━━●━━━━━━━━●━━━━━━━━●━━━━━━━━━→              │
│      0       50      100      150      200              │
│               │                │                        │
│               │                └┈┈┈┈┈┈ Interactive-2    │
│               │                  150    180             │
│               │                                         │
│               └┈┈┈┈┈┈┈┈┈┈ Interactive-1                 │
│                 50   60   75  100                       │
│                                                         │
│ Current: Main @ tick 200                                │
│ [Switch Branch ▼] [New Branch] [Commit Branch]          │
└─────────────────────────────────────────────────────────┘
```

#### Branch Manager Sidebar

```
┌──────────────────────┐
│ Branches             │
├──────────────────────┤
│ ● main               │
│   dkd-basic.toml     │
│   seed: 42           │
│   tick: 200          │
│                      │
│ ○ interactive-1      │
│   forked: main@50    │
│   seed: 456          │
│   tick: 100          │
│   [Commit] [Delete]  │
│                      │
│ ○ interactive-2      │
│   forked: main@150   │
│   seed: 789          │
│   tick: 180          │
│   [Commit] [Delete]  │
│                      │
│ [+ New Branch]       │
└──────────────────────┘
```

### Implementation Details

#### Branch Storage

```rust
// Simulation Server
pub struct SimulationServer {
    branches: HashMap<BranchId, Branch>,
    current_branch: BranchId,
}

pub struct Branch {
    id: BranchId,
    name: String,
    parent: Option<(BranchId, u64)>,  // (parent_branch, fork_tick)
    scenario: Option<ScenarioConfig>,  // Present for main branch
    seed: u64,
    simulation: InstrumentedSimulation,
    checkpoints: BTreeMap<u64, SimulationCheckpoint>,
    interactive_commands: Vec<InteractiveCommand>,  // Commands that created this branch
}

pub struct InteractiveCommand {
    tick: u64,
    command: String,
    effect: CommandEffect,
}
```

#### REPL Fork Detection

```rust
impl SimulationServer {
    pub fn handle_repl_command(&mut self, cmd: ReplCommand) -> Result<Response> {
        match cmd {
            // Read-only commands - no fork
            ReplCommand::Devices | ReplCommand::State(_) | ReplCommand::Ledger(_) => {
                self.execute_readonly(cmd)
            }

            // Mutation commands - fork required
            ReplCommand::Inject { .. } | ReplCommand::Partition { .. } => {
                if self.current_branch().is_main() && !self.interactive_mode {
                    // Prompt user to fork
                    return Ok(Response::ForkRequired {
                        message: "This command will fork the simulation. Continue?",
                        command: cmd,
                    });
                } else {
                    // Already on interactive branch or user confirmed
                    self.execute_mutation(cmd)
                }
            }
        }
    }

    pub fn fork_branch(&mut self, from_branch: BranchId, at_tick: u64) -> BranchId {
        let parent = &self.branches[&from_branch];
        let fork_checkpoint = parent.checkpoints.get(&at_tick)
            .expect("Fork point must have checkpoint");

        let new_branch_id = BranchId::new();
        let new_seed = generate_branch_seed();

        let mut new_simulation = parent.simulation.clone();
        new_simulation.restore_checkpoint(fork_checkpoint);
        new_simulation.set_seed(new_seed);  // Different seed for divergent behavior

        let new_branch = Branch {
            id: new_branch_id,
            name: format!("interactive-{}", self.next_branch_number()),
            parent: Some((from_branch, at_tick)),
            scenario: None,  // Not from scenario
            seed: new_seed,
            simulation: new_simulation,
            checkpoints: BTreeMap::new(),
            interactive_commands: Vec::new(),
        };

        self.branches.insert(new_branch_id, new_branch);
        self.current_branch = new_branch_id;

        new_branch_id
    }
}
```

#### Scenario Export from Branch

```rust
impl Branch {
    pub fn export_as_scenario(&self, server: &SimulationServer) -> ScenarioConfig {
        let mut scenario = if let Some((parent_id, fork_tick)) = self.parent {
            // Base on parent scenario
            let parent = &server.branches[&parent_id];
            parent.scenario.clone().expect("Parent must have scenario")
        } else {
            // This is main branch, export as-is
            self.scenario.clone().expect("Main branch must have scenario")
        };

        // Add interactive commands as scripted actions
        for cmd in &self.interactive_commands {
            scenario.phases.push(ScenarioPhase {
                name: format!("Interactive: {}", cmd.command),
                checkpoint: Some(cmd.tick),
                actions: vec![cmd.to_scenario_action()],
            });
        }

        // Update seed for reproducibility
        scenario.setup.seed = Some(self.seed);
        scenario.metadata.generated_from_branch = Some(self.name.clone());

        scenario
    }
}
```

### Use Cases

#### Use Case 1: Exploring Alternative Outcomes

```bash
# Start with known-good scenario
>> load scenario dkd-basic.toml
>> run
[Scenario completes successfully]

# What if we introduce network partition?
>> goto 100
>> partition alice,bob
[Forks to interactive-1]
>> step 50
[Observe protocol behavior under partition]

# Interesting! Let's save this
>> export scenario dkd-with-partition.toml

# Switch back to main to continue original test
>> checkout main
```

#### Use Case 2: Debugging CI Failures

```bash
# Load trace from CI failure
>> load trace ci-failure-#1234.bin
Loaded trace into main branch (seed: 987654)

# Rewind to just before failure
>> goto 1450
>> violations
PropertyViolation at tick 1455: threshold_safety

# Try different intervention
>> checkpoint before-intervention
>> inject alice alternate_commitment
[Forks to interactive-1]
>> step 10
[Violation disappears!]

# Found the issue - export minimal reproduction
>> export minimal 1455
Exported: minimal-reproduction-1455.toml
Includes: interactive changes that prevented violation
```

#### Use Case 3: Scenario Refinement

```bash
# Start with rough scenario
>> load scenario rough-byzantine-test.toml
>> run
[Some parts work, some don't]

# Interactively fix the broken parts
>> goto 75
>> byzantine carol commit_equivocation
[Forks to interactive-1]
>> step 25
[Much better!]

# Commit refinements back
>> commit interactive-1 as improved-byzantine-test.toml
```

## Trace Format

The console uses a compact, queryable trace format for recording simulation and live network events.

### Trace Structure

```rust
pub struct SimulationTrace {
    pub metadata: TraceMetadata,
    pub timeline: Vec<TraceEvent>,
    pub checkpoints: Vec<CheckpointRef>,
    pub participants: HashMap<DeviceId, ParticipantInfo>,
    pub network_topology: NetworkTopology,
}

pub struct TraceMetadata {
    pub scenario_name: String,
    pub seed: u64,
    pub total_ticks: u64,
    pub properties_checked: Vec<String>,
    pub violations: Vec<PropertyViolation>,
}

pub struct TraceEvent {
    pub tick: u64,
    pub event_id: u64,
    pub event_type: EventType,
    pub participant: DeviceId,
    pub causality: CausalityInfo,
}

pub enum EventType {
    ProtocolStateTransition {
        protocol: ProtocolType,
        from_state: String,
        to_state: String,
        witness_data: Option<Vec<u8>>,
    },
    MessageSent {
        envelope_id: EnvelopeId,
        to: Vec<DeviceId>,
        message_type: String,
        size_bytes: usize,
    },
    MessageReceived {
        envelope_id: EnvelopeId,
        from: DeviceId,
        message_type: String,
    },
    MessageDropped {
        envelope_id: EnvelopeId,
        reason: DropReason,
    },
    EffectExecuted {
        effect_type: String,
        effect_data: Vec<u8>,
    },
    CrdtMerge {
        from_replica: DeviceId,
        heads_before: Vec<ChangeHash>,
        heads_after: Vec<ChangeHash>,
    },
    CheckpointCreated {
        checkpoint_id: String,
        label: String,
    },
    PropertyViolation {
        property: String,
        violation_details: String,
    },
}

pub struct CausalityInfo {
    pub parent_events: Vec<u64>,
    pub happens_before: Vec<u64>,
    pub concurrent_with: Vec<u64>,
}
```

### Trace Serialization

Traces are serialized using `postcard` for compact binary encoding:

```rust
// Export trace
let trace_bytes = postcard::to_allocvec(&trace)?;
std::fs::write("trace.bin", trace_bytes)?;

// Load trace in console
let trace_bytes = load_file("trace.bin");
let analyzer = TraceAnalyzer::new(&trace_bytes)?;
```

### Trace Querying

The analysis engine provides efficient querying:

```rust
// Get events in time range
let events = analyzer.get_events_in_range(100, 200);

// Get all events for a participant
let alice_events = analyzer.get_events_by_participant("alice");

// Get causal chain leading to event
let chain = analyzer.get_causality_path(event_id);

// Reconstruct state at tick
let state = analyzer.get_state_at_tick(150, "alice");

// Find pattern
let matches = analyzer.find_pattern(|e| {
    matches!(e.event_type, EventType::PropertyViolation { .. })
});
```

## Technology Stack

### Backend (Rust → WASM)

**Core Libraries**:
- `wasm-bindgen`: JavaScript/WASM interop
- `serde` + `serde-wasm-bindgen`: Serialization across boundary
- `postcard`: Compact binary trace format
- `petgraph`: Graph algorithms for causality analysis
- `web-sys`: Web API bindings (WebSocket, DOM)

**Aura Crates**:
- `aura-sim`: Instrumented simulation framework
- `aura-agent`: DeviceAgent for live mode
- `aura-coordination`: Protocol execution
- `aura-crypto`: FROST, DKD
- `aura-journal`: CRDT ledger
- `aura-transport`: P2P communication

**Build**:
```bash
cd crates/console-core
wasm-pack build --target web --out-dir ../../console/pkg
```

### Frontend (Leptos)

**Core Libraries**:
- `leptos`: Reactive UI framework (CSR mode)
- `web-sys`: Browser API access
- `wasm-bindgen`: Import console-core WASM

**Styling**:
- `stylance`: Compile-time scoped CSS modules (zero runtime overhead)
- `phosphor-leptos`: Icon library (modern, technical aesthetic)

**Visualization Libraries**:
- `cytoscape.js`: Network topology graphs
- `d3.js`: Timeline and custom visualizations
- `monaco-editor`: REPL and code editing

**Build**:
```bash
cd console
trunk serve        # Development with hot reload
trunk build --release  # Production bundle
```

**Bundle Size (gzipped)**:
- Leptos UI: ~50-100KB
- Stylance CSS: ~10-20KB (compiled to plain CSS, no runtime)
- Phosphor icons: ~5-10KB (only icons used)
- Mode-specific WASM clients: ~100-200KB each (loaded on demand)
- Total first load: ~165-330KB (UI + one WASM client)
- Mode switching: Additional ~100-200KB (lazy load new client)

Note: Simulation/Analysis servers run natively, not in browser, so no large WASM bundle.

### Styling

**Approach**: Stylance CSS Modules with compile-time scoping

Stylance provides type-safe, scoped CSS modules compiled at build time. Each component has its own `.module.css` file that is imported and scoped automatically.

**Benefits**:
- **Zero runtime overhead**: All CSS processing happens at compile time
- **Type-safe class names**: CSS classes are Rust identifiers with autocomplete
- **Scoped by default**: No naming conflicts between components
- **Standard CSS**: No new syntax, works with CSS variables
- **Component co-location**: Each component's styles live next to its code

**Example**:
```rust
// timeline.rs
use leptos::*;
use stylance::import_style;
use phosphor_leptos::{Play, Pause, IconWeight};

import_style!(style, "timeline.module.css");

#[component]
pub fn Timeline() -> impl IntoView {
    view! {
        <div class=style::container>
            <div class=style::controls>
                <button class=style::button>
                    <Play weight=IconWeight::Bold />
                    "Play"
                </button>
            </div>
        </div>
    }
}
```

```css
/* timeline.module.css */
.container {
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    padding: var(--spacing-md);
}

.controls {
    display: flex;
    gap: var(--spacing-sm);
}

.button {
    display: flex;
    align-items: center;
    gap: var(--spacing-xs);
    padding: var(--spacing-sm) var(--spacing-md);
    background: var(--bg-tertiary);
    color: var(--text-primary);
    cursor: pointer;
    transition: all 0.15s ease;
}

.button:hover {
    background: var(--accent);
}
```

**Theme Variables** (global.css):
```css
:root {
    /* Colors */
    --bg-primary: #1e1e1e;
    --bg-secondary: #252525;
    --bg-tertiary: #2d2d2d;
    --text-primary: #e0e0e0;
    --text-secondary: #a0a0a0;
    --accent: #007acc;
    --accent-hover: #005a9e;
    --success: #4caf50;
    --error: #f44336;
    --warning: #ff9800;
    --border: #3e3e3e;
    
    /* Typography */
    --font-mono: 'JetBrains Mono', 'Fira Code', monospace;
    --font-sans: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
    
    /* Spacing */
    --spacing-xs: 4px;
    --spacing-sm: 8px;
    --spacing-md: 16px;
    --spacing-lg: 24px;
    --spacing-xl: 32px;
}

body {
    font-family: var(--font-sans);
    background: var(--bg-primary);
    color: var(--text-primary);
    font-size: 14px;
}
```

**Design Principles**:
- Dark theme optimized for extended use
- High contrast for accessibility
- Minimal animations (performance)
- Responsive layout (grid-based)
- Monospace fonts for technical data
- Component-scoped styles prevent conflicts

## Project Structure

```
aura/
├── crates/
│   ├── console-core/              # Console backend (Rust → WASM)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs             # WASM entry point
│   │       ├── simulation_runtime.rs
│   │       ├── live_client.rs
│   │       ├── analyzer.rs
│   │       ├── causality.rs
│   │       ├── repl.rs
│   │       └── scenario_engine.rs
│   │
│   └── console-types/             # Shared types (no dependencies)
│       ├── Cargo.toml
│       └── src/
│           ├── trace.rs
│           ├── messages.rs
│           └── commands.rs
│
└── console/                       # Dev Console UI (Leptos)
    ├── Cargo.toml
    ├── index.html
    ├── Trunk.toml
    │
    ├── src/
    │   ├── main.rs                # App entry point
    │   ├── app.rs                 # Main app with mode switching
    │   │
    │   ├── modes/
    │   │   ├── simulate.rs        # Simulation mode
    │   │   ├── simulate.module.css
    │   │   ├── live.rs            # Live network mode
    │   │   ├── live.module.css
    │   │   ├── design.rs          # Scenario designer
    │   │   └── design.module.css
    │   │
    │   ├── components/
    │   │   ├── network_view.rs    # Cytoscape.js wrapper
    │   │   ├── network_view.module.css
    │   │   ├── timeline.rs        # D3.js timeline
    │   │   ├── timeline.module.css
    │   │   ├── state_inspector.rs # JSON tree view
    │   │   ├── state_inspector.module.css
    │   │   ├── message_composer.rs
    │   │   ├── message_composer.module.css
    │   │   ├── protocol_stepper.rs
    │   │   ├── protocol_stepper.module.css
    │   │   ├── repl.rs            # REPL interface
    │   │   ├── repl.module.css
    │   │   ├── event_log.rs       # Event stream
    │   │   ├── event_log.module.css
    │   │   ├── choreography.rs    # State machine viz
    │   │   └── choreography.module.css
    │   │
    │   └── bridge/
    │       ├── simulation.rs      # Bridge to simulation runtime
    │       └── live.rs            # Bridge to live network
    │
    ├── styles/
    │   ├── global.css             # CSS variables, theme, reset
    │   └── fonts/                 # Font files (optional)
    │
    ├── static/
    │   ├── scenarios/             # Example TOML scenarios
    │   │   ├── dkd-basic.toml
    │   │   ├── byzantine-resharing.toml
    │   │   └── recovery-flow.toml
    │   └── traces/                # Example traces
    │       └── violation-example.bin
    │
    └── pkg/                       # Generated WASM bindings
        ├── console_core.js
        └── console_core_bg.wasm
```

## Development Workflow

### Local Development

```bash
# Terminal 1: Watch and rebuild WASM on changes
cd crates/console-core
cargo watch -x 'build --target wasm32-unknown-unknown'

# Terminal 2: Dev server with hot reload
cd console
trunk serve

# Open browser to http://localhost:8080
```

### Production Build

```bash
# Build optimized WASM
cd crates/console-core
wasm-pack build --target web --release --out-dir ../../console/pkg

# Build optimized UI
cd ../../console
trunk build --release

# Output in console/dist/
# - index.html
# - app.js (Leptos UI)
# - app.wasm (Leptos UI)
# - pkg/console_core_bg.wasm (Full Aura)
```

### Deployment

The console is a static site that can be deployed anywhere:

```bash
# Deploy to Vercel/Netlify
vercel deploy console/dist

# Or serve locally
python -m http.server -d console/dist 8080

# Or package as Electron app for desktop
# (see electron-builder configuration)
```

## Integration with Testing

### Exporting Traces from CI

```rust
// In automated tests
#[test]
fn test_byzantine_resharing() {
    let mut sim = Simulation::new(Seed::from_u64(42));
    // ... run scenario ...

    if test_failed {
        let trace = sim.export_trace();
        std::fs::write("ci-artifacts/trace.bin", postcard::to_allocvec(&trace).unwrap()).unwrap();
    }
}
```

### Loading CI Traces in Console

1. Download trace artifact from CI
2. Open dev console
3. Drag and drop trace.bin file
4. Console loads simulation at final state
5. Use time travel to debug failure
6. Export minimal reproduction

### Generating Test Cases from Console

1. Design scenario in Design Mode
2. Run in Simulation Mode
3. Verify behavior
4. Export as TOML
5. Add to `scenarios/` directory
6. CI automatically runs all scenarios

## Security Considerations

### Sensitive Data Handling

The console may display sensitive data during development. Protections:

- **Key share redaction**: Private key material is masked in UI by default
- **Encrypted trace export**: Traces can be encrypted before export
- **Session isolation**: Each console instance uses ephemeral credentials
- **No persistence**: Console does not store data in browser storage by default

### Live Network Mode

When connecting to live networks:

- Use ephemeral DeviceAgent (no persistent identity)
- Connect over TLS WebSocket (wss://)
- Validate all signatures on incoming messages
- Rate limit outgoing messages to prevent abuse
- Clear sensitive data on disconnect

## Future Enhancements

### Phase 1 (Immediate)
- Basic simulation mode with timeline
- Network topology view
- State inspector
- Simple REPL

### Phase 2 (Short-term)
- Live network mode
- Message composer
- Protocol stepper
- Trace export/import

### Phase 3 (Medium-term)
- Scenario design mode
- Choreography visualizer
- Advanced REPL commands
- Causality path highlighting

### Phase 4 (Long-term)
- Collaborative debugging (shared sessions)
- AI-assisted debugging (pattern recognition)
- Performance profiling
- Automated test generation from traces
- Integration with Quint formal verification

## Related Documents

- `006_simulation_engine_using_injected_effects.md`: Simulation architecture
- `080_quint_driven_chaos_testing.md`: Chaos testing and time travel debugging
- `100_implementation_plan.md`: Development roadmap

## Conclusion

The Aura Dev Console is the primary interface for understanding, testing, and debugging the Aura distributed system. By combining simulation, live network interaction, and powerful visualization in a lightweight browser-based tool, it provides developers with unprecedented visibility into choreographic protocol execution.

The architecture leverages Rust's type safety and performance throughout, compiling both the simulation engine and UI framework to WebAssembly for a portable, fast, and unified development experience.
