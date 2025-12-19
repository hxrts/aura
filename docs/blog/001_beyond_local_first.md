# Beyond Local-First

The notion of [Local-First](https://www.inkandswitch.com/essay/local-first/) reframed software design patterns, asserting that real-time, multi-device software does not have to rely on centralized servers. Its core insight is that data should live primarily on the user’s own devices, with the network acting as a helper rather than a gatekeeper. CRDTs, offline-first UX, and user agency over data push us toward a healthier relationship with our tools.

Aura builds on the same dissatisfaction with cloud dependency, but arrives at a different destination. Where Local-First keeps authority on the device, Aura treats the social graph as the computational substrate. The device loses its status as the privileged anchor of trust. Instead, identity, state, and availability emerge from a network of peers who collectively maintain each other’s presence and continuity.

Rather than asking “How can a device own its data?”, Aura asks a deeper question: Who is the user in a world where people span many devices, trust relationships, and shared spaces?

## Identity Beyond the Device

Local-First assumes each device holds the primary state for a user. Aura discards this assumption. An Aura identity is an opaque authority built from a threshold of devices or guardians. No single device holds the full key; no single loss event destroys the identity. The user’s long-term presence is maintained cryptographically rather than physically. CRDT-like convergence still matters, but it is layered beneath a stronger guarantee: an attacker cannot impersonate you by compromising a device, and you cannot “lose yourself” if hardware fails.

This shifts us from personal device autonomy to relational identity continuity. The identity is durable because it is distributed.

## Collective Resources Instead of Local Silos

Local-First architectures preserve local ownership while enabling sync, but they stop at the boundary of the individual. Every user is their own island, occasionally exchanging state with others. Aura breaks that boundary cleanly.

In Aura, relationships, groups, and shared contexts are first-class state machines backed by consensus and semilattice facts. Every shared context—friendship, project, group chat, recovery relationship—has a dedicated journal and its own cryptographic root. Participants jointly maintain the space. No server arbitrates; no user acts as host. The context is the unit of collaboration.

This allows the system to support resources that belong to relationships, not devices: shared storage, shared budgets, shared state transitions, and shared coordination protocols. Collaboration is no longer “merge my edits with yours,” but “we jointly enact changes to a shared state machine.”

## Consent as a Primitive, Not a Policy

Local-First aims for privacy through local storage and controlled replication. Aura requires stronger guarantees because the social graph is the network. Data, metadata, and protocol steps flow through people’s devices. This requires a discipline that goes beyond access control.

Aura makes consent a technical primitive. Every action that produces a network-visible effect must pass a guard chain: capabilities, information flow budgets, leakage budgets, and journal coupling. Guard evaluation runs over a prepared `GuardSnapshot` and returns `EffectCommand` data that an async interpreter executes only after the entire chain succeeds, so no transport work occurs unless every permission and budget check passes. A message cannot be sent—literally cannot be emitted on the wire—unless all parties have cryptographically granted the necessary permissions. This ensures that participation, disclosure, and coordination are always intentional.

Local-First keeps data on your device. Aura ensures that every observable effect results from consent—whether the data lives locally or within a shared context.

## Coordination Without a Server

Local-First reduces dependence on servers, but often expects them for discovery, presence, or convenience. Aura removes them from the critical path entirely. Rendezvous happens through encrypted gossip within a context. Secure channels form through threshold-derived keys. Consensus is single-shot and scoped to specific contexts. Everything is routed through the peers who already have a reason to know each other.

The result is a network that resembles a living mesh: small, intimate, and shaped exactly like your social world. Instead of a cloud that intermediates collaboration, coordination arises from the relationships themselves.

## A Philosophical Progression

Local-First was a necessary corrective to the server-centric era. It restored autonomy, durability, and respect for the user. But it retains an implicit assumption: the device is the primary actor.

Aura moves past that. It treats identity as relational, data as shared between peers who trust each other, and action as something that only occurs with explicit consent. Devices come and go. Servers are optional. What persists are relationships, commitments, and the cryptographic structures that encode them.

In this sense, Aura is not only Local-First. It is Network-Native, where the network is not an infrastructure provider but the set of people you trust. It is a world where collaboration is not a feature but the substrate—and where autonomy is guaranteed not by ownership of hardware, but by the sovereignty of the user within their social graph.

This is what lies beyond Local-First: a system where data is local, relationships are global, but neither depends on the cloud to survive.

## See Also

- [Authority and Identity](../100_authority_and_identity.md) - Relational identity model and contextual isolation
- [Relational Contexts](../103_relational_contexts.md) - Shared state machines and collective resources
- [Authorization](../109_authorization.md) - Consent primitives and capability attenuation
- [Rendezvous](../110_rendezvous.md) - Peer discovery without servers
