//! CLI Output Types for Testable Command Results
//!
//! Handlers return a structured [`CliOutput`] instead of printing. The text
//! rendering and the `--json` view are both derived from the same entries,
//! so the two cannot disagree about what a command reported.

use serde_json::{Map, Value};

/// A single line of rendered CLI output
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

/// One structured element of a command's output.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Entry {
    Line(String),
    Warning(String),
    Section(String),
    Field(String, String),
    Table {
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    Blank,
}

/// Structured CLI output that can be rendered as text or JSON, or tested
#[derive(Debug, Clone, Default)]
pub struct CliOutput {
    entries: Vec<Entry>,
}

impl CliOutput {
    /// Create an empty output
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Add a stdout line
    pub fn println(&mut self, s: impl Into<String>) -> &mut Self {
        self.entries.push(Entry::Line(s.into()));
        self
    }

    /// Add a stderr line (a warning or diagnostic, not a failure)
    pub fn eprintln(&mut self, s: impl Into<String>) -> &mut Self {
        self.entries.push(Entry::Warning(s.into()));
        self
    }

    /// Add a section header (e.g., "=== Title ===")
    pub fn section(&mut self, title: impl Into<String>) -> &mut Self {
        self.entries.push(Entry::Section(title.into()));
        self
    }

    /// Add a key-value pair (e.g., "Key: Value")
    pub fn kv(&mut self, key: impl Into<String>, value: impl Into<String>) -> &mut Self {
        self.entries.push(Entry::Field(key.into(), value.into()));
        self
    }

    /// Add a blank line
    pub fn blank(&mut self) -> &mut Self {
        self.entries.push(Entry::Blank);
        self
    }

    /// Add a formatted table
    pub fn table(&mut self, headers: &[&str], rows: &[Vec<String>]) -> &mut Self {
        if headers.is_empty() {
            return self;
        }
        self.entries.push(Entry::Table {
            headers: headers.iter().map(|h| (*h).to_string()).collect(),
            rows: rows.to_vec(),
        });
        self
    }

    /// All output as rendered text lines
    #[must_use]
    pub fn lines(&self) -> Vec<OutputLine> {
        let mut lines = Vec::new();
        for entry in &self.entries {
            match entry {
                Entry::Line(s) => lines.push(OutputLine::Out(s.clone())),
                Entry::Warning(s) => lines.push(OutputLine::Err(s.clone())),
                Entry::Section(title) => lines.push(OutputLine::Out(format!("=== {title} ==="))),
                Entry::Field(k, v) => lines.push(OutputLine::Out(format!("{k}: {v}"))),
                Entry::Blank => lines.push(OutputLine::Out(String::new())),
                Entry::Table { headers, rows } => {
                    lines.extend(render_table(headers, rows).into_iter().map(OutputLine::Out));
                }
            }
        }
        lines
    }

    /// Check if output is empty
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// All stdout lines
    #[must_use]
    pub fn stdout_lines(&self) -> Vec<String> {
        self.lines()
            .into_iter()
            .filter_map(|l| match l {
                OutputLine::Out(s) => Some(s),
                OutputLine::Err(_) => None,
            })
            .collect()
    }

    /// All stderr lines
    #[must_use]
    pub fn stderr_lines(&self) -> Vec<String> {
        self.lines()
            .into_iter()
            .filter_map(|l| match l {
                OutputLine::Out(_) => None,
                OutputLine::Err(s) => Some(s),
            })
            .collect()
    }

    /// Structured view of this output for `--json`.
    ///
    /// Sections become objects carrying their key-value `fields`, table
    /// `rows` (objects keyed by header) and free-form `lines`; entries before
    /// the first section go into an untitled leading section. Warnings are
    /// collected under `warnings`.
    #[must_use]
    pub fn to_json(&self) -> Value {
        let mut sections: Vec<Map<String, Value>> = Vec::new();
        let mut warnings = Vec::new();
        for entry in &self.entries {
            match entry {
                Entry::Section(title) => {
                    let mut section = Map::new();
                    section.insert("title".to_string(), Value::String(title.clone()));
                    sections.push(section);
                }
                Entry::Field(k, v) => {
                    if let Value::Object(fields) = current_section(&mut sections)
                        .entry("fields")
                        .or_insert_with(|| Value::Object(Map::new()))
                    {
                        fields.insert(k.clone(), Value::String(v.clone()));
                    }
                }
                Entry::Line(line) => {
                    let line = line.trim();
                    if !line.is_empty() {
                        push_item(
                            current_section(&mut sections),
                            "lines",
                            Value::String(line.to_string()),
                        );
                    }
                }
                Entry::Table { headers, rows } => {
                    let section = current_section(&mut sections);
                    for row in rows {
                        let object: Map<String, Value> = headers
                            .iter()
                            .zip(row)
                            .map(|(h, cell)| (h.clone(), Value::String(cell.clone())))
                            .collect();
                        push_item(section, "rows", Value::Object(object));
                    }
                }
                Entry::Warning(w) => warnings.push(Value::String(w.clone())),
                Entry::Blank => {}
            }
        }
        let mut doc = Map::new();
        doc.insert(
            "sections".to_string(),
            Value::Array(sections.into_iter().map(Value::Object).collect()),
        );
        if !warnings.is_empty() {
            doc.insert("warnings".to_string(), Value::Array(warnings));
        }
        Value::Object(doc)
    }

    /// Render output to stdout/stderr
    pub fn render(&self) {
        for line in self.lines() {
            match line {
                OutputLine::Out(s) => println!("{s}"),
                OutputLine::Err(s) => eprintln!("{s}"),
            }
        }
    }

    /// Merge another output into this one
    pub fn extend(&mut self, other: CliOutput) -> &mut Self {
        self.entries.extend(other.entries);
        self
    }
}

fn current_section(sections: &mut Vec<Map<String, Value>>) -> &mut Map<String, Value> {
    if sections.is_empty() {
        sections.push(Map::new());
    }
    let last = sections.len() - 1;
    &mut sections[last]
}

fn push_item(section: &mut Map<String, Value>, key: &str, value: Value) {
    if let Value::Array(items) = section
        .entry(key)
        .or_insert_with(|| Value::Array(Vec::new()))
    {
        items.push(value);
    }
}

fn render_table(headers: &[String], rows: &[Vec<String>]) -> Vec<String> {
    let mut widths: Vec<usize> = headers.iter().map(String::len).collect();
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            if i < widths.len() {
                widths[i] = widths[i].max(cell.len());
            }
        }
    }
    let pad = |cells: &[String]| {
        cells
            .iter()
            .zip(&widths)
            .map(|(cell, w)| format!("{cell:w$}", w = *w))
            .collect::<Vec<_>>()
            .join("  ")
    };
    let mut lines = vec![pad(headers)];
    lines.push(
        widths
            .iter()
            .map(|w| "-".repeat(*w))
            .collect::<Vec<_>>()
            .join("  "),
    );
    lines.extend(rows.iter().map(|row| pad(row)));
    lines
}

/// Builder for CliOutput that allows method chaining
pub struct CliOutputBuilder {
    output: CliOutput,
}

impl CliOutputBuilder {
    /// Start building output
    #[must_use]
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
    #[must_use]
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

        assert_eq!(out.stdout_lines(), vec!["Before", "", "After"]);
    }

    #[test]
    fn test_is_empty() {
        assert!(CliOutput::new().is_empty());
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
    fn test_table_empty_headers() {
        let mut out = CliOutput::new();
        out.table(&[], &[]);
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
        assert!(lines[2].starts_with("1   "));
        assert!(lines[3].contains("1000"));
    }

    #[test]
    fn json_view_groups_fields_rows_and_lines_by_section() {
        let mut out = CliOutput::new();
        out.println("preamble");
        out.section("Account");
        out.kv("Nickname", "Alex");
        out.table(&["ID", "Name"], &[vec!["1".into(), "Barbara".into()]]);
        out.blank();
        out.eprintln("careful");

        let json = out.to_json();
        assert_eq!(json["sections"][0]["lines"][0], "preamble");
        assert_eq!(json["sections"][1]["title"], "Account");
        assert_eq!(json["sections"][1]["fields"]["Nickname"], "Alex");
        assert_eq!(json["sections"][1]["rows"][0]["Name"], "Barbara");
        assert_eq!(json["warnings"][0], "careful");
    }
}
