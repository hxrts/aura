# Session Choreography Examples

This directory contains examples demonstrating how to convert manual session handling patterns to choreography macros in the Aura system.

## Examples

### 1. `session_patterns.rs` - Basic Session Patterns
Demonstrates the conversion from manual session handling to choreography macros for common session patterns:
- Session creation and coordination
- Participant invitation and response
- Session lifecycle management

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
```
