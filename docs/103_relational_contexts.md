# Relational Contexts

This document describes the architecture of relational contexts in Aura. It explains how cross-authority relationships are represented using dedicated context namespaces. It defines the structure of relational facts and the role of Aura Consensus in producing agreed relational state. It also describes privacy boundaries and the interpretation of relational data by participating authorities.

## 1. RelationalContext Abstraction

A relational context is shared state linking two or more authorities. A relational context has its own journal namespace. A relational context does not expose internal authority structure. A relational context contains only the facts that the participating authorities choose to share.

A relational context is identified by a `ContextId`. Authorities publish relational facts inside the context journal. The context journal is a join semilattice under set union. Reduction produces a deterministic relational state.

```rust
pub struct ContextId(Uuid);
```

This identifier selects the journal namespace for a relational context. It does not encode participant information. It does not reveal the type of relationship. Only the participants know how the context is used.

## 2. Participants and Fact Store

A relational context has a defined set of participating authorities. This set is not encoded in the `ContextId`. Participation is expressed by writing relational facts to the context journal. Each fact references the commitments of the participating authorities.

```rust
pub enum RelationalFact {
    GuardianBinding { account_commitment: Hash32, guardian_commitment: Hash32, parameters: Vec<u8> },
    RecoveryGrant { account_commitment_old: Hash32, account_commitment_new: Hash32, guardian_commitment: Hash32, operation: Vec<u8> },
    Generic { payload: Vec<u8>, bindings: Vec<Hash32> },
}
```

This fact model covers guardian configuration, recovery, and general relational operations. Each fact is self contained. Each fact carries enough information for reduction to produce relational state.

## 3. Prestate Model

Relational context operations use a prestate model. Aura Consensus verifies that all witnesses see the same authority states. The prestate hash binds the relational fact to current authority states.

```rust
let prestate_hash = H(C_auth1, C_auth2, C_context);
```

This hash represents the commitments of the authorities and the current context. Aura Consensus witnesses check this hash before producing shares. The final commit fact includes a threshold signature over the relational operation.

## 4. Types of Relational Contexts

Several categories of relational contexts appear in Aura. Each fact type carries a well-defined schema to ensure deterministic reduction.

### GuardianConfigContext

Stores `GuardianBinding` facts:

```json
{
  "type": "GuardianBinding",
  "account_commitment": "Hash32",
  "guardian_commitment": "Hash32",
  "parameters": {
    "recovery_delay_secs": 86400,
    "notification_required": true
  },
  "consensus_commitment": "Hash32",
  "consensus_proof": "ThresholdSignature"
}
```

- `account_commitment`: reduced commitment of the protected authority.
- `guardian_commitment`: reduced commitment of the guardian authority.
- `parameters`: serialized GuardianParameters (delay, notification policy, etc.).
- `consensus_commitment`: commitment hash of the Aura Consensus instance that approved the binding.
- `consensus_proof`: aggregated signature from the witness set.

### GuardianRecoveryContext

Stores `RecoveryGrant` facts:

```json
{
  "type": "RecoveryGrant",
  "account_commitment_old": "Hash32",
  "account_commitment_new": "Hash32",
  "guardian_commitment": "Hash32",
  "operation": {
    "kind": "ReplaceTree",
    "payload": "Base64(TreeOp)"
  },
  "consensus_commitment": "Hash32",
  "consensus_proof": "ThresholdSignature"
}
```

- `account_commitment_old/new`: before/after commitments for the account authority.
- `guardian_commitment`: guardian authority that approved the grant.
- `operation`: serialized recovery operation (matches TreeOp schema).
- `consensus_*`: Aura Consensus identifiers tying the grant to witness approvals.

### Generic / Application Contexts

Shared group or project contexts can store application-defined facts:

```json
{
  "type": "Generic",
  "payload": "Base64(opaque application data)",
  "bindings": ["Hash32 commitment of participant A", "Hash32 commitment of participant B"],
  "labels": ["project:alpha", "role:reviewer"]
}
```

Generic facts should include enough metadata (`bindings`, optional labels) for interpreters to apply context-specific rules.

## 5. Relational Facts

Relational facts express specific cross-authority operations. A `GuardianBinding` fact defines the guardian authority for an account. A `RecoveryGrant` fact defines an allowed update to the account state. A `Generic` fact covers application defined interactions. Consensus-backed facts include the `consensus_commitment` and aggregated signature so reducers can verify provenance even after witnesses rotate.

Reduction applies all relational facts to produce relational state. Reduction verifies that authority commitments in each fact match the current reduced state of each authority.

```rust
pub struct RelationalState {
    pub bindings: Vec<RelationalFact>,
}
```

This structure represents the reduced relational state. It contains the relational facts relevant to the context. Reduction removes superseded relational facts when necessary.

## 6. Aura Consensus in Relational Contexts

Some relational operations require strong agreement. Aura Consensus produces these operations. Aura Consensus uses a witness set drawn from participating authorities. Witnesses compute shares after verifying the prestate hash.

Commit facts contain threshold signatures. Each commit fact is inserted into the relational context journal. Reduction interprets the commit fact as a confirmed relational operation.

Aura Consensus binds relational operations to authority state. After consensus completes, the initiator inserts a `CommitFact` into the relational context journal that includes:

```rust
pub struct RelationalCommitFact {
    pub context_id: ContextId,
    pub consensus_commitment: Hash32, // H(Op, prestate)
    pub fact: RelationalFact,         // GuardianBinding, RecoveryGrant, etc.
    pub aggregated_signature: ThresholdSignature,
    pub attesters: BTreeSet<AuthorityId>,
}
```

Reducers validate `aggregated_signature` before accepting the embedded `RelationalFact`. This mirrors the account-level process but scoped to the context namespace.

## 7. Interpretation of Relational State

Authorities interpret relational state by reading the relational context journal. An account reads `GuardianBinding` facts to determine its guardian authority. A guardian authority reads the same facts to determine which accounts it protects.

Other relational contexts follow similar patterns. Each context defines its own interpretation rules. No context affects authority internal structure. Context rules remain confined to the specific relationship.

## 8. Privacy and Isolation

A relational context does not reveal its participants. The `ContextId` is opaque. Only participating authorities know the relationship. No external party can infer connections between authorities based on context identifiers.

Profile information shared inside a context stays local to that context. Display names and contact attributes do not leave the context journal. Each context forms a separate identity boundary. Authorities can maintain many unrelated relationships without cross linking.

## 9. Summary

Relational contexts represent cross-authority relationships in Aura. They provide shared state without revealing authority structure. They support guardian configuration, recovery, and application specific collaboration. Aura Consensus ensures strong agreement where needed. Deterministic reduction ensures consistent relational state. Privacy boundaries isolate each relationship from all others.
