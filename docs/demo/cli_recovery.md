# Aura CLI Recovery Demo (CLI + Simulator)

This demo shows Bob onboarding with guardians, losing his device, and recovering with help from Alice and Charlie. It mirrors the `scenarios/integration/cli_recovery_demo.toml` flow that the CLI now drives through the simulator.

## Story

- Alice and Charlie each create their own authorities.
- Bob creates his authority and **requests** Alice and Charlie to become guardians.
- Alice and Charlie **accept** the guardian requests; together they form a guardian authority for Bob (threshold 2).
- A three‑party group chat is created and messages flow.
- Bob loses his device; guardians coordinate recovery and restore Bob.
- Bob rejoins the chat with full history intact.

## Running the Demo

From the repo root (inside `nix develop`):
```bash
cargo run -p aura-cli -- scenarios run --directory scenarios/integration --pattern cli_recovery_demo
```
This uses the CLI scenario runner plus the simulator to execute the guardian setup and recovery choreography and logs to `work/scenario_logs/cli_recovery_demo.log`.

## What Happens in Each Phase

1) **alice_charlie_setup**
   - Create Alice and Charlie authorities (simulated).

2) **bob_onboarding**
   - Create Bob authority.
   - Bob sends guardian requests to Alice and Charlie.
   - Alice and Charlie accept.
   - Guardian authority configured for Bob with threshold 2.

3) **group_chat_setup**
   - Alice creates a group chat and invites Bob and Charlie.
   - All members join; context keys derived for the chat.

4) **group_messaging**
   - Normal chat messages among all three; history is persisted.

5) **bob_account_loss**
   - Simulated total device loss for Bob; he cannot access his authority.

6) **recovery_initiation**
   - Bob initiates recovery; Alice and Charlie validate and approve the request.
   - Guardian approval threshold (2) is met.

7) **account_restoration**
   - Threshold key recovery runs; Bob’s chat history is synchronized back.

8) **post_recovery_messaging**
   - Bob sends messages again and sees full history; group remains functional.
