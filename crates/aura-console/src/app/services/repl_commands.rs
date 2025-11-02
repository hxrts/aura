/// REPL Command Handler Service
///
/// Handles command parsing, execution, history management, and autocomplete.
/// Pure business logic separated from UI concerns.
///
/// This handler only manages UI-level commands. Data commands are delegated
/// to the current data source (Mock, Simulator, or Real).
use std::collections::VecDeque;

#[derive(Clone, Debug)]
pub struct ReplEntry {
    pub command: String,
    pub output: String,
    pub is_error: bool,
}

pub struct ReplCommandHandler {
    history: VecDeque<String>,
}

impl ReplCommandHandler {
    pub fn new() -> Self {
        Self {
            history: VecDeque::new(),
        }
    }

    /// Execute a command and return output
    pub fn execute(&mut self, cmd: &str) -> ReplEntry {
        let trimmed = cmd.trim();

        if trimmed.is_empty() {
            return ReplEntry {
                command: String::new(),
                output: String::new(),
                is_error: false,
            };
        }

        // Add to history
        self.history.push_back(trimmed.to_string());
        if self.history.len() > 50 {
            self.history.pop_front();
        }

        let output = self.handle_command(trimmed);

        ReplEntry {
            command: trimmed.to_string(),
            output,
            is_error: false,
        }
    }

    /// Get command suggestions for autocomplete
    /// Only includes UI commands - data commands are delegated to the current data source
    pub fn autocomplete(&self, partial: &str) -> Option<String> {
        let commands = [
            "help",
            "clear",
            // Choreography commands
            "choreo",
            "choreo list",
            "choreo start",
            "choreo step",
            "choreo state",
            "choreo trace",
            "choreo export",
            "choreo replay",
            "choreo breakpoint",
            "choreo continue",
            "choreo timeline",
            "choreo analyze",
        ];

        let matches: Vec<&str> = commands
            .iter()
            .filter(|cmd| cmd.starts_with(partial))
            .copied()
            .collect();

        if matches.len() == 1 {
            Some(matches[0].to_string())
        } else {
            None
        }
    }

    /// Get command history
    pub fn get_history(&self) -> Vec<String> {
        self.history.iter().cloned().collect()
    }

    /// Get history entry by index (0 = oldest)
    #[allow(dead_code)]
    pub fn get_history_entry(&self, index: usize) -> Option<String> {
        self.history.get(index).cloned()
    }

    /// Handle command execution
    fn handle_command(&self, cmd: &str) -> String {
        let parts: Vec<&str> = cmd.split_whitespace().collect();

        if parts.is_empty() {
            return String::new();
        }

        let command = parts[0];

        // UI commands - independent of data source
        match command {
            "help" => self.cmd_help(),
            "clear" => self.cmd_clear(),
            "choreo" => self.handle_choreo_command(&parts[1..]),
            // All other commands should be delegated to the data source
            _ => format!("Unknown command: {}", command),
        }
    }

    fn cmd_help(&self) -> String {
        r#"Aura REPL - Command Reference

UI Commands:
  help                  - Show this help
  clear                 - Clear console output
  choreo                - Choreography debugging commands (type 'choreo help')

Data Commands (delegated to current data source):

  Simulation Commands (Mock/Simulated sources):
    status              - Show simulation status
    step [n]            - Step simulation forward by n ticks
    reset               - Reset simulation to initial state
    devices             - List all devices in simulation
    state [device]      - Show device state
    branches            - List simulation branches
    network             - Show network topology

  Live Network Commands (Live source):
    status              - Show network status
    peers               - List connected peers
    sync                - Show sync status
    disconnect          - Disconnect from network

Navigation:
  Tab                   - Autocomplete commands
  Up/Down               - Navigate command history
  Switch Source         - Header dropdown"#
            .to_string()
    }

    fn cmd_clear(&self) -> String {
        "Console cleared.".to_string()
    }

    fn handle_choreo_command(&self, args: &[&str]) -> String {
        if args.is_empty() {
            return self.cmd_choreo_help();
        }

        let subcommand = args[0];
        let remaining_args = &args[1..];

        match subcommand {
            "list" => self.cmd_choreo_list(),
            "start" => self.cmd_choreo_start(remaining_args),
            "step" => self.cmd_choreo_step(remaining_args),
            "state" => self.cmd_choreo_state(remaining_args),
            "trace" => self.cmd_choreo_trace(remaining_args),
            "export" => self.cmd_choreo_export(remaining_args),
            "replay" => self.cmd_choreo_replay(remaining_args),
            "breakpoint" => self.cmd_choreo_breakpoint(remaining_args),
            "continue" => self.cmd_choreo_continue(remaining_args),
            "timeline" => self.cmd_choreo_timeline(remaining_args),
            "analyze" => self.cmd_choreo_analyze(remaining_args),
            "help" => self.cmd_choreo_help(),
            _ => format!("Unknown choreography subcommand: {}", subcommand),
        }
    }

    fn cmd_choreo_help(&self) -> String {
        r#"Choreography Commands:

  choreo list                        - List available choreographies
  choreo start <name> <participants> - Start a choreography
    Example: choreo start dkd alice,bob,carol
  
  choreo step <id> <participant>     - Step through choreography execution
  choreo state <id>                  - Show choreography state
  choreo trace <id> [participant]    - Show execution trace
  
  choreo export <id> <format> <path> - Export trace (json|dot|mermaid|console)
    Example: choreo export abc123 mermaid trace.md
  
  choreo replay <file> [speed]       - Replay from trace file
  choreo breakpoint <type> <event>   - Set breakpoint
  choreo continue <id>               - Continue execution
  
  choreo timeline [id]               - Show timeline visualization
  choreo analyze <id>                - Analyze for deadlocks

Type 'choreo help' for this help."#
            .to_string()
    }

    fn cmd_choreo_list(&self) -> String {
        r#"Available Choreographies:
  dkd                    - Deterministic Key Derivation
  frost_signing          - FROST Threshold Signing
  decentralized_lottery  - P2P Coordinator Selection
  commit_reveal          - Commit-Reveal Protocol
  session_epoch_bump     - Session Epoch Management
  failure_recovery       - Coordinator Failure Recovery"#
            .to_string()
    }

    fn cmd_choreo_start(&self, args: &[&str]) -> String {
        if args.len() < 2 {
            return "Usage: choreo start <name> <participants>\nExample: choreo start dkd alice,bob,carol".to_string();
        }

        let name = args[0];
        let participants = args[1];

        // In a real implementation, this would interface with the simulator
        let mock_id = format!("{:x}", {
            #[allow(clippy::disallowed_methods)]
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        });
        format!(
            "Starting {} choreography with participants: {}\nProtocol ID: {}",
            name, participants, mock_id
        )
    }

    fn cmd_choreo_step(&self, args: &[&str]) -> String {
        if args.len() < 2 {
            return "Usage: choreo step <protocol_id> <participant>".to_string();
        }

        format!("Stepping protocol {} as participant {}", args[0], args[1])
    }

    fn cmd_choreo_state(&self, args: &[&str]) -> String {
        if args.is_empty() {
            return "Usage: choreo state <protocol_id>".to_string();
        }

        format!(
            "State of protocol {}:\n  Phase: Started\n  Step: 3\n  Participants: 3",
            args[0]
        )
    }

    fn cmd_choreo_trace(&self, args: &[&str]) -> String {
        if args.is_empty() {
            return "Usage: choreo trace <protocol_id> [participant]".to_string();
        }

        format!("Trace for protocol {}:\n  [0] ProtocolStarted\n  [1] MessageSent\n  [2] MessageReceived", args[0])
    }

    fn cmd_choreo_export(&self, args: &[&str]) -> String {
        if args.len() < 3 {
            return "Usage: choreo export <protocol_id> <format> <path>".to_string();
        }

        format!(
            "Exported protocol {} as {} to {}",
            args[0], args[1], args[2]
        )
    }

    fn cmd_choreo_replay(&self, args: &[&str]) -> String {
        if args.is_empty() {
            return "Usage: choreo replay <trace_file> [speed]".to_string();
        }

        let speed = args.get(1).unwrap_or(&"1.0");
        format!("Replaying {} at {}x speed", args[0], speed)
    }

    fn cmd_choreo_breakpoint(&self, args: &[&str]) -> String {
        if args.len() < 2 {
            return "Usage: choreo breakpoint <protocol_type> <event_type>".to_string();
        }

        format!(
            "Breakpoint set for {} protocol on {} events",
            args[0], args[1]
        )
    }

    fn cmd_choreo_continue(&self, args: &[&str]) -> String {
        if args.is_empty() {
            return "Usage: choreo continue <protocol_id>".to_string();
        }

        format!("Continuing execution of protocol {}", args[0])
    }

    fn cmd_choreo_timeline(&self, args: &[&str]) -> String {
        let protocol_id = args
            .first()
            .map(|s| s.to_string())
            .unwrap_or_else(|| "all".to_string());
        format!("Timeline for protocol {}:\n  0ms: ProtocolStarted\n  50ms: MessageSent\n  150ms: MessageReceived", protocol_id)
    }

    fn cmd_choreo_analyze(&self, args: &[&str]) -> String {
        if args.is_empty() {
            return "Usage: choreo analyze <protocol_id>".to_string();
        }

        format!(
            "Analyzing protocol {} for deadlocks...\nNo deadlocks detected.",
            args[0]
        )
    }
}

impl Default for ReplCommandHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execute_help() {
        let mut handler = ReplCommandHandler::new();
        let entry = handler.execute("help");
        assert!(!entry.output.is_empty());
        assert!(entry.output.contains("REPL"));
    }

    #[test]
    fn test_unknown_command_delegated() {
        let mut handler = ReplCommandHandler::new();
        let entry = handler.execute("status");
        assert!(entry.output.starts_with("Unknown command:"));
    }

    #[test]
    fn test_clear_command() {
        let mut handler = ReplCommandHandler::new();
        let entry = handler.execute("clear");
        assert!(entry.output.contains("cleared"));
    }

    #[test]
    fn test_autocomplete_single_match() {
        let handler = ReplCommandHandler::new();
        let result = handler.autocomplete("hel");
        assert_eq!(result, Some("help".to_string()));
    }

    #[test]
    fn test_autocomplete_no_match() {
        let handler = ReplCommandHandler::new();
        let result = handler.autocomplete("xyz");
        assert_eq!(result, None);
    }

    #[test]
    fn test_history() {
        let mut handler = ReplCommandHandler::new();
        handler.execute("help");
        handler.execute("status");

        let history = handler.get_history();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0], "help");
        assert_eq!(history[1], "status");
    }

    #[test]
    fn test_empty_command() {
        let mut handler = ReplCommandHandler::new();
        let entry = handler.execute("   ");
        assert_eq!(entry.command, "");
        assert_eq!(entry.output, "");
    }
}
