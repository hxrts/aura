//! TUI Test Runner
//!
//! Executes key sequences against the TUI and captures output for verification.
//!
//! This module provides infrastructure for:
//! - Spawning the TUI in a tmux session for proper screen capture
//! - Sending key sequences with configurable delays
//! - Capturing terminal output via tmux capture-pane
//! - Timeout-based abort on hung states

use std::collections::HashMap;
use std::process::Command;
use std::time::Instant;

/// A key event to send to the TUI
#[derive(Debug, Clone)]
pub enum Key {
    /// Regular character
    Char(char),
    /// Enter/Return key
    Enter,
    /// Escape key
    Escape,
    /// Tab key
    Tab,
    /// Shift+Tab (backtab)
    BackTab,
    /// Backspace
    Backspace,
    /// Arrow keys
    Up,
    Down,
    Left,
    Right,
    /// Function keys
    F(u8),
    /// Ctrl+key combination
    Ctrl(char),
    /// Shift+key combination
    Shift(char),
    /// Number keys 1-9 for screen navigation
    Num(u8),
}

impl Key {
    /// Convert key to ANSI escape sequence for terminal
    pub fn to_ansi(&self) -> Vec<u8> {
        match self {
            Key::Char(c) => vec![*c as u8],
            Key::Enter => vec![13], // CR
            Key::Escape => vec![27], // ESC
            Key::Tab => vec![9],
            Key::BackTab => vec![27, 91, 90], // ESC [ Z
            Key::Backspace => vec![127],
            Key::Up => vec![27, 91, 65],    // ESC [ A
            Key::Down => vec![27, 91, 66],  // ESC [ B
            Key::Right => vec![27, 91, 67], // ESC [ C
            Key::Left => vec![27, 91, 68],  // ESC [ D
            Key::F(n) => {
                // F1-F4: ESC O P/Q/R/S, F5+: ESC [ 15~, etc.
                match n {
                    1 => vec![27, 79, 80],
                    2 => vec![27, 79, 81],
                    3 => vec![27, 79, 82],
                    4 => vec![27, 79, 83],
                    5 => vec![27, 91, 49, 53, 126],
                    _ => vec![],
                }
            }
            Key::Ctrl(c) => vec![(*c as u8) & 0x1f], // Ctrl makes it 0-31
            Key::Shift(c) => vec![c.to_ascii_uppercase() as u8],
            Key::Num(n) => vec![b'0' + n],
        }
    }

    /// Parse a string into a key sequence
    /// Supports: "a", "Enter", "Escape", "Tab", "1", "Ctrl+c", etc.
    pub fn parse(s: &str) -> Option<Key> {
        match s.to_lowercase().as_str() {
            "enter" | "return" => Some(Key::Enter),
            "escape" | "esc" => Some(Key::Escape),
            "tab" => Some(Key::Tab),
            "backtab" | "shift+tab" => Some(Key::BackTab),
            "backspace" | "bs" => Some(Key::Backspace),
            "up" => Some(Key::Up),
            "down" => Some(Key::Down),
            "left" => Some(Key::Left),
            "right" => Some(Key::Right),
            "space" => Some(Key::Char(' ')),
            _ => {
                // Check for Ctrl+X
                if s.starts_with("ctrl+") || s.starts_with("Ctrl+") {
                    let c = s.chars().last()?;
                    return Some(Key::Ctrl(c));
                }
                // Check for Shift+X
                if s.starts_with("shift+") || s.starts_with("Shift+") {
                    let c = s.chars().last()?;
                    return Some(Key::Shift(c));
                }
                // Check for F-keys
                if s.starts_with('f') || s.starts_with('F') {
                    if let Ok(n) = s[1..].parse::<u8>() {
                        return Some(Key::F(n));
                    }
                }
                // Single character
                if s.len() == 1 {
                    let c = s.chars().next()?;
                    if c.is_ascii_digit() {
                        return Some(Key::Num(c as u8 - b'0'));
                    }
                    return Some(Key::Char(c));
                }
                None
            }
        }
    }
}

/// Verification criteria for a test step
#[derive(Debug, Clone, Default)]
pub struct VerifyCriteria {
    /// Patterns that MUST appear in screen output (AND logic)
    pub expect_all: Vec<String>,
    /// At least one of these patterns must appear (OR logic)
    pub expect_any: Vec<String>,
    /// Patterns that must NOT appear in screen output
    pub reject: Vec<String>,
    /// Stage name for grouping (e.g., "Account Setup", "Import Alice")
    pub stage: Option<String>,
}

impl VerifyCriteria {
    pub fn new() -> Self {
        Self::default()
    }

    /// Strip ANSI escape codes from text for clean matching
    pub fn strip_ansi(text: &str) -> String {
        // Regex-free ANSI stripping - look for ESC sequences
        let mut result = String::with_capacity(text.len());
        let mut chars = text.chars().peekable();

        while let Some(c) = chars.next() {
            if c == '\x1b' {
                // Start of escape sequence - skip until we hit the end
                if let Some(&next) = chars.peek() {
                    if next == '[' {
                        chars.next(); // consume '['
                        // Skip until we hit a letter (the command) or the sequence ends
                        while let Some(&ch) = chars.peek() {
                            chars.next();
                            // CSI sequences end with a letter or ~ (for function keys)
                            if ch.is_ascii_alphabetic() || ch == '~' {
                                break;
                            }
                        }
                    } else if next == '(' || next == ')' {
                        // Character set selection sequences
                        chars.next();
                        chars.next(); // Skip the character set designator
                    } else if next == ']' {
                        // OSC (Operating System Command) sequence - skip until BEL or ST
                        chars.next(); // consume ']'
                        while let Some(&ch) = chars.peek() {
                            if ch == '\x07' {
                                chars.next();
                                break;
                            }
                            if ch == '\x1b' {
                                chars.next();
                                if chars.peek() == Some(&'\\') {
                                    chars.next();
                                    break;
                                }
                            }
                            chars.next();
                        }
                    } else if next == '?' || next == '=' || next == '>' {
                        // DEC private mode sequences
                        chars.next();
                    } else if next == 'c' || next == 'M' || next == 'D' || next == 'E' || next == '7' || next == '8' {
                        // Single character escape sequences
                        chars.next();
                    }
                }
            } else if c.is_control() && c != '\n' && c != '\t' {
                // Skip other control characters except newline and tab
                continue;
            } else {
                result.push(c);
            }
        }
        result
    }

    /// Check if criteria are satisfied by the given screen content
    pub fn check(&self, screen: &str) -> Result<(), String> {
        // Strip ANSI codes for cleaner matching
        let clean_screen = Self::strip_ansi(screen);

        // Check expect_all - all patterns must be present
        // Use case-insensitive matching for robustness
        let clean_lower = clean_screen.to_lowercase();
        for pattern in &self.expect_all {
            let pattern_lower = pattern.to_lowercase();
            if !clean_lower.contains(&pattern_lower) {
                return Err(format!("Expected text not found: '{}'", pattern));
            }
        }

        // Check expect_any - at least one must be present (if any specified)
        if !self.expect_any.is_empty() {
            let found_any = self.expect_any.iter().any(|p| clean_screen.contains(p));
            if !found_any {
                return Err(format!(
                    "None of expected texts found: {:?}",
                    self.expect_any
                ));
            }
        }

        // Check reject - none of these should be present
        for pattern in &self.reject {
            if clean_screen.contains(pattern) {
                return Err(format!("Unexpected text found: '{}'", pattern));
            }
        }

        Ok(())
    }
}

/// A step in a test sequence
#[derive(Debug, Clone)]
pub struct TestStep {
    /// Description of this step
    pub description: String,
    /// Key to send
    pub key: Key,
    /// Delay after sending key (ms)
    pub delay_ms: u64,
    /// Optional text to expect in output after this step (legacy, use verify for more control)
    pub expect: Option<String>,
    /// Timeout for this step (ms), 0 means use default
    pub timeout_ms: u64,
    /// Verification criteria for this step
    pub verify: VerifyCriteria,
}

impl TestStep {
    pub fn new(description: impl Into<String>, key: Key) -> Self {
        Self {
            description: description.into(),
            key,
            delay_ms: 100,
            expect: None,
            timeout_ms: 0,
            verify: VerifyCriteria::default(),
        }
    }

    pub fn with_delay(mut self, delay_ms: u64) -> Self {
        self.delay_ms = delay_ms;
        self
    }

    pub fn with_expect(mut self, text: impl Into<String>) -> Self {
        self.expect = Some(text.into());
        self
    }

    pub fn with_timeout(mut self, ms: u64) -> Self {
        self.timeout_ms = ms;
        self
    }

    /// Add a pattern that must be present (AND logic)
    pub fn must_see(mut self, pattern: impl Into<String>) -> Self {
        self.verify.expect_all.push(pattern.into());
        self
    }

    /// Add patterns where at least one must be present (OR logic)
    pub fn must_see_any(mut self, texts: Vec<String>) -> Self {
        self.verify.expect_any = texts;
        self
    }

    /// Add a pattern that must NOT be present
    pub fn must_not_see(mut self, text: impl Into<String>) -> Self {
        self.verify.reject.push(text.into());
        self
    }

    /// Mark this step as starting a new stage
    pub fn stage(mut self, name: impl Into<String>) -> Self {
        self.verify.stage = Some(name.into());
        self
    }
}

/// Result of a single test step
#[derive(Debug, Clone)]
pub struct StepResult {
    /// Index in the sequence
    pub step_index: usize,
    /// Description of the step
    pub description: String,
    /// Whether step succeeded
    pub success: bool,
    /// Captured screen content
    pub output_captured: String,
    /// Error message if failed
    pub error: Option<String>,
    /// How long this step took
    pub duration_ms: u64,
    /// Stage this step belongs to
    pub stage: Option<String>,
    /// Verification result (if criteria were specified)
    pub verification: Option<Result<(), String>>,
}

/// Result of a complete test run
#[derive(Debug)]
pub struct TestResult {
    /// Whether all steps succeeded
    pub success: bool,
    /// Results for each step
    pub steps: Vec<StepResult>,
    /// Total duration in ms
    pub total_duration_ms: u64,
    /// Final screen capture
    pub final_screenshot: String,
    /// Global error (if test couldn't run)
    pub error: Option<String>,
}

/// Configuration for the test runner
#[derive(Debug, Clone)]
pub struct RunnerConfig {
    /// Path to the aura binary
    pub binary_path: String,
    /// Arguments to pass
    pub args: Vec<String>,
    /// Default timeout per step (ms)
    pub default_timeout_ms: u64,
    /// Startup wait time (ms)
    pub startup_wait_ms: u64,
    /// Terminal width
    pub term_width: u16,
    /// Terminal height
    pub term_height: u16,
    /// Environment variables
    pub env: HashMap<String, String>,
}

impl Default for RunnerConfig {
    fn default() -> Self {
        Self {
            binary_path: "cargo".to_string(),
            args: vec![
                "run".to_string(),
                "-p".to_string(),
                "aura-terminal".to_string(),
                "--features".to_string(),
                "development".to_string(),
                "--".to_string(),
                "tui".to_string(),
                "--demo".to_string(),
            ],
            default_timeout_ms: 5000,
            startup_wait_ms: 5000, // Wait longer for TUI to fully initialize
            term_width: 120,
            term_height: 40,
            env: HashMap::new(),
        }
    }
}

impl RunnerConfig {
    /// Create config for demo mode
    pub fn demo() -> Self {
        Self::default()
    }

    /// Create config with custom binary path
    pub fn with_binary(mut self, path: impl Into<String>) -> Self {
        self.binary_path = path.into();
        self.args = vec!["tui".to_string(), "--demo".to_string()];
        self
    }
}

/// TUI Test Runner
///
/// Drives a TUI application through a sequence of key presses and
/// captures output for verification using tmux.
pub struct TuiTestRunner {
    config: RunnerConfig,
}

impl TuiTestRunner {
    pub fn new(config: RunnerConfig) -> Self {
        Self { config }
    }

    /// Generate a deterministic tmux session name
    /// Uses a fixed name for reproducibility - any stale session is cleaned up before use
    fn generate_session_name() -> String {
        "aura_tui_test".to_string()
    }

    /// Run a test sequence and return results
    ///
    /// This spawns the TUI in a tmux session, sends keys, and captures output.
    pub fn run_sequence(&self, steps: &[TestStep]) -> TestResult {
        let start = Instant::now();
        let session_name = Self::generate_session_name();

        // Clean demo data before running to ensure fresh state
        let demo_data_path = std::path::Path::new("./aura-demo-data");
        if demo_data_path.exists() {
            let _ = std::fs::remove_dir_all(demo_data_path);
        }

        // Kill any existing session with this name (cleanup from previous runs)
        let _ = Command::new("tmux")
            .args(["kill-session", "-t", &session_name])
            .output();

        // Generate and execute the bash script
        let script = self.generate_tmux_script(&session_name, steps);

        // Write script to temp file
        let script_path = std::env::temp_dir().join("aura_tui_test.sh");
        if let Err(e) = std::fs::write(&script_path, &script) {
            return TestResult {
                success: false,
                steps: vec![],
                total_duration_ms: start.elapsed().as_millis() as u64,
                final_screenshot: String::new(),
                error: Some(format!("Failed to write test script: {}", e)),
            };
        }

        // Make script executable
        let _ = Command::new("chmod")
            .args(["+x", script_path.to_str().unwrap()])
            .output();

        // Run the script
        let output = Command::new("bash")
            .arg(&script_path)
            .env("TERM", "xterm-256color")
            .output();

        // Clean up script
        let _ = std::fs::remove_file(&script_path);

        // Always cleanup tmux session
        let _ = Command::new("tmux")
            .args(["kill-session", "-t", &session_name])
            .output();

        match output {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();

                // Parse results from script output
                self.parse_tmux_output(&stdout, &stderr, steps, start.elapsed().as_millis() as u64)
            }
            Err(e) => {
                // Cleanup on error
                let _ = Command::new("tmux")
                    .args(["kill-session", "-t", &session_name])
                    .output();

                TestResult {
                    success: false,
                    steps: vec![],
                    total_duration_ms: start.elapsed().as_millis() as u64,
                    final_screenshot: String::new(),
                    error: Some(format!("Failed to run test script: {}", e)),
                }
            }
        }
    }

    /// Generate a bash script that uses tmux for the test
    fn generate_tmux_script(&self, session_name: &str, steps: &[TestStep]) -> String {
        let mut script = String::new();

        // Bash script header with strict error handling
        script.push_str("#!/bin/bash\n");
        script.push_str("set -e\n\n");

        // Store session name for cleanup
        script.push_str(&format!("SESSION=\"{}\"\n", session_name));
        script.push_str(&format!("TERM_WIDTH={}\n", self.config.term_width));
        script.push_str(&format!("TERM_HEIGHT={}\n", self.config.term_height));
        script.push_str("\n");

        // Cleanup function
        script.push_str("cleanup() {\n");
        script.push_str("    tmux kill-session -t \"$SESSION\" 2>/dev/null || true\n");
        script.push_str("}\n");
        script.push_str("trap cleanup EXIT\n\n");

        // Function to capture screen
        script.push_str("capture_screen() {\n");
        script.push_str("    local step_num=$1\n");
        script.push_str("    echo \"<<<SCREEN_START:${step_num}>>>\"\n");
        script.push_str("    tmux capture-pane -t \"$SESSION\" -p -e 2>/dev/null || true\n");
        script.push_str("    echo \"<<<SCREEN_END:${step_num}>>>\"\n");
        script.push_str("}\n\n");

        // Function to send keys
        script.push_str("send_key() {\n");
        script.push_str("    tmux send-keys -t \"$SESSION\" \"$1\"\n");
        script.push_str("}\n\n");

        // Function to log
        script.push_str("log() {\n");
        script.push_str("    echo \"=== $1 ===\"\n");
        script.push_str("}\n\n");

        // Start tmux session with the TUI command
        let cmd = format!("{} {}", self.config.binary_path, self.config.args.join(" "));
        script.push_str("log \"Starting TUI in tmux session\"\n");
        script.push_str(&format!(
            "tmux new-session -d -s \"$SESSION\" -x $TERM_WIDTH -y $TERM_HEIGHT \"{}\"\n",
            cmd.replace("\"", "\\\"")
        ));
        script.push_str("\n");

        // Wait for startup
        script.push_str(&format!(
            "sleep {}\n",
            self.config.startup_wait_ms as f64 / 1000.0
        ));
        script.push_str("\n");

        // Initial screen capture
        script.push_str("log \"Initial screen\"\n");
        script.push_str("capture_screen 0\n\n");

        // Execute each step
        for (i, step) in steps.iter().enumerate() {
            // Log stage if this step starts a new stage
            if let Some(ref stage) = step.verify.stage {
                script.push_str(&format!(
                    "log \"STAGE: {}\"\n",
                    stage.replace("\"", "\\\"")
                ));
            }

            script.push_str(&format!(
                "log \"Step {}: {}\"\n",
                i + 1,
                step.description.replace("\"", "\\\"")
            ));

            // Send the key
            let key_send = self.key_to_tmux_send(&step.key);
            script.push_str(&format!("send_key \"{}\"\n", key_send));

            // Wait for delay
            script.push_str(&format!(
                "sleep {}\n",
                step.delay_ms as f64 / 1000.0
            ));

            // Capture screen state
            script.push_str(&format!("capture_screen {}\n\n", i + 1));
        }

        // Final screenshot
        script.push_str("log \"Final screenshot\"\n");
        script.push_str("capture_screen final\n\n");

        // Send quit command
        script.push_str("log \"Sending quit\"\n");
        script.push_str("send_key \"q\"\n");
        script.push_str("sleep 0.5\n");

        // The cleanup trap will handle killing the session

        script
    }

    /// Convert a Key to tmux send-keys format
    fn key_to_tmux_send(&self, key: &Key) -> String {
        match key {
            Key::Char(c) => {
                // tmux send-keys interprets certain characters specially
                match c {
                    ' ' => "Space".to_string(),
                    '\'' => "'".to_string(),
                    '"' => "\\\"".to_string(),
                    '\\' => "\\\\".to_string(),
                    '$' => "\\$".to_string(),
                    '`' => "\\`".to_string(),
                    '\0' => "".to_string(), // No-op
                    _ => c.to_string(),
                }
            }
            Key::Enter => "Enter".to_string(),
            Key::Escape => "Escape".to_string(),
            Key::Tab => "Tab".to_string(),
            Key::BackTab => "BTab".to_string(),
            Key::Backspace => "BSpace".to_string(),
            Key::Up => "Up".to_string(),
            Key::Down => "Down".to_string(),
            Key::Right => "Right".to_string(),
            Key::Left => "Left".to_string(),
            Key::F(n) => format!("F{}", n),
            Key::Ctrl(c) => format!("C-{}", c),
            Key::Shift(c) => c.to_ascii_uppercase().to_string(),
            Key::Num(n) => format!("{}", n),
        }
    }

    /// Extract screen content between markers for a given step
    fn extract_screen_content(stdout: &str, step_num: usize) -> String {
        let start_marker = format!("<<<SCREEN_START:{}>>>", step_num);
        let end_marker = format!("<<<SCREEN_END:{}>>>", step_num);

        if let Some(start_pos) = stdout.find(&start_marker) {
            let content_start = start_pos + start_marker.len();
            if let Some(end_offset) = stdout[content_start..].find(&end_marker) {
                return stdout[content_start..content_start + end_offset]
                    .trim()
                    .to_string();
            }
        }
        String::new()
    }

    /// Extract screen content for the "final" marker
    fn extract_final_screen(stdout: &str) -> String {
        let start_marker = "<<<SCREEN_START:final>>>";
        let end_marker = "<<<SCREEN_END:final>>>";

        if let Some(start_pos) = stdout.find(start_marker) {
            let content_start = start_pos + start_marker.len();
            if let Some(end_offset) = stdout[content_start..].find(end_marker) {
                return stdout[content_start..content_start + end_offset]
                    .trim()
                    .to_string();
            }
        }
        String::new()
    }

    /// Parse tmux script output into test results
    fn parse_tmux_output(
        &self,
        stdout: &str,
        stderr: &str,
        steps: &[TestStep],
        total_ms: u64,
    ) -> TestResult {
        let mut step_results = Vec::new();
        let mut all_success = true;
        let mut current_stage: Option<String> = None;

        // Parse step results from output markers
        for (i, step) in steps.iter().enumerate() {
            // Update current stage if this step defines one
            if let Some(ref stage) = step.verify.stage {
                current_stage = Some(stage.clone());
            }

            // Extract screen content for this step
            let screen_content = Self::extract_screen_content(stdout, i + 1);

            // Run verification if criteria are defined
            let has_verify_criteria = !step.verify.expect_all.is_empty()
                || !step.verify.expect_any.is_empty()
                || !step.verify.reject.is_empty();

            let verification = if has_verify_criteria {
                Some(step.verify.check(&screen_content))
            } else {
                None
            };

            // Determine overall step success
            let verification_failed = matches!(&verification, Some(Err(_)));
            let step_success = !verification_failed;

            if !step_success {
                all_success = false;
            }

            // Build error message
            let error = if let Some(Err(ref verify_err)) = verification {
                Some(verify_err.clone())
            } else {
                None
            };

            step_results.push(StepResult {
                step_index: i,
                description: step.description.clone(),
                success: step_success,
                output_captured: screen_content,
                error,
                duration_ms: step.delay_ms,
                stage: current_stage.clone(),
                verification: verification.clone(),
            });
        }

        // Extract final screenshot
        let final_screenshot = Self::extract_final_screen(stdout);

        TestResult {
            success: all_success,
            steps: step_results,
            total_duration_ms: total_ms,
            final_screenshot,
            error: if stderr.is_empty() {
                None
            } else {
                // Only report stderr as error if it contains actual errors
                let stderr_lower = stderr.to_lowercase();
                if stderr_lower.contains("error") || stderr_lower.contains("failed") {
                    Some(stderr.to_string())
                } else {
                    None
                }
            },
        }
    }
}

/// Builder for creating test sequences
pub struct TestSequenceBuilder {
    steps: Vec<TestStep>,
}

impl TestSequenceBuilder {
    pub fn new() -> Self {
        Self { steps: Vec::new() }
    }

    /// Add a step to press a key
    pub fn press(mut self, description: impl Into<String>, key: Key) -> Self {
        self.steps.push(TestStep::new(description, key));
        self
    }

    /// Add a step with custom delay
    pub fn press_with_delay(
        mut self,
        description: impl Into<String>,
        key: Key,
        delay_ms: u64,
    ) -> Self {
        self.steps
            .push(TestStep::new(description, key).with_delay(delay_ms));
        self
    }

    /// Add a step that expects certain text in output
    pub fn press_expect(
        mut self,
        description: impl Into<String>,
        key: Key,
        expect: impl Into<String>,
    ) -> Self {
        self.steps
            .push(TestStep::new(description, key).with_expect(expect));
        self
    }

    /// Type a string (multiple character presses)
    pub fn type_text(mut self, description: impl Into<String>, text: &str) -> Self {
        let desc = description.into();
        for (i, c) in text.chars().enumerate() {
            let step_desc = if i == 0 {
                desc.clone()
            } else {
                format!("{} (continued)", desc)
            };
            self.steps
                .push(TestStep::new(step_desc, Key::Char(c)).with_delay(50));
        }
        self
    }

    /// Wait without pressing a key
    pub fn wait(mut self, description: impl Into<String>, ms: u64) -> Self {
        // Use a no-op - we'll just set a long delay on the next step
        // For now, add a dummy step with delay
        self.steps.push(
            TestStep::new(description, Key::Char('\0'))
                .with_delay(ms),
        );
        self
    }

    /// Add a step with verification that specified text must appear
    pub fn press_verify(
        mut self,
        description: impl Into<String>,
        key: Key,
        delay_ms: u64,
        must_see: &[&str],
    ) -> Self {
        let mut step = TestStep::new(description, key).with_delay(delay_ms);
        for text in must_see {
            step = step.must_see(*text);
        }
        self.steps.push(step);
        self
    }

    /// Add a step that starts a new stage with verification
    pub fn stage_start(
        mut self,
        stage_name: impl Into<String>,
        description: impl Into<String>,
        key: Key,
        delay_ms: u64,
        must_see: &[&str],
    ) -> Self {
        let stage = stage_name.into();
        let mut step = TestStep::new(description, key)
            .with_delay(delay_ms)
            .stage(stage);
        for text in must_see {
            step = step.must_see(*text);
        }
        self.steps.push(step);
        self
    }

    /// Add a step that verifies absence of patterns (must not see)
    pub fn press_reject(
        mut self,
        description: impl Into<String>,
        key: Key,
        delay_ms: u64,
        must_not_see: &[&str],
    ) -> Self {
        let mut step = TestStep::new(description, key).with_delay(delay_ms);
        for text in must_not_see {
            step = step.must_not_see(*text);
        }
        self.steps.push(step);
        self
    }

    /// Get a mutable reference to the last step for additional configuration
    pub fn last_step_mut(&mut self) -> Option<&mut TestStep> {
        self.steps.last_mut()
    }

    /// Build the sequence
    pub fn build(self) -> Vec<TestStep> {
        self.steps
    }
}

impl Default for TestSequenceBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_parse() {
        assert!(matches!(Key::parse("a"), Some(Key::Char('a'))));
        assert!(matches!(Key::parse("Enter"), Some(Key::Enter)));
        assert!(matches!(Key::parse("escape"), Some(Key::Escape)));
        assert!(matches!(Key::parse("1"), Some(Key::Num(1))));
        assert!(matches!(Key::parse("Ctrl+c"), Some(Key::Ctrl('c'))));
    }

    #[test]
    fn test_key_to_ansi() {
        assert_eq!(Key::Char('a').to_ansi(), vec![b'a']);
        assert_eq!(Key::Enter.to_ansi(), vec![13]);
        assert_eq!(Key::Escape.to_ansi(), vec![27]);
        assert_eq!(Key::Up.to_ansi(), vec![27, 91, 65]);
    }

    #[test]
    fn test_sequence_builder() {
        let steps = TestSequenceBuilder::new()
            .press("Press 1", Key::Num(1))
            .press_expect("Press Enter", Key::Enter, "expected")
            .type_text("Type hello", "hello")
            .build();

        assert_eq!(steps.len(), 7); // 1 + 1 + 5 chars
        assert!(matches!(steps[0].key, Key::Num(1)));
        assert!(steps[1].expect.is_some());
    }
}
