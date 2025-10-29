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
        let commands = ["help", "clear"];

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
            // All other commands should be delegated to the data source
            _ => format!("Unknown command: {}", command),
        }
    }

    fn cmd_help(&self) -> String {
        r#"Aura REPL - Command Reference

UI Commands:
  help                  - Show this help
  clear                 - Clear console output

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
