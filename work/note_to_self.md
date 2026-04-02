# Note-to-Self Channel and Home Decoupling

## Problem

The Note-to-self channel is not a real AMP channel. It exists as a display-only entry in `ChatState` until the first message is sent, at which point `ensure_runtime_note_to_self_channel` lazily creates it in the runtime. This means runtime operations like topic editing fail with "Missing authoritative context" because the runtime has no registered context for the channel.

Separately, account bootstrap auto-creates a home ("{nickname}'s Home") as a mandatory step. This is wrong. A home is a social structure the user chooses to create when they want to connect with others. A new account should start with just the authority, signing keys, and a Note-to-self channel.

## Design

### Note-to-self channel

The Note-to-self channel is a real AMP channel scoped to the user's authority. It uses an authority-scoped context (single participant). The context and channel IDs are deterministic from the authority ID so all devices in a multi-factor authority converge on the same channel.

The channel is created during account bootstrap, immediately after signing keys are ready. It goes through the same `amp_create_channel` and `amp_join_channel` path as any other channel. The existing `ensure_runtime_note_to_self_channel` function handles this correctly and is idempotent.

### Home creation

Homes are user-initiated. Creating or joining a home is how a user opts into social participation. The bootstrap flow does not create a home. The Neighborhood screen shows an empty state with a "Create Home" action. Channel creation for group chats requires a home context and fails with a clear message if none exists.

### Post-bootstrap navigation

After account creation, the user lands on the Chat screen showing the Note-to-self channel. Previously they landed on Neighborhood showing their auto-created home.

## Key files

| File | Role |
|------|------|
| `crates/aura-app/src/workflows/account.rs` | Bootstrap flow, completion check |
| `crates/aura-app/src/workflows/messaging.rs` | `ensure_runtime_note_to_self_channel`, send path |
| `crates/aura-app/src/workflows/messaging/routing.rs` | Context resolution with Note-to-self bypass |
| `crates/aura-app/src/workflows/semantic_facts.rs` | `AccountCreatedProof` (carries home_id) |
| `crates/aura-app/src/views/chat.rs` | Note-to-self ID derivation, `ensure_note_to_self_channel` |
| `crates/aura-terminal/src/tui/screens/app/subscriptions/chat_projection.rs` | UI-only Note-to-self injection |
| `crates/aura-ui/src/app/runtime_views/chat.rs` | UI-only Note-to-self injection |
| `crates/aura-web/src/shell/app.rs` | Post-bootstrap navigation target |
| `crates/aura-web/src/harness/commands.rs` | Post-bootstrap navigation target |
| `crates/aura-web/src/shell_host.rs` | Post-bootstrap navigation target |

---

## Phase 1: Provision Note-to-self at bootstrap

Move Note-to-self channel creation from lazy (first message send) to bootstrap (account finalization). The home creation stays in place for now to keep the rest of the system working while we validate the Note-to-self path.

### Tasks

- [x] In `finalize_runtime_account_bootstrap_inner` (`account.rs:504`), add a call to `ensure_runtime_note_to_self_channel` after signing key bootstrap completes (after line 571/590), using the authority_id already available and `current_time_ms` from the time workflow
- [x] Make `ensure_runtime_note_to_self_channel` pub(crate) so it can be called from the account module (currently `async fn` with no visibility modifier at `messaging.rs:994`)
- [x] In `reconcile_pending_runtime_account_bootstrap` (`account.rs:466`), add a call to `ensure_runtime_note_to_self_channel` in the `(true, Some(_))` and `(true, None)` arms so existing accounts get the channel on next login
- [x] Run `cargo test -p aura-app` and verify green
- [x] Run `cargo check -p aura-terminal -p aura-web` and verify green
- [x] Commit: "Provision Note-to-self AMP channel at account bootstrap"

## Phase 2: Remove lazy creation and UI special cases

Now that the channel is created at bootstrap, remove the workarounds.

### Tasks

- [x] In `messaging.rs` around line 3709, remove the `if is_note_to_self { ensure_runtime_note_to_self_channel(...) }` block from the send path. The channel already exists.
- [x] In `messaging/routing.rs` lines 174-178, remove the hardcoded Note-to-self bypass in `context_id_for_channel`. The normal `resolve_amp_channel_context` lookup will find it.
- [x] In `chat_projection.rs` (lines 230, 264), remove the `ensure_note_to_self_channel()` calls that inject the channel into ChatState as a display-only entry
- [x] In `aura-ui/src/app/runtime_views/chat.rs` (line 112), remove the `ensure_note_to_self_channel()` call
- [x] Run `cargo test -p aura-app` and verify green
- [x] Run `cargo check -p aura-terminal -p aura-web -p aura-ui` and verify green
- [ ] Run `just demo` and verify Note-to-self channel appears in the chat screen and topic editing works (manual verification)
- [x] Commit: "Remove Note-to-self lazy creation and UI injection workarounds"

## Phase 3: Remove home creation from bootstrap

Decouple home creation from account setup. The bootstrap flow creates the authority, signing keys, and Note-to-self channel. Home creation becomes user-initiated.

### Tasks

- [x] In `finalize_runtime_account_bootstrap_inner` (`account.rs:504`), remove the `context::create_home()` call (lines 518-524) and the `ensure_local_home_projection` call (lines 606-614)
- [x] Remove the `home_name` variable (line 510) and the `home_id` return from the function
- [x] Change return type of `finalize_runtime_account_bootstrap_inner` from `Result<ChannelId, AuraError>` to `Result<(), AuraError>`
- [x] Update `AccountCreatedProof` in `semantic_facts.rs` to remove the `home_id` field since bootstrap no longer creates a home. Replace with a marker proof (no data needed).
- [x] Update `issue_account_created_proof` to not require a `home_id` argument
- [x] Update the caller at `account.rs:447-459` to not pass `home_id` to the proof
- [x] Update `has_runtime_bootstrapped_account` (`account.rs:366-374`) to check for runtime account config instead of home context existence
- [x] Run `cargo test -p aura-app` and fix any test failures from the changed bootstrap flow
- [x] Run `cargo check --workspace` and fix compilation errors from the changed return types
- [ ] Commit: "Remove automatic home creation from account bootstrap"

## Phase 4: Update post-bootstrap navigation

After account creation, the user should land on Chat (showing Note-to-self) instead of Neighborhood (showing the now-nonexistent auto-home).

### Tasks

- [ ] In `aura-web/src/shell/app.rs`, change all `finalize_account_setup(ScreenId::Neighborhood)` calls to `finalize_account_setup(ScreenId::Chat)`
- [ ] In `aura-web/src/harness/commands.rs`, change `finalize_account_setup(ScreenId::Neighborhood)` to `ScreenId::Chat`
- [ ] In `aura-web/src/shell_host.rs`, change `finalize_account_setup(ScreenId::Neighborhood)` to `ScreenId::Chat`
- [ ] In the TUI bootstrap handler (search for post-bootstrap screen navigation in `aura-terminal/src/tui/` and `aura-terminal/src/handlers/tui/`), update the post-bootstrap screen from Neighborhood to Chat
- [ ] Update any harness scenario contracts that assert post-bootstrap screen is Neighborhood
- [ ] Run `cargo check -p aura-web -p aura-terminal -p aura-harness` and verify green
- [ ] Run `just demo` and verify:
  - New account lands on Chat screen
  - Note-to-self channel is visible and functional
  - Neighborhood screen shows empty state with "Create Home" button
  - Pressing `e` on Note-to-self channel opens the edit modal and saving works
- [ ] Commit: "Navigate to Chat after account creation"

## Phase 5: Improve error messaging for home-dependent operations

Group channel creation requires a home context. Now that homes are not auto-created, this path needs a clear error.

### Tasks

- [ ] In `create_channel_with_authoritative_binding` (`messaging.rs`), when `current_home_context()` returns `NotFound`, return a user-facing error like "Create a home from the Neighborhood screen before creating group channels"
- [ ] In the TUI create-channel wizard, if no home exists, show the error as a toast instead of opening the wizard
- [ ] In the webapp create-channel flow, show the same error in the UI
- [ ] Run `cargo test -p aura-app` and verify green
- [ ] Run `just demo` and verify: attempting to create a group channel without a home shows a clear message
- [ ] Commit: "Show clear error when creating group channels without a home"

## Phase 6: Cleanup and documentation

### Tasks

- [ ] Remove `ensure_note_to_self_channel` from `aura-app/src/views/chat.rs` if no callers remain
- [ ] Update `docs/115_social_architecture.md`: clarify that homes are user-initiated, not auto-created. Remove or revise "In v1, each user belongs to exactly one home" (line 88). Add a note that home creation is a user choice for social participation.
- [ ] Update `docs/001_system_architecture.md` if it implies homes are created at account setup
- [ ] Update `docs/804_testing_guide.md`: the shared-flow coverage anchor for `real-runtime-mixed-startup-smoke.toml` should reflect that startup no longer creates a home. Update the note-to-self channel parity guidance (line 168) to reflect that Note-to-self is now a real AMP channel provisioned at bootstrap.
- [ ] Update `docs/801_hello_world_guide.md` if any bootstrap examples reference auto-home creation
- [ ] Update `crates/aura-app/ARCHITECTURE.md` if it documents bootstrap postconditions that include home creation
- [ ] Update any harness scenarios or test fixtures that depend on a home existing after bootstrap
- [ ] Run `just test` for the full test suite
- [ ] Run `just ci-shared-flow-policy` to verify shared flow contracts
- [ ] Commit: "Clean up unused Note-to-self helpers and update docs for user-initiated homes"
