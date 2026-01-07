# Beyond Local-First

[Local-First](https://www.inkandswitch.com/essay/local-first/) gets a lot right. Its seven ideals for software (fast, multi-device, offline, collaborative, long-lived, private, user-controlled) describe what we actually want from our tools. The key architectural move is treating CRDTs as first-class citizens. Data becomes a distributed structure that converges across replicas without central coordination.

Aura takes this further. If data can be a distributed relation, so can identity. An identity in Aura is not stored on a device. It is a threshold structure distributed across participants, recoverable from the social graph alone. The same is true for all state. If all of your devices are lost, your data and your identity can be reconstructed from the peers who share your contexts.

This calls into question what "local" means. In Local-First, local means the device. In Aura, local means the trust graph. Your data is local to the people you have chosen to share it with. Your identity is local to the relationships that constitute it. The device is just a window into this distributed locality, not the source of truth.

Rather than asking "How can a device own its data?", Aura asks: Who is the user in a world where people span many devices, trust relationships, and shared spaces?

## Identity Beyond the Device

Implicit in the Local-First pattern is an assumption that individual devices should be the authoritative record for state and identity. Aura starts with a different assumption. Identities in Aura are opaque authorities built from some threshold of partial signatures. No single device holds the full key, no single loss event destroys the identity. The user's long-term presence is maintained cryptographically rather than physically.

This model does not distinguish between personal and group identity. An authority can be one person with one device, one person with many devices, or many people acting as one through threshold signing. External observers see an opaque identity in all cases. There is no need to retrofit group identity onto a system designed for individuals. The same primitive works at every scale.

CRDT-like convergence still matters, but it is layered beneath a stronger guarantee: an attacker cannot impersonate you by compromising a device, and you cannot lose yourself if hardware fails. The identity is durable because it is distributed.

## Collective Resources Instead of Local Silos

Local-First architectures preserve local ownership while enabling sync, but they stop at the boundary of the individual. Every user is their own island, occasionally exchanging state with others. Aura breaks that boundary at two levels.

First, authorities themselves can be collective. A group can act as a single identity through threshold cryptography, signing and encrypting as one actor without any member holding the full key. This is not a wrapper around individual accounts. It is the same primitive used for personal identity.

Second, relational contexts provide shared state between distinct authorities. Every shared context has a dedicated journal and its own cryptographic root. Participants jointly maintain the space through CRDT convergence for most operations and consensus for those requiring agreement. No server arbitrates. No user acts as host. The context is the unit of collaboration.

This allows the system to support resources that belong to relationships, not devices: shared storage, shared budgets, shared state transitions, and shared coordination protocols. Collaboration is no longer "merge my edits with yours," but "we jointly enact changes to a shared state machine."

## Consent as a Primitive, Not a Policy

Local-First aims for privacy through local storage and controlled replication. Aura requires stronger guarantees because the social graph is the network. Data, metadata, and protocol steps flow through people’s devices. This requires a discipline that goes beyond access control.

Aura makes consent a technical primitive. Every action that produces a network-visible effect must pass a guard chain: capabilities, information flow budgets, leakage budgets, and journal coupling. Guard evaluation is pure and synchronous. An async interpreter executes the resulting commands only after the entire chain succeeds. No transport work occurs unless every permission and budget check passes. A message cannot be sent unless all parties have cryptographically granted the necessary permissions. This ensures that participation, disclosure, and coordination are always intentional.

Local-First keeps data on your device. Aura ensures that every observable effect results from consent, whether the data lives locally or within a shared context.

## Coordination Without a Server

Local-First reduces dependence on servers, but often expects them for discovery, presence, or convenience. Aura removes them from the critical path entirely. Rendezvous happens through encrypted gossip within a context. Secure channels form through threshold-derived keys. Consensus is single-shot and scoped to specific contexts. Everything is routed through the peers who already have a reason to know each other.

The result is a network that resembles a living mesh: small, intimate, and shaped exactly like your social world. Instead of a cloud that intermediates collaboration, coordination arises from the relationships themselves.

## A Philosophical Progression

Local-First was a necessary corrective to the server-centric era. It restored autonomy, durability, and respect for the user. But it retains an implicit assumption: the device is the primary actor.

When shared resources enter the picture, this assumption creates problems. If the device is authoritative, who holds group state? The common answer is to delegate authority to a server, recreating the dependency that Local-First tried to escape. The dichotomy between device and server misses a third option: identity itself can be distributed across participants without requiring a server to hold it together.

Aura takes this path. It treats identity as relational, data as shared between peers who trust each other, and action as something that only occurs with explicit consent. Devices come and go. Servers are optional. What persists are relationships, commitments, and the cryptographic structures that encode them.

In this sense, Aura is not only Local-First. It is Network-Native, where the network is not an infrastructure provider but the set of people you trust. Collaboration is not a feature but the substrate. Autonomy is guaranteed not by ownership of hardware, but by the sovereignty of the user within their social graph.

## See Also

- [Authority and Identity](../100_authority_and_identity.md) - Relational identity model and contextual isolation
- [Relational Contexts](../103_relational_contexts.md) - Shared state machines and collective resources
- [Authorization](../109_authorization.md) - Consent primitives and capability attenuation
- [Rendezvous](../110_rendezvous.md) - Peer discovery without servers
