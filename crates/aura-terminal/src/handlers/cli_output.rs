//! CLI Output Types for Testable Command Results
//!
//! This module provides structured output types that handlers return
//! instead of printing directly. This enables:
//! - Unit testing of handlers without capturing stdout
//! - Consistent output formatting
//! - Clear separation of logic from I/O

/// A single line of CLI output
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputLine {
    /// Standard output (stdout)
    Out(String),
    /// Error output (stderr)
    Err(String),
}

impl OutputLine {
    /// Create a stdout line
    pub fn out(s: impl Into<String>) -> Self {
        Self::Out(s.into())
    }

    /// Create a stderr line
    pub fn err(s: impl Into<String>) -> Self {
        Self::Err(s.into())
    }
}

/// Structured CLI output that can be rendered or tested
#[derive(Debug, Clone, Default)]
pub struct CliOutput {
    lines: Vec<OutputLine>,
}

impl CliOutput {
    /// Create an empty output
    pub fn new() -> Self {
        Self { lines: Vec::new() }
    }

    /// Add a stdout line
    pub fn println(&mut self, s: impl Into<String>) -> &mut Self {
        self.lines.push(OutputLine::Out(s.into()));
        self
    }

    /// Add a stderr line
    pub fn eprintln(&mut self, s: impl Into<String>) -> &mut Self {
        self.lines.push(OutputLine::Err(s.into()));
        self
    }

    /// Add a section header (e.g., "=== Title ===")
    pub fn section(&mut self, title: impl Into<String>) -> &mut Self {
        let title = title.into();
        self.lines
            .push(OutputLine::Out(format!("=== {} ===", title)));
        self
    }

    /// Add a key-value pair (e.g., "Key: Value")
    pub fn kv(&mut self, key: impl Into<String>, value: impl Into<String>) -> &mut Self {
        self.lines
            .push(OutputLine::Out(format!("{}: {}", key.into(), value.into())));
        self
    }

    /// Add a blank line
    pub fn blank(&mut self) -> &mut Self {
        self.lines.push(OutputLine::Out(String::new()));
        self
    }

    /// Add a formatted table
    pub fn table(&mut self, headers: &[&str], rows: &[Vec<String>]) -> &mut Self {
        if headers.is_empty() {
            return self;
        }

        // Calculate column widths
        let mut widths: Vec<usize> = headers.iter().map(|h| h.len()).collect();
        for row in rows {
            for (i, cell) in row.iter().enumerate() {
                if i < widths.len() {
                    widths[i] = widths[i].max(cell.len());
                }
            }
        }

        // Format header
        let header_line: String = headers
            .iter()
            .zip(&widths)
            .map(|(h, w)| format!("{:width$}", h, width = *w))
            .collect::<Vec<_>>()
            .join("  ");
        self.lines.push(OutputLine::Out(header_line));

        // Format separator
        let separator: String = widths
            .iter()
            .map(|w| "-".repeat(*w))
            .collect::<Vec<_>>()
            .join("  ");
        self.lines.push(OutputLine::Out(separator));

        // Format rows
        for row in rows {
            let row_line: String = row
                .iter()
                .zip(&widths)
                .map(|(cell, w)| format!("{:width$}", cell, width = *w))
                .collect::<Vec<_>>()
                .join("  ");
            self.lines.push(OutputLine::Out(row_line));
        }

        self
    }

    /// Get all output lines
    pub fn lines(&self) -> &[OutputLine] {
        &self.lines
    }

    /// Check if output is empty
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Get all stdout lines as strings
    pub fn stdout_lines(&self) -> Vec<&str> {
        self.lines
            .iter()
            .filter_map(|l| match l {
                OutputLine::Out(s) => Some(s.as_str()),
                OutputLine::Err(_) => None,
            })
            .collect()
    }

    /// Get all stderr lines as strings
    pub fn stderr_lines(&self) -> Vec<&str> {
        self.lines
            .iter()
            .filter_map(|l| match l {
                OutputLine::Out(_) => None,
                OutputLine::Err(s) => Some(s.as_str()),
            })
            .collect()
    }

    /// Render output to stdout/stderr
    pub fn render(&self) {
        for line in &self.lines {
            match line {
                OutputLine::Out(s) => println!("{}", s),
                OutputLine::Err(s) => eprintln!("{}", s),
            }
        }
    }

    /// Merge another output into this one
    pub fn extend(&mut self, other: CliOutput) -> &mut Self {
        self.lines.extend(other.lines);
        self
    }
}

/// Builder for CliOutput that allows method chaining
pub struct CliOutputBuilder {
    output: CliOutput,
}

impl CliOutputBuilder {
    /// Start building output
    pub fn new() -> Self {
        Self {
            output: CliOutput::new(),
        }
    }

    /// Add a stdout line
    pub fn println(mut self, s: impl Into<String>) -> Self {
        self.output.println(s);
        self
    }

    /// Add a stderr line
    pub fn eprintln(mut self, s: impl Into<String>) -> Self {
        self.output.eprintln(s);
        self
    }

    /// Add a section header
    pub fn section(mut self, title: impl Into<String>) -> Self {
        self.output.section(title);
        self
    }

    /// Add a key-value pair
    pub fn kv(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.output.kv(key, value);
        self
    }

    /// Build the final output
    pub fn build(self) -> CliOutput {
        self.output
    }
}

impl Default for CliOutputBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_output() {
        let mut out = CliOutput::new();
        out.println("Hello");
        out.eprintln("Error!");

        assert_eq!(out.stdout_lines(), vec!["Hello"]);
        assert_eq!(out.stderr_lines(), vec!["Error!"]);
    }

    #[test]
    fn test_section_and_kv() {
        let mut out = CliOutput::new();
        out.section("Status");
        out.kv("Name", "Alice");
        out.kv("Role", "Guardian");

        let lines = out.stdout_lines();
        assert_eq!(lines[0], "=== Status ===");
        assert_eq!(lines[1], "Name: Alice");
        assert_eq!(lines[2], "Role: Guardian");
    }

    #[test]
    fn test_table() {
        let mut out = CliOutput::new();
        out.table(
            &["Name", "Age"],
            &[
                vec!["Alice".into(), "30".into()],
                vec!["Bob".into(), "25".into()],
            ],
        );

        let lines = out.stdout_lines();
        assert_eq!(lines.len(), 4); // header, separator, 2 rows
        assert!(lines[0].contains("Name"));
        assert!(lines[0].contains("Age"));
        assert!(lines[1].contains("---"));
    }

    #[test]
    fn test_builder() {
        let out = CliOutputBuilder::new()
            .section("Test")
            .kv("Key", "Value")
            .println("Done")
            .build();

        assert_eq!(out.stdout_lines().len(), 3);
    }

    #[test]
    fn test_blank_adds_empty_line() {
        let mut out = CliOutput::new();
        out.println("Before");
        out.blank();
        out.println("After");

        let lines = out.stdout_lines();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "Before");
        assert_eq!(lines[1], "");
        assert_eq!(lines[2], "After");
    }

    #[test]
    fn test_is_empty() {
        let empty = CliOutput::new();
        assert!(empty.is_empty());

        let mut non_empty = CliOutput::new();
        non_empty.println("Hello");
        assert!(!non_empty.is_empty());
    }

    #[test]
    fn test_extend_merges_outputs() {
        let mut out1 = CliOutput::new();
        out1.println("Line 1");
        out1.eprintln("Error 1");

        let mut out2 = CliOutput::new();
        out2.println("Line 2");
        out2.eprintln("Error 2");

        out1.extend(out2);

        assert_eq!(out1.stdout_lines(), vec!["Line 1", "Line 2"]);
        assert_eq!(out1.stderr_lines(), vec!["Error 1", "Error 2"]);
    }

    #[test]
    fn test_output_line_constructors() {
        let out = OutputLine::out("stdout");
        let err = OutputLine::err("stderr");

        assert_eq!(out, OutputLine::Out("stdout".to_string()));
        assert_eq!(err, OutputLine::Err("stderr".to_string()));
    }

    #[test]
    fn test_table_empty_headers() {
        let mut out = CliOutput::new();
        out.table(&[], &[]);

        // Empty headers should result in no output
        assert!(out.is_empty());
    }

    #[test]
    fn test_table_handles_varying_widths() {
        let mut out = CliOutput::new();
        out.table(
            &["ID", "Name"],
            &[
                vec!["1".into(), "Alice".into()],
                vec!["1000".into(), "B".into()],
            ],
        );

        let lines = out.stdout_lines();
        // Column widths should accommodate longest values
        assert!(lines[2].contains("1   ") || lines[2].contains("1  ")); // ID column
        assert!(lines[3].contains("1000")); // Second row
    }

    #[test]
    fn test_builder_eprintln() {
        let out = CliOutputBuilder::new()
            .println("stdout")
            .eprintln("stderr")
            .build();

        assert_eq!(out.stdout_lines().len(), 1);
        assert_eq!(out.stderr_lines().len(), 1);
    }
}
