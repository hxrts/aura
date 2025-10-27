# Aura Dev Console Architecture

This document provides technical details about the Aura Dev Console architecture, implementation decisions, and development patterns.

## System Overview

The Aura Dev Console is a browser-based development tool that follows a client-server architecture with clear separation of concerns:

- **Simulation Server** (Rust): Runs full Aura simulations with deterministic execution
- **Instrumentation Server** (Rust): Provides real-time observability for live nodes
- **Browser Client** (Leptos/WASM): Reactive UI for visualization and interaction
- **WebSocket Protocol**: Real-time bidirectional communication

## Architecture Principles

### 1. Thin Client, Heavy Server

**Rationale**: Complex protocol logic runs in native Rust for performance and determinism, while the browser focuses solely on presentation and interaction.

**Benefits**:
- **Performance**: Simulations run at native speed
- **Determinism**: Exact reproducibility across different environments
- **Security**: No sensitive protocol logic exposed to browser
- **Compatibility**: Works across different browser environments

### 2. Reactive UI with WebSocket Integration

**Technology Stack**:
- **Leptos 0.7**: Rust-based reactive framework compiled to WebAssembly
- **wasm-core**: Shared WebSocket infrastructure across all WASM clients
- **Trunk**: Build system for WASM applications

**Benefits**:
- **Type Safety**: Full Rust type system in both frontend and backend
- **Performance**: WebAssembly performance with reactive updates
- **Consistency**: Shared types and error handling across client-server boundary

### 3. Three-Mode Integration

Each mode serves distinct use cases while sharing common infrastructure:

#### Simulation Mode
- **Server**: `crates/sim-server` - Deterministic protocol simulation
- **Protocol**: WebSocket with scenario loading and control commands
- **Features**: Time travel, branching, scenario replay

#### Live Mode
- **Server**: Instrumentation hooks in `crates/agent`
- **Protocol**: WebSocket with observation and limited control
- **Features**: Real-time monitoring, topology visualization

#### Analysis Mode
- **Server**: `crates/analysis-client` (WASM) - Client-side trace processing
- **Data Source**: Exported traces from simulation or live capture
- **Features**: Causality analysis, state inspection, query interface

## Component Architecture

### Frontend Components (Leptos)

```
console/src/app/
├── mod.rs                    # Main app shell and mode switching
├── components/
│   ├── timeline.rs           # Event timeline with D3.js integration
│   ├── network_view.rs       # Network topology with Cytoscape.js
│   ├── state_inspector.rs    # JSON tree viewer with search/filter
│   ├── repl.rs              # Command-line interface
│   └── branch_manager.rs     # Git-like branch management
└── services/
    └── websocket_foundation.rs # WebSocket service using wasm-core
```

#### Design Patterns

1. **Component Composition**: Small, focused components with clear responsibilities
2. **Context Providers**: Global state management through Leptos context
3. **Reactive Signals**: Event-driven updates without manual state synchronization
4. **JavaScript Interop**: Minimal, well-defined boundaries with D3.js and Cytoscape.js

### Backend Services

#### Simulation Server (`crates/sim-server`)

```rust
pub struct SimulationServer {
    branches: HashMap<BranchId, Branch>,
    current_branch: BranchId,
    websocket_server: WebSocketServer,
}

pub struct Branch {
    id: BranchId,
    simulation: InstrumentedSimulation,
    checkpoints: BTreeMap<u64, SimulationCheckpoint>,
    interactive_commands: Vec<InteractiveCommand>,
}
```

**Key Features**:
- **Branch Management**: Git-like workflow for experimental simulation paths
- **Deterministic Execution**: Reproducible results with controlled randomness
- **Time Travel**: Seek to any point in execution history
- **Interactive Commands**: Real-time protocol manipulation

#### Instrumentation Server (`crates/agent/src/infrastructure/instrumentation`)

```rust
pub struct InstrumentationServer {
    agent: Arc<IntegratedAgent>,
    trace_recorder: Arc<Mutex<TraceRecorder>>,
    connections: Arc<Mutex<HashMap<Uuid, WebSocketConnection>>>,
}
```

**Key Features**:
- **Non-Intrusive Observation**: Tap into existing agent APIs without modification
- **Real-Time Streaming**: Live event feed to connected clients
- **Command Injection**: Limited control for debugging and testing
- **Multi-Client Support**: Multiple console instances can connect simultaneously

### WebSocket Protocol

The console uses a unified WebSocket protocol based on `MessageEnvelope` from `wasm-core`:

```rust
#[derive(Serialize, Deserialize)]
pub struct MessageEnvelope {
    pub message_type: String,
    pub payload: serde_json::Value,
    pub timestamp: Option<u64>,
    pub client_id: Option<String>,
}
```

#### Message Types

**Simulation Server**:
- `scenario_load` - Load TOML scenario file
- `simulation_control` - Step, run, reset, seek commands
- `branch_operation` - Fork, commit, checkout, delete
- `state_query` - Request device or global state
- `event_stream` - Real-time event notifications

**Instrumentation Server**:
- `console_command` - State queries, network inspection
- `trace_event` - Real-time protocol events
- `command_response` - Response to console commands
- `connection_status` - Server status updates

## State Management

### Frontend State (Leptos Signals)

```rust
// Global application state
let (current_mode, set_current_mode) = signal(ConsoleMode::Simulation);
let (connection_state, set_connection_state) = signal(ConnectionState::Disconnected);

// WebSocket communication
let (events, set_events) = signal(VecDeque::<MessageEnvelope>::new());
let (responses, set_responses) = signal(VecDeque::<serde_json::Value>::new());

// Component-specific state
let (timeline_events, set_timeline_events) = signal(Vec::<TimelineEvent>::new());
let (network_topology, set_network_topology) = signal(NetworkTopology::empty());
```

**Design Principles**:
- **Reactive Updates**: State changes trigger automatic UI updates
- **Unidirectional Flow**: Clear data flow from WebSocket → state → UI
- **Context Sharing**: Global state accessible throughout component tree
- **Minimal State**: Only store what's necessary for UI rendering

### Backend State Management

#### Simulation State
- **Deterministic**: All state changes driven by explicit events
- **Checkpointing**: Efficient state snapshots for time travel
- **Branching**: Copy-on-write semantics for experimental paths
- **Serializable**: Full state can be exported/imported

#### Live Node State
- **Read-Only Observation**: No state modification through console
- **Event Streaming**: Changes pushed to clients in real-time
- **Sampling**: Configurable event filtering for performance
- **Privacy**: Sensitive data excluded from observation

## Performance Considerations

### Frontend Performance

1. **WASM Compilation**: Rust compiled to WebAssembly for near-native performance
2. **Event Batching**: Multiple WebSocket messages batched for efficient updates
3. **Virtual Scrolling**: Large event lists rendered on-demand
4. **Lazy Loading**: Components rendered only when visible

### Backend Performance

1. **Native Rust**: Full simulation performance without virtualization overhead
2. **Efficient Serialization**: `postcard` for compact binary traces
3. **Streaming**: Large datasets streamed rather than loaded entirely
4. **Connection Pooling**: Efficient WebSocket connection management

### Network Performance

1. **Compression**: WebSocket messages compressed when beneficial
2. **Selective Updates**: Only changed state transmitted
3. **Heartbeat Protocol**: Connection health monitoring
4. **Reconnection Logic**: Automatic recovery from network issues

## Security Model

### Threat Model

**Assumptions**:
- Development environment with trusted participants
- Network may be untrusted (use appropriate WebSocket security)
- Console should not compromise live node security

**Attack Scenarios**:
- **Malicious Scenarios**: Scenarios could contain harmful instructions
- **Network Eavesdropping**: WebSocket traffic should be encrypted
- **Code Injection**: JavaScript interop must be sanitized
- **Resource Exhaustion**: Large simulations or traces could consume excessive resources

### Security Measures

1. **Scenario Validation**: TOML parsing with strict schema validation
2. **Sandboxing**: Simulation runs in isolated environment
3. **Access Controls**: Live mode requires explicit opt-in via CLI flag
4. **Resource Limits**: Configurable limits on simulation size and duration
5. **Input Sanitization**: All user inputs validated before processing

### Production Guidelines

- **Never use with production keys or sensitive data**
- **Use TLS for WebSocket connections in production environments**
- **Restrict access to instrumentation endpoints**
- **Monitor resource usage to prevent DoS**

## Development Patterns

### Adding New Components

1. **Create component file** in `console/src/app/components/`
2. **Define props interface** with clear types
3. **Use reactive signals** for state management
4. **Add to module exports** in `components/mod.rs`
5. **Integrate with app shell** in `mod.rs`

Example component structure:
```rust
use leptos::prelude::*;

#[component]
pub fn MyComponent(
    #[prop(into)] data: ReadSignal<MyData>,
    #[prop(optional)] on_change: Option<impl Fn(MyEvent) + 'static>,
) -> impl IntoView {
    let (local_state, set_local_state) = signal(InitialValue);

    // Effects for reactive updates
    Effect::new(move |_| {
        // React to prop changes
    });

    view! {
        <div class="my-component">
            // Component template
        </div>
    }
}
```

### Adding New REPL Commands

1. **Define command** in `ConsoleCommand` enum
2. **Add parsing** in `parse_command()` function
3. **Implement handler** in appropriate server
4. **Add help text** in `help` command output
5. **Add autocomplete** support

### Adding New Event Types

1. **Define event** in appropriate crate (sim, agent, etc.)
2. **Add serialization** support with serde
3. **Update timeline rendering** for new event type
4. **Add to scenario format** documentation
5. **Include in example scenarios**

## Testing Strategy

### Frontend Testing

- **Unit Tests**: Component logic and state management
- **Integration Tests**: WebSocket communication
- **Visual Tests**: Component rendering and interactions
- **Performance Tests**: Large dataset handling

### Backend Testing

- **Simulation Tests**: Scenario execution and validation
- **Protocol Tests**: WebSocket message handling
- **Performance Tests**: Large simulation benchmarks
- **Security Tests**: Input validation and error handling

### End-to-End Testing

- **Scenario Validation**: All example scenarios execute successfully
- **Mode Switching**: Seamless transitions between modes
- **Error Recovery**: Graceful handling of connection failures
- **Cross-Browser**: Compatibility across major browsers

## Future Enhancements

### Short Term

1. **Enhanced Visualization**: More sophisticated timeline and network views
2. **Query Language**: SQL-like interface for trace analysis
3. **Performance Profiling**: Built-in performance analysis tools
4. **Collaborative Features**: Multi-user scenario development

### Long Term

1. **Plugin Architecture**: Third-party visualization components
2. **Cloud Integration**: Remote scenario execution and sharing
3. **AI-Assisted Testing**: Automated test case generation
4. **Mobile Support**: Responsive design for mobile devices

## Contributing

### Code Style

- Follow Rust standard formatting (`cargo fmt`)
- Use clear, descriptive names
- Prefer composition over inheritance
- Write self-documenting code
- Include comprehensive error handling

### Pull Request Process

1. **Create issue** describing the change
2. **Write tests** for new functionality
3. **Update documentation** as needed
4. **Test across browsers** for frontend changes
5. **Submit PR** with clear description

### Development Environment

```bash
# Set up development environment
nix develop

# Start development server
cd console
trunk serve

# Run tests (when implemented)
just test

# Check formatting and lints
just fmt
just clippy
```

For detailed development guidelines, see the main project README and CLAUDE.md files.
