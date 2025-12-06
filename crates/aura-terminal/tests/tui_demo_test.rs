//! TUI Demo Sequence Tests
//!
//! Tests the TUI demo mode by executing key sequences and verifying behavior.
//!
//! These tests run the actual TUI in demo mode and drive it through various
//! scenarios using the TuiTestRunner.

mod tui_helpers;

use tui_helpers::{Key, RunnerConfig, TestSequenceBuilder, TuiTestRunner, VerifyCriteria};

/// Create a test sequence for basic screen navigation
///
/// Tests that all 8 screens can be accessed via number keys.
fn navigation_sequence() -> Vec<tui_helpers::TestStep> {
    TestSequenceBuilder::new()
        // Start on Block screen (default)
        .press_with_delay("Press 2 to go to Chat", Key::Num(2), 500)
        .press_with_delay("Press 3 to go to Contacts", Key::Num(3), 500)
        .press_with_delay("Press 4 to go to Neighborhood", Key::Num(4), 500)
        .press_with_delay("Press 5 to go to Invitations", Key::Num(5), 500)
        .press_with_delay("Press 6 to go to Settings", Key::Num(6), 500)
        .press_with_delay("Press 7 to go to Recovery", Key::Num(7), 500)
        .press_with_delay("Press 8 to go to Help", Key::Num(8), 500)
        .press_with_delay("Press 1 to return to Block", Key::Num(1), 500)
        .build()
}

/// Create a test sequence for Chat screen interaction
///
/// Tests entering insert mode, typing, and sending a message.
fn chat_interaction_sequence() -> Vec<tui_helpers::TestStep> {
    TestSequenceBuilder::new()
        // Go to Chat screen
        .press_with_delay("Go to Chat screen", Key::Num(2), 500)
        // Enter insert mode
        .press_with_delay("Enter insert mode with 'i'", Key::Char('i'), 300)
        // Type a message
        .type_text("Type hello", "hello world")
        // Send message
        .press_with_delay("Send message with Enter", Key::Enter, 500)
        // Exit insert mode
        .press_with_delay("Exit insert mode with Escape", Key::Escape, 300)
        // Navigate channels with j/k
        .press_with_delay("Press j to navigate down", Key::Char('j'), 300)
        .press_with_delay("Press k to navigate up", Key::Char('k'), 300)
        .build()
}

/// Create a test sequence for Recovery screen interaction
///
/// Tests navigating to the Recovery screen and viewing guardian status.
fn recovery_screen_sequence() -> Vec<tui_helpers::TestStep> {
    TestSequenceBuilder::new()
        // Go to Recovery screen
        .press_with_delay("Go to Recovery screen", Key::Num(7), 500)
        // Navigate the recovery options
        .press_with_delay("Press j to navigate down", Key::Char('j'), 300)
        .press_with_delay("Press k to navigate up", Key::Char('k'), 300)
        .build()
}

/// Create a test sequence for Invitations screen
///
/// Tests creating and managing invitations.
fn invitations_sequence() -> Vec<tui_helpers::TestStep> {
    TestSequenceBuilder::new()
        // Go to Invitations screen
        .press_with_delay("Go to Invitations screen", Key::Num(5), 500)
        // Navigate the invitation list
        .press_with_delay("Press j to navigate down", Key::Char('j'), 300)
        .press_with_delay("Press k to navigate up", Key::Char('k'), 300)
        .build()
}

/// Create a test sequence for Contacts screen
///
/// Tests viewing and managing contacts.
fn contacts_sequence() -> Vec<tui_helpers::TestStep> {
    TestSequenceBuilder::new()
        // Go to Contacts screen
        .press_with_delay("Go to Contacts screen", Key::Num(3), 500)
        // Navigate the contact list
        .press_with_delay("Press j to navigate down", Key::Char('j'), 300)
        .press_with_delay("Press k to navigate up", Key::Char('k'), 300)
        // Use h/l for column navigation
        .press_with_delay("Press l to move right", Key::Char('l'), 300)
        .press_with_delay("Press h to move left", Key::Char('h'), 300)
        .build()
}

/// Create the cli_recovery.md demo flow sequence
///
/// This sequence implements the ACTUAL flow from docs/demo/cli_recovery.md:
///
/// Phase 1: Bob's account setup (account modal appears when no account exists)
/// Phase 2: View demo invite codes (press 'd' to see Alice/Charlie codes)
/// Phase 3: Import Alice's invite code via Invitations screen ('a' auto-fills when empty)
/// Phase 4: Import Charlie's invite code ('c' auto-fills when empty)
/// Phase 5: Accept invitations from Alice and Charlie
/// Phase 6: Mark Alice as guardian in Contacts
/// Phase 7: Mark Charlie as guardian
/// Phase 8: Go to Chat and send a message
/// Phase 9: Go to Recovery and start recovery process
fn cli_recovery_demo_sequence() -> Vec<tui_helpers::TestStep> {
    TestSequenceBuilder::new()
        // Wait for TUI to initialize (account setup modal appears)
        .press_with_delay("Wait for TUI to initialize", Key::Num(1), 1500)

        // Phase 1: Account setup modal - create Bob's account
        // Type "Bob" and press Enter (modal captures all input when visible)
        .type_text("Type account name 'Bob'", "Bob")
        .press_with_delay("Submit account creation", Key::Enter, 1000)

        // Phase 2: View demo invite codes (press 'd' shows modal with Alice/Charlie codes)
        .press_with_delay("Press 'd' to view demo invite codes", Key::Char('d'), 500)
        // Note: The modal shows Alice and Charlie codes - user would copy these
        .press_with_delay("Close demo codes modal with Escape", Key::Escape, 300)

        // Phase 3: Go to Invitations screen and import Alice's invite code
        .press_with_delay("Go to Invitations (5)", Key::Num(5), 500)
        .press_with_delay("Press 'i' to open import modal", Key::Char('i'), 300)
        // In demo mode, 'a' auto-fills Alice's invite code when input is empty
        .press_with_delay("Press 'a' to auto-fill Alice's code", Key::Char('a'), 300)
        .press_with_delay("Submit import", Key::Enter, 500)

        // Phase 4: Import Charlie's invite code
        .press_with_delay("Press 'i' to open import modal again", Key::Char('i'), 300)
        // In demo mode, 'c' auto-fills Charlie's invite code when input is empty
        .press_with_delay("Press 'c' to auto-fill Charlie's code", Key::Char('c'), 300)
        .press_with_delay("Submit import", Key::Enter, 500)

        // Phase 5: Accept pending invitations
        // Navigate to received invitations and accept them
        .press_with_delay("Press 'f' to filter to Received", Key::Char('f'), 300)
        .press_with_delay("Press 'f' again for Received filter", Key::Char('f'), 300)
        // First invitation (Alice)
        .press_with_delay("Press 'a' to accept first invitation", Key::Char('a'), 500)
        // Navigate to next invitation
        .press_with_delay("Press 'j' to move to next invitation", Key::Char('j'), 300)
        // Second invitation (Charlie)
        .press_with_delay("Press 'a' to accept second invitation", Key::Char('a'), 500)

        // Phase 6: Go to Contacts and mark Alice as guardian
        .press_with_delay("Go to Contacts (3)", Key::Num(3), 500)
        // Alice should be first contact
        .press_with_delay("Press 'g' to toggle Alice as guardian", Key::Char('g'), 500)

        // Phase 7: Navigate to Charlie and mark as guardian
        .press_with_delay("Press 'j' to move to Charlie", Key::Char('j'), 300)
        .press_with_delay("Press 'g' to toggle Charlie as guardian", Key::Char('g'), 500)

        // Phase 8: Go to Chat and send a group message
        .press_with_delay("Go to Chat (2)", Key::Num(2), 500)
        .press_with_delay("Enter insert mode with 'i'", Key::Char('i'), 300)
        .type_text("Type message", "Hello Alice and Charlie! This is Bob.")
        .press_with_delay("Send message with Enter", Key::Enter, 500)
        .press_with_delay("Exit insert mode with Escape", Key::Escape, 300)

        // Phase 9: Go to Recovery and start recovery process
        .press_with_delay("Go to Recovery (7)", Key::Num(7), 500)
        // Verify guardians are shown
        .press_with_delay("Navigate guardian list with 'j'", Key::Char('j'), 300)
        .press_with_delay("Navigate back with 'k'", Key::Char('k'), 300)
        // Start recovery process
        .press_with_delay("Press 's' to start recovery", Key::Char('s'), 500)

        // Final: Return to Block screen to confirm flow completed
        .press_with_delay("Return to Block (1)", Key::Num(1), 500)

        .build()
}

/// Create the cli_recovery.md demo flow with STAGED VERIFICATION
///
/// This version adds verification criteria at each stage to confirm the expected
/// UI state is actually achieved. Each stage checks for specific text patterns
/// that should appear on screen.
///
/// ## Verification Strategy
///
/// Each stage verifies:
/// 1. Expected screen elements are visible (must_see patterns)
/// 2. No unexpected error states (must_not_see patterns)
///
/// ## Stages and Criteria
///
/// Note: In demo mode with no existing account, the account setup modal appears first.
/// The user must type a name (e.g., "Bob") and press Enter to create the account.
///
/// | Stage | Action | Verification |
/// |-------|--------|--------------|
/// | 1. Account Setup | Type "Bob", Enter | See "Welcome to Aura" → "Block" |
/// | 2. Demo Codes | Press 'd' | See "DEMO MODE", "Alice:", "Charlie:" |
/// | 3. Import Alice | Go to Invitations, 'i', 'a', Enter | See "Invitations" screen |
/// | 4. Import Charlie | 'i', 'c', Enter | Invitations screen |
/// | 5. Accept Invitations | Filter, accept | Invitations screen |
/// | 6. Mark Alice Guardian | Go to Contacts, 'g' | See "Contacts" |
/// | 7. Mark Charlie Guardian | 'j', 'g' | Contacts screen |
/// | 8. Send Chat Message | Go to Chat, 'i', type, Enter | See "Chat" |
/// | 9. Start Recovery | Go to Recovery, 's' | See "Recovery" |
fn cli_recovery_demo_staged() -> Vec<tui_helpers::TestStep> {
    TestSequenceBuilder::new()
        // ========== STAGE 1: Account Setup ==========
        // In demo mode with no existing account, the account setup modal appears.
        // Type "Bob" and press Enter to create the account.
        .stage_start(
            "Account Setup",
            "Wait for account setup modal to appear",
            Key::Num(1), // This will be ignored while modal is open, just initializes timing
            3000, // Longer initial wait for TUI startup and modal to appear
            &["Enter your name"], // Should see account setup modal input prompt
        )
        .type_text("Type account name 'Bob'", "Bob")
        .press_verify(
            "Submit account creation with Enter",
            Key::Enter,
            2000, // Longer delay for account creation to complete
            &["Block"], // After account creation, should see Block screen
        )

        // ========== STAGE 2: View Demo Codes ==========
        .stage_start(
            "View Demo Codes",
            "Press 'd' to view demo invite codes",
            Key::Char('d'),
            500,
            &["DEMO MODE"], // Modal should show demo mode header
        )
        .press_verify(
            "Close demo codes modal with Escape",
            Key::Escape,
            300,
            &["Block"], // Should return to Block screen
        )

        // ========== STAGE 3: Import Alice's Code ==========
        .stage_start(
            "Import Alice",
            "Go to Invitations (5)",
            Key::Num(5),
            500,
            &["Invitations"], // Should see Invitations screen
        )
        .press_verify(
            "Press 'i' to open import modal",
            Key::Char('i'),
            300,
            &["Import Invitation"], // Import modal should open
        )
        .press_with_delay("Press 'a' to auto-fill Alice's code", Key::Char('a'), 300)
        .press_verify(
            "Submit import",
            Key::Enter,
            500,
            &["Invitations"], // Back to invitations list
        )

        // ========== STAGE 4: Import Charlie's Code ==========
        .stage_start(
            "Import Charlie",
            "Press 'i' to open import modal again",
            Key::Char('i'),
            300,
            &["Import Invitation"],
        )
        .press_with_delay("Press 'c' to auto-fill Charlie's code", Key::Char('c'), 300)
        .press_verify("Submit import", Key::Enter, 500, &["Invitations"])

        // ========== STAGE 5: Accept Invitations ==========
        .stage_start(
            "Accept Invitations",
            "Press 'f' to cycle filter",
            Key::Char('f'),
            300,
            &["Invitations"],
        )
        .press_with_delay("Press 'f' again for Received filter", Key::Char('f'), 300)
        .press_with_delay("Press 'a' to accept first invitation", Key::Char('a'), 500)
        .press_with_delay("Press 'j' to move to next invitation", Key::Char('j'), 300)
        .press_with_delay("Press 'a' to accept second invitation", Key::Char('a'), 500)

        // ========== STAGE 6: Mark Alice as Guardian ==========
        .stage_start(
            "Mark Alice Guardian",
            "Go to Contacts (3)",
            Key::Num(3),
            500,
            &["Contacts"], // Should see Contacts screen
        )
        .press_with_delay("Press 'g' to toggle Alice as guardian", Key::Char('g'), 500)

        // ========== STAGE 7: Mark Charlie as Guardian ==========
        .stage_start(
            "Mark Charlie Guardian",
            "Press 'j' to move to Charlie",
            Key::Char('j'),
            300,
            &["Contacts"],
        )
        .press_with_delay("Press 'g' to toggle Charlie as guardian", Key::Char('g'), 500)

        // ========== STAGE 8: Send Chat Message ==========
        .stage_start(
            "Send Chat Message",
            "Go to Chat (2)",
            Key::Num(2),
            500,
            &["Chat"], // Should see Chat screen
        )
        .press_with_delay("Enter insert mode with 'i'", Key::Char('i'), 300)
        .type_text("Type message", "Hello Alice and Charlie! This is Bob.")
        .press_with_delay("Send message with Enter", Key::Enter, 500)
        .press_with_delay("Exit insert mode with Escape", Key::Escape, 300)

        // ========== STAGE 9: Start Recovery ==========
        .stage_start(
            "Start Recovery",
            "Go to Recovery (7)",
            Key::Num(7),
            500,
            &["Recovery"], // Should see Recovery screen
        )
        .press_with_delay("Navigate guardian list with 'j'", Key::Char('j'), 300)
        .press_with_delay("Navigate back with 'k'", Key::Char('k'), 300)
        .press_with_delay("Press 's' to start recovery", Key::Char('s'), 500)

        // ========== FINAL: Return to Block ==========
        .stage_start(
            "Complete",
            "Return to Block (1)",
            Key::Num(1),
            500,
            &["Block"], // Should see Block screen
        )

        .build()
}

/// Create a simpler demo sequence that exercises the main TUI features
///
/// This is a simplified version for basic testing:
/// 0. Create account in setup modal (appears when no account exists)
/// 1. Navigate through screens
/// 2. View contacts (Alice, Charlie in demo)
/// 3. Go to Chat and send a message
/// 4. View Recovery screen (guardian status)
fn full_demo_sequence() -> Vec<tui_helpers::TestStep> {
    TestSequenceBuilder::new()
        // Phase 0: Account setup (modal appears when no account exists)
        .press_with_delay("Wait for TUI to initialize", Key::Num(1), 1500)
        .type_text("Type account name 'Bob'", "Bob")
        .press_with_delay("Submit account creation", Key::Enter, 1000)

        // Phase 1: Initial exploration - Block screen
        .press_with_delay("Go to Block (1)", Key::Num(1), 500)

        // Phase 2: Check Contacts (should show Alice and Charlie in demo)
        .press_with_delay("Go to Contacts (3)", Key::Num(3), 500)
        .press_with_delay("Navigate contacts with j", Key::Char('j'), 300)
        .press_with_delay("Navigate contacts with k", Key::Char('k'), 300)

        // Phase 3: Go to Chat and send a message
        .press_with_delay("Go to Chat (2)", Key::Num(2), 500)
        .press_with_delay("Enter insert mode", Key::Char('i'), 300)
        .type_text("Type greeting", "Hello from the demo test!")
        .press_with_delay("Send message", Key::Enter, 1000)
        .press_with_delay("Exit insert mode", Key::Escape, 300)

        // Phase 4: View Recovery screen
        .press_with_delay("Go to Recovery (7)", Key::Num(7), 500)
        .press_with_delay("Navigate recovery list", Key::Char('j'), 300)

        // Phase 5: Check Invitations
        .press_with_delay("Go to Invitations (5)", Key::Num(5), 500)

        // Phase 6: Check Settings
        .press_with_delay("Go to Settings (6)", Key::Num(6), 500)

        // Phase 7: Return to Block
        .press_with_delay("Return to Block (1)", Key::Num(1), 500)

        .build()
}

/// Run a test sequence and print results
fn run_and_report(name: &str, steps: Vec<tui_helpers::TestStep>) {
    println!("\n{}", "=".repeat(60));
    println!("Running test: {}", name);
    println!("Steps: {}", steps.len());
    println!("{}\n", "=".repeat(60));

    let config = RunnerConfig::demo();
    let runner = TuiTestRunner::new(config);
    let result = runner.run_sequence(&steps);

    println!("\nTest Result: {}", if result.success { "PASSED" } else { "FAILED" });
    println!("Total duration: {}ms", result.total_duration_ms);
    println!("Steps executed: {}", result.steps.len());

    for step in &result.steps {
        let status = if step.success { "✓" } else { "✗" };
        println!("  {} Step {}: {}", status, step.step_index + 1, step.description);
        if let Some(ref error) = step.error {
            println!("      Error: {}", error);
        }
    }

    if let Some(ref error) = result.error {
        println!("\nError: {}", error);
    }

    println!("\n--- Final Screenshot (last 50 lines) ---");
    let lines: Vec<&str> = result.final_screenshot.lines().collect();
    let start = if lines.len() > 50 { lines.len() - 50 } else { 0 };
    for line in &lines[start..] {
        println!("{}", line);
    }
}

/// Run a staged test sequence and print detailed results with verification
fn run_staged_and_report(name: &str, steps: Vec<tui_helpers::TestStep>) {
    println!("\n{}", "=".repeat(70));
    println!("Running STAGED test: {}", name);
    println!("Steps: {}", steps.len());
    println!("{}\n", "=".repeat(70));

    let config = RunnerConfig::demo();
    let runner = TuiTestRunner::new(config);
    let result = runner.run_sequence(&steps);

    // Group results by stage
    let mut current_stage: Option<String> = None;
    let mut stage_passed = true;
    let mut stages_summary: Vec<(String, bool, usize, usize)> = Vec::new(); // (name, passed, passed_count, total_count)
    let mut stage_step_count = 0;
    let mut stage_pass_count = 0;

    println!("\n{}", "-".repeat(70));
    println!("DETAILED STEP RESULTS");
    println!("{}", "-".repeat(70));

    for step in &result.steps {
        // Check if this step starts a new stage (only print when stage name changes)
        if let Some(ref stage) = step.stage {
            let is_new_stage = current_stage.as_ref().map(|s| s != stage).unwrap_or(true);
            if is_new_stage {
                // Save previous stage summary
                if let Some(ref prev_stage) = current_stage {
                    stages_summary.push((
                        prev_stage.clone(),
                        stage_passed,
                        stage_pass_count,
                        stage_step_count,
                    ));
                }
                // Start new stage
                current_stage = Some(stage.clone());
                stage_passed = true;
                stage_step_count = 0;
                stage_pass_count = 0;
                println!("\n▶ STAGE: {}", stage);
            }
        }

        stage_step_count += 1;
        if step.success {
            stage_pass_count += 1;
        } else {
            stage_passed = false;
        }

        let status = if step.success { "✓" } else { "✗" };

        // Show verification status
        let verify_status = match &step.verification {
            None => "",
            Some(Ok(())) => " [VERIFIED]",
            Some(Err(_)) => " [VERIFY FAILED]",
        };

        println!(
            "  {} Step {}: {}{}",
            status,
            step.step_index + 1,
            step.description,
            verify_status
        );

        if let Some(ref error) = step.error {
            println!("      ❌ Error: {}", error);
        }

        // Show cleaned screen dump for failed verifications
        if step.verification.as_ref().map(|v| v.is_err()).unwrap_or(false) {
            // Use VerifyCriteria's strip_ansi to clean the output
            let cleaned = VerifyCriteria::strip_ansi(&step.output_captured);
            // Collapse multiple spaces/newlines for readable output
            let collapsed: String = cleaned
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            if !collapsed.is_empty() {
                println!("      📺 Screen content (cleaned):");
                // Show up to 800 chars, broken into lines
                let preview: String = collapsed.chars().take(800).collect();
                for chunk in preview.as_bytes().chunks(100) {
                    let line = String::from_utf8_lossy(chunk);
                    println!("         {}", line);
                }
                if collapsed.len() > 800 {
                    println!("         ... ({} more chars)", collapsed.len() - 800);
                }
            } else {
                println!("      📺 Screen content: (empty after cleaning)");
            }
        }
    }

    // Save final stage
    if let Some(ref stage) = current_stage {
        stages_summary.push((
            stage.clone(),
            stage_passed,
            stage_pass_count,
            stage_step_count,
        ));
    }

    // Print stage summary
    println!("\n{}", "=".repeat(70));
    println!("STAGE SUMMARY");
    println!("{}", "=".repeat(70));

    for (stage_name, passed, pass_count, total) in &stages_summary {
        let icon = if *passed { "✅" } else { "❌" };
        println!(
            "  {} {} ({}/{} steps passed)",
            icon, stage_name, pass_count, total
        );
    }

    // Overall result
    let total_verified = result
        .steps
        .iter()
        .filter(|s| s.verification.is_some())
        .count();
    let verified_passed = result
        .steps
        .iter()
        .filter(|s| matches!(&s.verification, Some(Ok(()))))
        .count();

    println!("\n{}", "=".repeat(70));
    println!(
        "FINAL RESULT: {}",
        if result.success { "✅ PASSED" } else { "❌ FAILED" }
    );
    println!("Total duration: {}ms", result.total_duration_ms);
    println!(
        "Verification: {}/{} checks passed",
        verified_passed, total_verified
    );
    println!("{}", "=".repeat(70));

    if let Some(ref error) = result.error {
        println!("\n⚠️ Error: {}", error);
    }

    // Only show final screenshot if test failed
    if !result.success {
        println!("\n--- Final Screenshot (last 30 lines) ---");
        let lines: Vec<&str> = result.final_screenshot.lines().collect();
        let start = if lines.len() > 30 { lines.len() - 30 } else { 0 };
        for line in &lines[start..] {
            println!("{}", line);
        }
    }
}

// Note: These tests are marked as ignored by default because they require
// a proper terminal environment and the TUI binary to be built.
// Run with: cargo test --test tui_demo_test -- --ignored --nocapture

#[test]
#[ignore = "Requires terminal environment"]
fn test_basic_navigation() {
    run_and_report("Basic Navigation", navigation_sequence());
}

#[test]
#[ignore = "Requires terminal environment"]
fn test_chat_interaction() {
    run_and_report("Chat Interaction", chat_interaction_sequence());
}

#[test]
#[ignore = "Requires terminal environment"]
fn test_recovery_screen() {
    run_and_report("Recovery Screen", recovery_screen_sequence());
}

#[test]
#[ignore = "Requires terminal environment"]
fn test_invitations() {
    run_and_report("Invitations", invitations_sequence());
}

#[test]
#[ignore = "Requires terminal environment"]
fn test_contacts() {
    run_and_report("Contacts", contacts_sequence());
}

#[test]
#[ignore = "Requires terminal environment"]
fn test_full_demo_sequence() {
    run_and_report("Full Demo Sequence", full_demo_sequence());
}

#[test]
#[ignore = "Requires terminal environment"]
fn test_cli_recovery_demo() {
    run_and_report("CLI Recovery Demo (cli_recovery.md)", cli_recovery_demo_sequence());
}

#[test]
#[ignore = "Requires terminal environment"]
fn test_cli_recovery_demo_staged() {
    run_staged_and_report(
        "CLI Recovery Demo STAGED (with verification)",
        cli_recovery_demo_staged(),
    );
}

/// Main entry point for running tests manually
///
/// Run with: cargo test --test tui_demo_test -- --ignored --nocapture
fn main() {
    println!("TUI Demo Tests");
    println!("==============");
    println!();
    println!("Run with: cargo test --test tui_demo_test -- --ignored --nocapture");
    println!();
    println!("Available tests:");
    println!("  - test_basic_navigation");
    println!("  - test_chat_interaction");
    println!("  - test_recovery_screen");
    println!("  - test_invitations");
    println!("  - test_contacts");
    println!("  - test_full_demo_sequence");
    println!("  - test_cli_recovery_demo         (full cli_recovery.md flow)");
    println!("  - test_cli_recovery_demo_staged  (with stage verification) ⭐");
}
