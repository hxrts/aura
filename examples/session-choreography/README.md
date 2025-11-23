# Session Choreography Examples

This directory contains examples demonstrating how to convert manual session handling patterns to choreography macros in the Aura system.

## Examples

### 1. `session_patterns.rs` - Basic Session Patterns
Demonstrates the conversion from manual session handling to choreography macros for common session patterns:
- Session creation and coordination
- Participant invitation and response
- Session lifecycle management

### 2. `multi_party_session.rs` - Multi-Party Session Management
Shows how to handle complex multi-party sessions with:
- Dynamic participant management
- Consensus-based decision making
- Graceful handling of participant failures

### 3. `session_lifecycle.rs` - Complete Session Lifecycle
Comprehensive example covering the full session lifecycle:
- Session initiation and setup
- Active session management
- Session termination and cleanup
- Error handling and recovery

## Architecture Benefits

Converting from manual session handling to choreography macros provides:

1. **Consistency**: All session protocols follow the same choreographic patterns
2. **Reliability**: Built-in guard capabilities, flow budgets, and journal integration
3. **Maintainability**: Protocol definitions are declarative and easier to understand
4. **Testing**: Choreographies can be simulated and tested independently
5. **Compliance**: Automatic compliance with Aura's security and authorization model

## Key Patterns Addressed

This addresses the 76 manual session handling patterns identified in the architecture check by:
- Replacing manual `session.send` and `session.recv` calls with choreography message flows
- Converting manual session state management to choreographic roles and phases
- Eliminating manual async protocol implementations in favor of declarative choreographies
- Providing consistent error handling and timeout management across all session types

## Running the Examples

```bash
# Basic session patterns
cargo run --bin session_patterns

# Multi-party session management
cargo run --bin multi_party_session

# Complete session lifecycle
cargo run --bin session_lifecycle
```