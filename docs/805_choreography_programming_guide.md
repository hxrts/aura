# Choreography Programming Guide

This guide provides comprehensive information about using the Rumpsteak-Aura choreographic programming system and DSL for implementing distributed protocols in Aura.

## Repository Information
- **GitHub Repository**: https://github.com/hxrts/rumpsteak-aura
- **DeepWiki URL**: https://deepwiki.com/hxrts/rumpsteak-aura (may be slightly out of date)

This guide covers the practical aspects of using rumpsteak-aura for choreographic programming within the Aura system. For the most up-to-date information, refer to the GitHub repository.

## Documentation Content

### Choreography DSL

The choreography DSL provides a high-level syntax for defining distributed protocols from a global perspective, which are automatically projected into local session types for each participant.

#### Basic Structure
```rust
choreography ProtocolName {
    roles: Role1, Role2, Role3
    
    // Optional sub-protocols
    protocol SubProtocolName {
        // protocol statements
    }
    
    // Main protocol body
    protocol_statements...
}
```

#### Core Grammar Rules
- **Send Statement**: `Role1 -> Role2: MessageName` - Point-to-point message from role Role1 to role Role2
- **Broadcast Statement**: `Leader ->* : Announcement` - Message from one role to all other roles
- **Choice Statement**: `choice DeciderRole { option1: { ... } option2: { ... } }` - Conditional branching with optional guards
- **Loop Statement**: `loop (condition) { ... }` - Supports count, decides, custom, or infinite loops
- **Parallel Statement**: `parallel { branch1 | branch2 }` - Concurrent execution
- **Recursive Protocol**: `rec Label { ... }` - Labeled recursion points
- **Call Statement**: `call SubProtocolName` - Invoke sub-protocols
- **Parameterized Roles**: `Worker[N]` for role arrays, `Worker[i]` for indexed access

#### Example
```rust
choreography PingPong {
    roles: Alice, Bob
    Alice -> Bob: Ping
    Bob -> Alice: Pong
}
```

### Session Type System

The session type system provides compile-time safety for distributed protocols using Multiparty Session Types (MPST) to statically guarantee the absence of communication errors like deadlocks.

#### Key Concepts
1. **Global Protocol Specification**: Define the entire interaction among all participants from a global viewpoint
2. **Projection**: Automatically generate local session types for each role from the global choreography
3. **Local Session Types**: Each participant gets a precise sequence of expected send/receive operations
4. **Compile-Time Safety**: Type mismatches prevent communication errors at compile time

#### How It Works
- The `project` function transforms global choreographies into local session types
- For `Send` operations: sender gets `LocalType::Send`, receiver gets `LocalType::Receive`
- For `Choice` statements: deciding role gets `LocalType::Select`, others get `LocalType::Offer`
- Generated session types like `Send<S, Add, Send<S, Add, Receive<S, Sum, End>>>` enforce exact message ordering

#### Safety Guarantees
- Prevents deadlocks through static analysis
- Ensures message ordering compliance
- Catches protocol violations at compile time
- Eliminates race conditions in distributed communication

### Effect System

The effect system decouples choreographic protocol logic from transport implementation using a free algebra approach where protocols are represented as data structures.

#### Core Components
- **`Effect<R, M>` Enum**: Represents individual choreographic operations:
  - `Send { to: R, msg: M }` - Send a message to another role
  - `Recv { from: R, msg_type: &'static str }` - Receive a message from another role
  - `Choose { at: R, label: Label }` - Make an internal choice
  - `Offer { from: R }` - Wait for external choice
  - `Branch { choosing_role: R, branches: Vec<(Label, Program<R, M>)> }` - Handle branching
  - `Loop { iterations: Option<usize>, body: Box<Program<R, M>> }` - Execute loops
  - `Timeout { at: R, dur: Duration, body: Box<Program<R, M>> }` - Execute with timeout
  - `Parallel { programs: Vec<Program<R, M>> }` - Execute programs concurrently
  - `End` - End of program

- **`Program<R, M>` Struct**: Holds sequences of `Effect`s representing complete choreographic protocols

- **`ChoreoHandler` Trait**: Central interface for interpreting effects with async methods:
  - `async fn send<M>(&mut self, ep: &mut Self::Endpoint, to: Self::Role, msg: &M)`
  - `async fn recv<M>(&mut self, ep: &mut Self::Endpoint, from: Self::Role)`
  - `async fn choose(&mut self, ep: &mut Self::Endpoint, who: Self::Role, label: Label)`
  - `async fn offer(&mut self, ep: &mut Self::Endpoint, from: Self::Role)`

#### Effect System Usage
```rust
let program = Program::new()
    .send(Role::Bob, Message::Ping)
    .recv::<Message>(Role::Bob)
    .choose(Role::Alice, Label("continue"))
    .end();

let result = interpret(&mut handler, &mut endpoint, program).await?;
```

#### Handler Implementations
- **`InMemoryHandler`**: For local testing using futures channels
- **`RumpsteakHandler`**: For production distributed execution with session types
- **`RecordingHandler`**: Records operations for verification and testing
- **`NoOpHandler`**: No-op implementation for testing protocol structure

#### Middleware Support
Composable middleware can wrap base handlers:
- **`Trace`**: Logs all operations for debugging
- **`Metrics`**: Counts operations for monitoring
- **`Retry`**: Retries failed operations with exponential backoff
- **`FaultInjection`**: Injects random failures and delays for testing

```rust
let handler = InMemoryHandler::new(role);
let handler = Retry::new(handler, 3, Duration::from_millis(100));
let handler = Trace::new(handler, "Alice".to_string());
let handler = Metrics::new(handler);
```

### WASM Support

Rumpsteak-Aura supports WebAssembly compilation for browser-based distributed protocols with the core library and effect handlers working in browser environments.

#### What Works in WASM
- Core session types and choreography system work fully
- `InMemoryHandler` provides local message passing for testing protocols
- `RumpsteakHandler` compiles for WASM and can be used with custom network transports
- All middleware (Trace, Metrics, Retry, FaultInjection) functions correctly
- Effect system and interpreter execute normally
- Timeouts use wasm-timer for cross-platform support

#### What Does Not Work in WASM
- Multi-threading (WASM runs single-threaded)
- Native file system access (requires browser File APIs)
- Some examples that use Redis/Hyper

#### Requirements
Add the `wasm` feature to dependencies in `Cargo.toml`:
```toml
[dependencies]
rumpsteak-choreography = { version = "0.1", features = ["wasm"] }
wasm-bindgen = "0.2"
wasm-bindgen-futures = "0.4"
```

#### Build Steps
1. **Development build**:
   ```bash
   wasm-pack build --target web
   ```

2. **Release build** (optimized):
   ```bash
   wasm-pack build --target web --release
   ```

3. **Testing**:
   ```bash
   wasm-pack test --headless --chrome
   ```

#### Example Usage
Complete browser example available in `examples/wasm-ping-pong/`:
```bash
cd examples/wasm-ping-pong
./build.sh
python3 -m http.server 8000
```

#### Custom Network Transport for WASM
```rust
use web_sys::WebSocket;
use wasm_bindgen::JsCast;

pub struct WebSocketHandler {
    role: Role,
    socket: WebSocket,
    incoming: mpsc::UnboundedReceiver<Vec<u8>>,
}

impl ChoreoHandler for WebSocketHandler {
    // Implement using browser WebSocket APIs
}
```

#### Platform Differences
- Runtime module provides platform-specific functions
- Timeouts use conditional compilation (wasm-timer vs tokio)
- Single-threaded async execution vs multi-threaded
