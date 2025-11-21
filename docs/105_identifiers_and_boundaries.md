# Identifiers and Boundaries

## Scope

This reference defines the identifiers that appear in Aura documents. Every other document should reuse these definitions instead of restating partial variants. Each identifier preserves structural privacy by design.

## AuthorityId

`AuthorityId` is a random UUID assigned to an authority journal namespace. It does not leak operator or membership metadata. All public keys, commitment trees, and attested operations reduce under this namespace. When a relationship references an account it uses the `AuthorityId` only.

## ContextId

`ContextId` is a random UUID that identifies a RelationalContext or a derived subcontext. Context IDs are opaque on the wire. They only appear inside encrypted envelopes and receipts. Context IDs never encode participant lists or roles. All flow budgets, receipts, and leakage metrics scope to a `(ContextId, peer)` pair.

## Receipt

`Receipt` is the accountability record emitted by FlowGuard. Each receipt contains `ContextId`, `src: AuthorityId`, `dst: AuthorityId`, `epoch`, `cost`, `nonce`, and chained hash plus signature. Receipts prove that upstream participants charged their budget before forwarding. No receipt includes device identifiers or user handles.

## SessionId

`SessionId` identifies an execution of a choreographic protocol. The identifier pairs a `ContextId` with a nonce. Session IDs are not long-lived. They expire when the protocol completes or when a timeout occurs. Protocol logs use `SessionId` to match receipts with specific choreographies.

## ContentId

`ContentId` is a hash of canonical content bytes. It is used for snapshot digests, stored blobs, and upgrade bundles. Content IDs do not reveal the author or recipient. Any party can verify payload integrity by hashing bytes and comparing with `ContentId`.

## Derived Keys

Aura derives per-context cryptographic keys from reduced account state and `ContextId`. Derived keys never surface on the wire. They only exist inside effect handlers to encrypt payloads, generate commitment tree secrets, or run DKD. The derivation inputs never include device identifiers, so derived keys inherit the privacy guarantees of `AuthorityId` and `ContextId`.
