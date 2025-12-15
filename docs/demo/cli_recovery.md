# Aura CLI Recovery Demo (CLI + Simulator)

This demo shows Bob onboarding with guardians, losing his device, and recovering with help from Alice and Carol. It mirrors the `scenarios/integration/cli_recovery_demo.toml` flow that the CLI now drives through the simulator.

## Story

- Alice and Carol each create their own authorities.
- Bob creates his authority and **requests** Alice and Carol to become guardians.
- Alice and Carol **accept** the guardian requests; together they form a guardian authority for Bob (threshold 2).
- A three‑party group chat is created and messages flow.
- Bob loses his device; guardians coordinate recovery and restore Bob.
- Bob rejoins the chat with full history intact.

## Running the Demo

From the repo root (inside `nix develop`):
```bash
cargo run -p aura-terminal -- scenarios run --directory scenarios/integration --pattern cli_recovery_demo
```
This uses the CLI scenario runner plus the simulator to execute the guardian setup and recovery choreography and logs to `work/scenario_logs/cli_recovery_demo.log`.

## Cryptographic Notes

**Single-Signer Onboarding**: When a user first creates their account (Alice, Bob, Carol each independently), the system uses standard Ed25519 signatures instead of FROST threshold signatures. This is because:
- FROST requires at least 2 signers (threshold >= 2)
- New accounts start with only one device
- Ed25519 uses the same curve as FROST, so signatures are cryptographically compatible

Once Bob adds Alice and Carol as guardians and configures the threshold to 2-of-3, subsequent signing operations that require guardian approval use the full FROST threshold protocol.

See [Crypto Architecture](../116_crypto.md#7-signing-modes-single-signer-vs-threshold) for details on signing modes.

## What Happens in Each Phase

1) **alice_carol_setup**
   - Create Alice and Carol authorities (simulated).
   - Each uses Ed25519 single-signer mode for initial key generation.

2) **bob_onboarding**
   - Create Bob authority (Ed25519 single-signer mode initially).
   - Bob sends guardian requests to Alice and Carol.
   - Alice and Carol accept.
   - Guardian authority configured for Bob with threshold 2 (now using FROST).

3) **group_chat_setup**
   - Alice creates a group chat and invites Bob and Carol.
   - All members join; context keys derived for the chat.

4) **group_messaging**
   - Normal chat messages among all three; history is persisted.

5) **bob_account_loss**
   - Simulated total device loss for Bob; he cannot access his authority.

6) **recovery_initiation**
   - Bob initiates recovery; Alice and Carol validate and approve the request.
   - Guardian approval threshold (2) is met.

7) **account_restoration**
   - Threshold key recovery runs; Bob’s chat history is synchronized back.

8) **post_recovery_messaging**
   - Bob sends messages again and sees full history; group remains functional.
