# Aura Protocol Formal Specifications

This directory contains formal specifications of the Aura protocol using Quint, an executable specification language based on the Temporal Logic of Actions (TLA).

## Getting Started

To work with these specifications, make sure you're in the Nix development environment:

```bash
nix develop
```

Then you can use Quint commands:

```bash
# Check syntax and types
quint typecheck <spec>.qnt

# Run the REPL
quint repl <spec>.qnt

# Verify properties with model checking
quint verify <spec>.qnt

# Generate random traces
quint run <spec>.qnt
```

## Specifications

### Protocol Specifications
- `threshold_signatures.qnt` - FROST threshold signature protocol
- `deterministic_key_derivation.qnt` - DKD protocol for context-specific keys
- `session_epochs.qnt` - Session epoch and presence ticket management
- `counter_coordination.qnt` - SSB counter coordination choreography
- `journal_effect_api.qnt` - CRDT-based account effect_api with threshold-signed events
- `choreographic_coordination.qnt` - Session types and protocol state machines
- `social_bulletin_board.qnt` - SBB web-of-trust and peer discovery
- `group_communication.qnt` - BeeKEM/CGKA group messaging protocols

### Coverage Matrix
| Subsystem | Specification | Status |
|-----------|---------------|---------|
| FROST Threshold Signatures | `threshold_signatures.qnt` | [OK] Core |
| P2P Deterministic Key Derivation | `deterministic_key_derivation.qnt` | [OK] Core |
| Session Management & Presence | `session_epochs.qnt` | [OK] Core |
| Distributed Counter Coordination | `counter_coordination.qnt` | [OK] Core |
| CRDT Journal & Capabilities | `journal_effect_api.qnt` | [OK] Core |
| Session Types & Choreographies | `choreographic_coordination.qnt` | [CRITICAL] Critical |
| Social Web-of-Trust & P2P Discovery | `social_bulletin_board.qnt` | [CRITICAL] Critical |
| BeeKEM Group Communication | `group_communication.qnt` | [CRITICAL] Critical |
| Key Resharing & Recovery | Covered in `threshold_signatures.qnt` | [OK] Integrated |
| Transport & Network Protocols | Covered in `social_bulletin_board.qnt` | [OK] Integrated |
| Storage & Replication | Covered in `journal_effect_api.qnt` | [OK] Integrated |
| Device Authentication | Covered in `session_epochs.qnt` | [OK] Integrated |
| Error Recovery & Byzantine Faults | Cross-cutting in all specs | [OK] Integrated |

## Protocol Properties to Verify

- **Safety**: Invalid states are never reached
- **Liveness**: Progress is eventually made
- **Threshold Security**: M-of-N signatures required for critical operations
- **Session Isolation**: Compromised sessions don't affect others
- **Counter Uniqueness**: No duplicate counter values in SSB relationships
- **Ledger Consistency**: CRDT convergence with threshold authorization

## Resources

- [Quint Documentation](https://quint-lang.org/docs)
- [Choreographic Testing](https://quint-lang.org/choreo)
- [Aura Architecture](../../docs/)
