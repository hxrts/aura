# App Simulation Client WASM Module

WebAssembly client library for browser-to-simulation-server communication in the App Dev Console.

## Features

- **WebSocket Client**: Efficient connection to simulation server
- **Event Streaming**: Real-time trace event reception with buffering
- **Command Interface**: Send commands to simulation server
- **Optimized Bundle**: ~100-150KB gzipped WASM module
- **TypeScript Support**: Complete type definitions included

## Building

```bash
# Install wasm-pack if not already installed
cargo install wasm-pack

# Build optimized WASM module
./build.sh
```

## Usage

### Basic Connection

```typescript
import init, { SimulationClient } from './pkg/app_sim_client.js';

// Initialize WASM module
await init();

// Create client
const client = new SimulationClient('ws://localhost:9001');

// Set up event handling
client.set_event_callback((event) => {
  console.log('Received event:', event);
});

client.set_connection_callback((connected) => {
  console.log('Connection status:', connected);
});

// Connect to server
await client.connect();
```

### Sending Commands

```typescript
// Send a step command
const stepCommand = {
  Step: { count: 5 }
};

await client.send_command(stepCommand, (response) => {
  console.log('Command response:', response);
});
```

### Event Subscription

```typescript
// Subscribe to trace events
await client.subscribe(['TraceEvent', 'StateChange']);

// Get buffered events
const events = client.get_events_since(lastEventId);
console.log('Recent events:', events);
```

### Branch Export

```typescript
// Export current branch as scenario
const exportCommand = {
  ExportScenario: {
    branch_id: 'current-branch-id',
    filename: 'my-scenario.toml'
  }
};

await client.send_command(exportCommand, (response) => {
  if (response.ExportScenario) {
    console.log('Exported TOML:', response.ExportScenario.toml_content);
  }
});
```

## Event Buffer

The client includes an efficient event buffer that:

- Stores up to 10,000 recent trace events
- Provides filtering by participant, tick range, event type
- Automatically drops oldest events when buffer is full
- Tracks statistics for memory usage monitoring

```typescript
// Get buffer statistics
const stats = client.get_buffer_stats();
console.log('Buffer size:', stats.buffer_size);
console.log('Memory usage:', stats.memory_usage_estimate);

// Clear buffer if needed
client.clear_event_buffer();
```

## Performance

- **Bundle Size**: ~100-150KB gzipped
- **Memory Usage**: ~1MB for 10K events
- **Latency**: Sub-millisecond command/response
- **Throughput**: 1000+ events/second

## API Reference

See `types.d.ts` for complete TypeScript definitions.

### Core Methods

- `connect()` - Connect to simulation server
- `disconnect()` - Close connection
- `send_command(command, callback)` - Send command to server
- `subscribe(eventTypes)` - Subscribe to event types
- `get_events_since(eventId)` - Get buffered events
- `is_connected()` - Check connection status

### Event Types

- `TraceEvent` - Simulation trace events
- `BranchSwitched` - Branch change notifications
- `SubscriptionChanged` - Subscription updates
- `SimulationStateChanged` - State change events

## Development

### Testing

```bash
# Run WASM tests in browser
wasm-pack test --headless --firefox
```

### Debugging

The module includes console logging that can be enabled:

```typescript
// Enable debug logging in browser console
localStorage.setItem('debug', 'app:*');
```

### Bundle Size Optimization

The build script automatically optimizes the WASM bundle:

1. Release mode compilation with size optimization (`opt-level = "s"`)
2. Link-time optimization (LTO)
3. wasm-opt optimization with `-Oz` flag
4. Dead code elimination

Current optimizations achieve ~100KB gzipped from ~300KB unoptimized.