//! The `aura` CLI and `aura rpc` reach the account only through the shared
//! app workflows (work/8.md Task 156).
//!
//! Account commands live in aura-terminal's command model (`src/command`),
//! its parsers (`src/cli`) and the RPC server (`src/rpc`). Those files may
//! import aura-app only through `aura_app::ui` (and its workflows facade):
//! no `aura_agent` paths, no crate-root `aura_app::*` reach-ins, no agent
//! service accessors (`.chat()`, `.invitations()`, `.runtime()`), and no
//! local files (`std::fs`, `tokio::fs`). Positive `cfg(test)` scopes are
//! exempt. `src/handlers` keeps only the listed offline tools and
//! long-running modes; a new handler module fails the check, so new account
//! commands start on the command model.
use anyhow::{bail, Result};
use proc_macro2::{TokenStream, TokenTree};
use std::path::Path;
use syn::{spanned::Spanned, visit::Visit};

/// Directories under `crates/aura-terminal/src` checked file by file.
const CHECKED_DIRS: &[&str] = &["command", "cli", "rpc"];

/// Handler modules that are not account commands (offline tools,
/// long-running modes, the TUI launcher and shared output types).
const HANDLER_MODULES: &[&str] = &[
    "budget.rs",
    "cli_output.rs",
    "config.rs",
    "demo.rs",
    "handler_context.rs",
    "init.rs",
    "mod.rs",
    "scenarios",
    "sync.rs",
    "threshold.rs",
    "tui",
    "tui_stdio.rs",
    "version.rs",
];

/// Agent service accessors a CLI command must not call.
const SERVICE_ACCESSORS: &[&str] = &["chat", "invitations", "runtime", "recovery_service"];

#[derive(Default)]
struct Violations {
    found: Vec<(usize, String)>,
}

impl Violations {
    fn path(&mut self, segments: &[String], line: usize) {
        let Some(first) = segments.first() else {
            return;
        };
        let rendered = segments.join("::");
        if first == "aura_agent" {
            self.found.push((
                line,
                format!("agent API `{rendered}`; use aura_app::ui::workflows"),
            ));
        } else if first == "aura_app" && segments.get(1).is_some_and(|s| s != "ui") {
            self.found.push((
                line,
                format!("`{rendered}` bypasses the aura_app::ui facade"),
            ));
        } else if (first == "std" || first == "tokio") && segments.get(1).is_some_and(|s| s == "fs")
        {
            self.found.push((
                line,
                format!("local file access `{rendered}`; account state goes through workflows"),
            ));
        }
    }

    fn tokens(&mut self, tokens: TokenStream) {
        let tokens: Vec<_> = tokens.into_iter().collect();
        for window in tokens.windows(4) {
            if let [TokenTree::Ident(first), TokenTree::Punct(a), TokenTree::Punct(b), TokenTree::Ident(second)] =
                window
            {
                if a.as_char() == ':' && b.as_char() == ':' {
                    self.path(
                        &[first.to_string(), second.to_string()],
                        first.span().start().line,
                    );
                }
            }
        }
        for token in tokens {
            if let TokenTree::Group(group) = token {
                self.tokens(group.stream());
            }
        }
    }
}

impl<'ast> Visit<'ast> for Violations {
    fn visit_path(&mut self, path: &'ast syn::Path) {
        let segments: Vec<String> = path.segments.iter().map(|s| s.ident.to_string()).collect();
        self.path(&segments, path.span().start().line);
        syn::visit::visit_path(self, path);
    }

    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        fn walk(tree: &syn::UseTree, prefix: &mut Vec<String>, out: &mut Vec<Vec<String>>) {
            match tree {
                syn::UseTree::Path(p) => {
                    prefix.push(p.ident.to_string());
                    walk(&p.tree, prefix, out);
                    prefix.pop();
                }
                syn::UseTree::Name(n) => {
                    let mut path = prefix.clone();
                    path.push(n.ident.to_string());
                    out.push(path);
                }
                syn::UseTree::Rename(r) => {
                    let mut path = prefix.clone();
                    path.push(r.ident.to_string());
                    out.push(path);
                }
                syn::UseTree::Glob(_) => out.push(prefix.clone()),
                syn::UseTree::Group(g) => {
                    for item in &g.items {
                        walk(item, prefix, out);
                    }
                }
            }
        }
        let mut paths = Vec::new();
        walk(&item.tree, &mut Vec::new(), &mut paths);
        for path in paths {
            self.path(&path, item.span().start().line);
        }
        syn::visit::visit_item_use(self, item);
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        let name = call.method.to_string();
        if call.args.is_empty() && SERVICE_ACCESSORS.contains(&name.as_str()) {
            self.found.push((
                call.method.span().start().line,
                format!("agent service accessor `.{name}()`; use aura_app::ui::workflows"),
            ));
        }
        syn::visit::visit_expr_method_call(self, call);
    }

    fn visit_macro(&mut self, item: &'ast syn::Macro) {
        self.tokens(item.tokens.clone());
        syn::visit::visit_macro(self, item);
    }
}

/// Production-scope violations in one source file, as `(line, reason)`.
pub(super) fn violations(source: &str) -> Result<Vec<(usize, String)>> {
    let test_lines = super::security_test_scope::test_lines(source)?;
    let mut visitor = Violations::default();
    visitor.visit_file(&syn::parse_file(source)?);
    let mut found: Vec<_> = visitor
        .found
        .into_iter()
        .filter(|(line, _)| !test_lines.contains(line))
        .collect();
    found.sort();
    found.dedup();
    Ok(found)
}

pub(super) fn run(repo_root: &Path) -> Result<()> {
    let src = repo_root.join("crates/aura-terminal/src");
    let mut problems = Vec::new();
    for dir in CHECKED_DIRS {
        let root = src.join(dir);
        if !root.is_dir() {
            bail!("cli-workflow-facade: missing {}", root.display());
        }
        for file in super::super::support::rust_files_under(&root) {
            let source = super::super::support::read(&file)?;
            for (line, reason) in violations(&source)? {
                problems.push(format!("{}:{line}: {reason}", file.display()));
            }
        }
    }
    let handlers = src.join("handlers");
    for entry in std::fs::read_dir(&handlers)? {
        let name = entry?.file_name().to_string_lossy().into_owned();
        if !HANDLER_MODULES.contains(&name.as_str()) {
            problems.push(format!(
                "{}: new handler module; account commands belong to the command model (src/command)",
                handlers.join(&name).display()
            ));
        }
    }
    if !problems.is_empty() {
        bail!("cli-workflow-facade:\n  {}", problems.join("\n  "));
    }
    println!("cli-workflow-facade: clean");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn lines(source: &str) -> BTreeSet<usize> {
        violations(source)
            .unwrap()
            .into_iter()
            .map(|(l, _)| l)
            .collect()
    }

    #[test]
    fn agent_paths_crate_root_reach_ins_and_service_calls_are_rejected() {
        let source = r#"
use aura_agent::handlers::ChatGroupId;
use aura_app::ui::workflows::messaging;
use aura_app::workflows::messaging as raw;
fn f(agent: &Agent) { let _ = agent.chat(); }
fn g() { let _ = std::fs::read("x"); }
fn h() { let _ = format!("{:?}", aura_agent::AuraAgent::new); }
fn ok() { let _ = messaging::resolve_channel; }
"#;
        assert_eq!(lines(source), BTreeSet::from([2, 4, 5, 6, 7]));
    }

    #[test]
    fn grouped_imports_and_aliases_are_inspected() {
        let source = r#"
use aura_app::{ui::types::AppCore, workflows::invitation};
use aura_agent as agent;
"#;
        assert_eq!(lines(source), BTreeSet::from([2, 3]));
    }

    #[test]
    fn only_genuine_test_scopes_are_exempt() {
        let source = r#"
#[cfg(test)]
mod tests { use aura_agent::AgentBuilder; }
#[cfg(any(test, feature = "x"))]
fn mixed() { let _ = aura_agent::AgentBuilder::new(); }
// aura_agent::AgentBuilder in a comment
fn string() { let _ = "aura_agent::AgentBuilder"; }
"#;
        assert_eq!(lines(source), BTreeSet::from([5]));
    }

    #[test]
    fn the_command_model_is_clean() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        run(&repo).unwrap();
    }
}
