# hello-choreo

Minimal two-role session that illustrates the guard chain and journal coupling.

What it shows
- CapGuard → FlowGuard → JournalCoupler ordering
- Predicate: `need(m) ≤ Caps(ctx) ∧ headroom(ctx, cost)`
- Atomic commit with send

Sketch
```rust
choreography! {
  protocol PingPong {
    roles: Alice, Bob;
    Alice -> Bob: Ping [need = SEND_PING, cost = 1, commit = Fact::PingSent];
    Bob   -> Alice: Pong [need = SEND_PONG, cost = 1, commit = Fact::PongSent];
  }
}

async fn alice_execute<E: AuraEffects>(e: &E, ctx: ContextId, bob: DeviceId) -> Result<()> {
  // CapGuard
  ensure!(need(SEND_PING) <= e.caps(ctx));
  // FlowGuard (charge-before-send)
  let r = e.flow().charge(ctx, bob, 1).await?;
  // JournalCoupler: commit + send atomically
  e.journal().merge_facts(Fact::PingSent).and_then(||
      e.network().send(bob, Envelope::ping(ctx, r))
  ).await
}
```

Run
- Use the simulator to assert that on cap/budget failure, no packet is observed and no fact is committed.

