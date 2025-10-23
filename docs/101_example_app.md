# 101 · Example App Walkthrough (BitChat-lite)

This guide shows how a lightweight messaging client can use the Phase 0/1 APIs.
It mirrors the “BitChat-lite” demo we plan to ship alongside the MVP.

## 1. Bootstrapping the Device Agent

```rust
use aura_client::{DeviceAgent, IdentityConfig, StorageClient};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = IdentityConfig::load("~/.aura/config.toml")?;
    let agent = DeviceAgent::connect(&cfg).await?;
    let storage = StorageClient::new(agent.clone()).await?;
    // ...
    Ok(())
}
```

## 2. Derive Per-Channel Identity

```rust
let (identity, ticket) = agent
    .derive_simple_identity("bitchat-lite", "geohash:9q8yy:2024-11-28")
    .await?;
mesh_transport.set_presence_ticket(ticket);
```

The helper gives us:
- `identity.pk_derived` – use in the Noise handshake.
- `identity.seed_fingerprint` – audit/debug logging.
- `ticket` – required for every mesh handshake (enforces session epoch).

## 3. Publish a Message

```rust
let payload = serde_cbor::to_vec(&Message {
    sender: identity.pk_derived,
    body: input_text.clone(),
    timestamp: now(),
})?;

let metadata = serde_cbor::to_vec(&hashmap! {
    "geohash".to_string() => "9q8yy".as_bytes().to_vec(),
    "timestamp".to_string() => now().to_le_bytes().to_vec(),
})?;

let cid = storage.store_encrypted(
    &payload,
    Recipients::Broadcast,
    PutOpts {
        class: StoreClass::Owned,
        pin: PinClass::Pin,
        repl_hint: ReplicationHint::local_mesh(),
        context: Some(ContextDescriptor::from_identity(&identity)),
        app_metadata: Some(metadata),
        caps: vec![],
    },
).await?;
```

- One manifest, one signing session, inline metadata.
- Indexer automatically updates `/index/app/bitchat-lite/...` for queries.
- Presence ticket ensures mesh peers honour the session epoch.

## 4. Sync via Authenticated CRDT

```rust
agent.sync_account_state().await?;
storage.sync_index().await?;
```

Under the hood:
- Devices exchange signed CRDT events (new posts, guardians, policy changes).
- Threshold-signed events (e.g., epoch bump) are verified before apply.

## 5. Fetch Messages

```rust
let objects = storage.query_by_app("bitchat-lite").await?;
for cid in objects {
    let (plaintext, manifest) = storage.fetch_encrypted(&cid, GetOpts::default()).await?;
    let message: Message = serde_cbor::from_slice(&plaintext)?;
    render_message(manifest.app_metadata.as_deref(), message);
}
```

`fetch_encrypted` returns both payload and manifest so the app can interpret
metadata without additional round trips.

## 6. Handling Recovery

```rust
if agent.is_primary_device_lost().await? {
    agent.start_recovery().await?;
    notify_guardians();
    // Cooldown enforced by CRDT; DeviceAgent polls until ready.
    agent.finish_recovery().await?;
    mesh_transport.refresh_presence_ticket().await?;
}
```

- Guardian approvals recorded in CRDT.
- Upon completion, session epoch bumps and transport automatically rejects old keys.

## 7. Testing Checklist

- [ ] Derived identity changes when session epoch increments.  
- [ ] Messages replicate to another device via proof-of-storage.  
- [ ] Guardian recovery revokes old tickets and grants access to new device.  
- [ ] Quota enforcement triggers LRU eviction when cache is full.  
- [ ] Presence ticket expiry forces re-issuance before handshake.