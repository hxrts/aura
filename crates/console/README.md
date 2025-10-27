# Aura Dev Console

A browser-based developer interface for simulating, debugging, and visualizing Aura's choreographic protocols. The console provides three integrated modes for comprehensive protocol development and testing.

## Quick Start

### Prerequisites

- Nix with flakes enabled (required for development environment)
- Modern web browser (Chrome, Firefox, Safari, Edge)

### Setup

1. **Enter the development environment**:
   ```bash
   cd /path/to/aura
   nix develop  # or use direnv: echo "use flake" > .envrc && direnv allow
   ```

2. **Start the console**:
   ```bash
   cd console
   trunk serve
   ```

3. **Open in browser**:
   Navigate to `http://localhost:8080`

### First Steps

1. **Explore Simulation Mode**: Load an example scenario from `../scenarios/`
2. **Try the REPL**: Type `help` to see available commands
3. **Use the Timeline**: Click and drag to explore event sequences
4. **Inspect Network State**: Switch to Live mode to see topology visualization

## Architecture Overview

The Aura Dev Console uses a **client-server architecture** designed for performance and flexibility:

### Core Design Principles

1. **Simulation runs server-side**: Full Aura stack in native Rust for performance
2. **Browser is thin client**: Only visualization and WebSocket communication  
3. **P2P preserved in live mode**: Instrumentation doesn't replace transport
4. **Branching model**: Git-like workflow for scenario vs. interactive testing

### Three Integrated Modes

#### [game] Simulation Mode
- **Purpose**: Interactive protocol development and testing
- **Features**: Time travel, branching, scenario replay
- **Use Cases**: Protocol design, edge case testing, educational demos

#### [network] Live Mode  
- **Purpose**: Real-time monitoring of live Aura nodes
- **Features**: Network topology, real-time events, command injection
- **Use Cases**: Production debugging, performance analysis, incident response

#### [search] Analysis Mode
- **Purpose**: Post-hoc analysis of execution traces
- **Features**: Causality graphs, state inspection, trace queries
- **Use Cases**: Bug investigation, performance optimization, audit trails

## User Interface Components

### Header & Navigation
- **Mode Switcher**: Toggle between Simulation, Live, and Analysis modes
- **Connection Status**: WebSocket connection indicator
- **Account Info**: Current user and permissions (live mode only)

### Timeline View (Simulation/Analysis)
- **Event Visualization**: Chronological event display with D3.js
- **Time Travel**: Seek to any point in execution history
- **Event Details**: Click events for detailed inspection
- **Zoom & Pan**: Navigate long execution traces

### Network Topology (Live Mode)
- **Force-Directed Layout**: Interactive network graph with Cytoscape.js
- **Node Types**: Visual distinction for honest/Byzantine/observer nodes
- **Connection Health**: Link quality and latency visualization
- **Interactive Selection**: Click nodes for detailed state inspection

### State Inspector (All Modes)
- **JSON Tree Viewer**: Hierarchical state exploration
- **Search & Filter**: Find specific state properties
- **Diff View**: Compare states between timepoints
- **Export**: Copy state data for external analysis

### REPL Console (Simulation Mode)
- **Interactive Commands**: Direct protocol manipulation
- **Command History**: Up/down arrow navigation
- **Autocomplete**: Tab completion for commands and parameters
- **Branch Management**: Fork, commit, and switch between branches

### Branch Manager (Simulation Mode)
- **Visual Branch Tree**: Git-like branch visualization
- **Scenario Export**: Convert interactive sessions to repeatable scenarios
- **Commit History**: Track experimental changes
- **Branch Switching**: Seamlessly move between different experimental paths

## Command Reference

### REPL Commands

#### Simulation Control
```bash
step [n]              # Advance simulation by n ticks (default: 1)
run                   # Run until idle or end of scenario
reset                 # Reset simulation to beginning
seek <tick>           # Jump to specific tick
```

#### State Inspection
```bash
devices               # List all devices and their status
state <device>        # Show complete device state
ledger [device]       # Show ledger state (global or device-specific)
network               # Show network topology and connections
events [device]       # Show recent events (global or device-specific)
```

#### Interactive Manipulation
```bash
inject <to> <message> # Send message to specific device
partition <devices>   # Create network partition between devices
byzantine <device>    # Make device exhibit Byzantine behavior
crash <device>        # Crash device (simulate failure)
recover <device>      # Recover crashed device
```

#### Branch Management
```bash
branches              # List all branches with status
fork [name]           # Create new branch from current state
checkout <branch>     # Switch to specified branch
commit <name>         # Save current branch as named scenario
export <file>         # Export current branch as TOML file
```

#### Utilities
```bash
help                  # Show detailed command help
clear                 # Clear console output
status                # Show simulation status and metrics
```

### Live Mode Commands

When connected to a live Aura node:

```bash
# Network Inspection
peers                 # List connected peers
topology              # Show network graph
latency <peer>        # Measure latency to peer

# State Queries  
account <device>      # Show account state
keys <device>         # Show key information
storage <device>      # Show storage state

# Administrative
recording on|off      # Enable/disable event recording
subscribe <events>    # Subscribe to specific event types
unsubscribe          # Stop event subscription
```

## Scenario Development

### Scenario File Format

Scenarios are defined in TOML format with clear structure:

```toml
[metadata]
name = "Scenario Name"
description = "What this demonstrates"
difficulty = "beginner|intermediate|advanced"
estimated_runtime = "30s"

[participants]
alice = { role = "honest", stake = 1000 }
bob = { role = "byzantine", stake = 800 }

[threshold_config]
threshold = 2
total_participants = 3

[[phases]]
name = "initialization"
description = "Set up participants"
duration = "10s"

  [[phases.events]]
  type = "threshold_key_generation"
  participants = ["alice", "bob"]
  threshold = 2
  tick = 1

[assertions]
# Properties that must hold
key_consistency = "All participants derive identical keys"

[expected_outcomes]
# Quantitative validation metrics
successful_derivations = 1
```

### Creating Scenarios

1. **Interactive Development**:
   ```bash
   # Start with empty simulation
   reset
   
   # Add participants interactively
   add participant alice honest
   add participant bob honest
   
   # Execute commands
   threshold_setup 2 3
   dkd alice storage_app user_docs
   
   # Export when satisfied
   export my-scenario.toml
   ```

2. **Template-Based**:
   - Copy existing scenario from `../scenarios/`
   - Modify participants, events, and timing
   - Test in console before committing

3. **From Trace Data**:
   - Load execution trace in Analysis mode
   - Use "Export as Scenario" feature
   - Edit exported TOML for reusability

### Best Practices

- **Incremental Complexity**: Start simple, add complexity gradually
- **Clear Naming**: Use descriptive names for participants and phases
- **Realistic Timing**: Space events appropriately for readability
- **Comprehensive Testing**: Validate all assertions and outcomes
- **Good Documentation**: Include comments explaining complex behaviors

## Integration with Development Workflow

### Testing Protocol Changes

1. **Unit Testing**: Run existing scenarios against modified protocols
2. **Regression Testing**: Ensure changes don't break existing functionality  
3. **Edge Case Exploration**: Use interactive mode to find corner cases
4. **Performance Analysis**: Use Analysis mode to identify bottlenecks

### Debugging Production Issues

1. **Connect to Live Node**:
   ```bash
   aura node --dev-console --dev-console-port 9003
   ```

2. **Monitor in Real-Time**:
   - Switch console to Live mode
   - Connect to `ws://localhost:9003/ws`
   - Watch network topology and event stream

3. **Capture Traces**:
   - Enable recording with `recording on`
   - Reproduce issue scenario
   - Export trace for offline analysis

4. **Analyze Offline**:
   - Switch to Analysis mode
   - Load captured trace
   - Use causality tools to understand issue

### Educational Use

- **Protocol Learning**: Start with basic scenarios like `dkd-basic.toml`
- **Security Understanding**: Explore Byzantine scenarios
- **Recovery Training**: Practice with `recovery-flow.toml`
- **Network Resilience**: Study `network-partition.toml`

## Advanced Features

### Custom Event Types

Define application-specific events for domain modeling:

```toml
[[phases.events]]
type = "custom_application_event"
app_specific_field = "value"
custom_data = { key = "value", nested = { data = true } }
tick = 15
```

### Time-Based Assertions

Validate timing properties:

```toml
[assertions]
response_time = "DKD requests complete within 5 ticks"
liveness = "System makes progress every 10 ticks"
```

### Complex Network Topologies

Model realistic network conditions:

```toml
[network_conditions]
baseline_latency = "50ms"
partition_probability = 0.1
byzantine_rate = 0.05

[[network_conditions.latency_matrix]]
from = "us-east"
to = "eu-west"  
latency = "100ms"
jitter = "10ms"
```

### Causality Analysis

Explore event relationships:

```bash
# Find causal dependencies
causality_path event_123
causality_graph --filter=dkd_events
happens_before event_456 event_789
```

## Troubleshooting

### Common Issues

#### Scenario Won't Load
- **Check TOML syntax**: Use online TOML validator
- **Verify participant consistency**: Names must match across sections
- **Check required fields**: All metadata fields are mandatory

#### WebSocket Connection Fails
- **Verify server running**: Check `aura node --dev-console` is active
- **Check port availability**: Default is 9003, may need to change
- **Firewall settings**: Ensure WebSocket port is open

#### Timeline Performance Issues
- **Large traces**: Use filtering to reduce displayed events
- **Browser memory**: Refresh page to clear accumulated state
- **Hardware limits**: Consider trace sampling for very long runs

#### REPL Commands Not Working
- **Check mode**: Some commands only work in specific modes
- **Syntax errors**: Use `help <command>` for correct syntax
- **State requirements**: Some commands need specific simulation state

### Getting Help

1. **Console Help**: Type `help` in REPL for interactive assistance
2. **Documentation**: Check `docs/110_dev_console_architecture.md`
3. **Examples**: Study scenarios in `../scenarios/`
4. **Issues**: Report bugs to Aura development team

### Performance Optimization

#### Large Simulations
- **Event filtering**: Use selective event display
- **Checkpoint usage**: Create checkpoints for long simulations
- **Branch pruning**: Delete unused experimental branches

#### Network Visualization
- **Node limiting**: Hide inactive nodes for clarity
- **Layout algorithms**: Switch between force-directed and hierarchical
- **Rendering optimization**: Disable animations for better performance

## Development & Contributing

### Console Development

The console itself is built with:
- **Frontend**: Leptos 0.7 (Rust + WebAssembly)
- **Styling**: CSS with custom properties for theming
- **Visualization**: D3.js for timeline, Cytoscape.js for networks
- **Build System**: Trunk for WASM compilation and serving

### Building from Source

```bash
# Development build
cd console
trunk serve

# Production build  
trunk build --release

# Testing
# TODO: Add test commands when implemented
```

### Contributing Scenarios

1. **Create scenarios** following the format in `../scenarios/`
2. **Test thoroughly** in the dev console
3. **Add documentation** with clear use cases
4. **Submit pull request** with scenario and tests

### Code Quality Standards

- **Clean, elegant code**: Follow project quality standards
- **No legacy code**: Modern patterns only
- **Self-documenting**: Clear names and structure
- **Minimal cognitive load**: Code should be immediately understandable

## License

This software is part of the Aura project and is licensed under MIT OR Apache-2.0.

## Security Notice

The dev console is intended for development and testing only. Do not use with production keys or sensitive data. When connecting to live nodes, ensure appropriate network security and access controls.