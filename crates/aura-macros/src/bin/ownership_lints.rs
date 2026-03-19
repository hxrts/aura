use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use proc_macro2::Span;
use quote::ToTokens;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{
    AttrStyle, Block, Expr, ExprAwait, ExprCall, ExprGroup, ExprMethodCall, ExprParen,
    ExprReference, File, ImplItem, ImplItemFn, Item,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum LintMode {
    SemanticOwnerBoundedAwaits,
    BestEffortSideEffectBoundary,
}

impl LintMode {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "semantic-owner-bounded-awaits" => Ok(Self::SemanticOwnerBoundedAwaits),
            "best-effort-side-effect-boundary" => Ok(Self::BestEffortSideEffectBoundary),
            other => Err(format!("unknown lint mode: {other}")),
        }
    }

    fn display_name(self) -> &'static str {
        match self {
            Self::SemanticOwnerBoundedAwaits => "semantic-owner-bounded-awaits",
            Self::BestEffortSideEffectBoundary => "best-effort-side-effect-boundary",
        }
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let mode = args
        .next()
        .ok_or_else(|| "usage: ownership_lints <lint-mode> <path> [<path> ...]".to_string())
        .and_then(|value| LintMode::parse(&value))?;
    let paths = args.map(PathBuf::from).collect::<Vec<_>>();
    if paths.is_empty() {
        return Err("expected at least one path to scan".to_string());
    }

    let mut rust_files = Vec::new();
    for path in &paths {
        collect_rust_files(path, &mut rust_files)?;
    }
    rust_files.sort();
    rust_files.dedup();

    let mut violations = Vec::new();
    for file in &rust_files {
        let source = fs::read_to_string(file)
            .map_err(|error| format!("failed to read {}: {error}", file.display()))?;
        let syntax = syn::parse_file(&source)
            .map_err(|error| format!("failed to parse {}: {error}", file.display()))?;
        violations.extend(scan_file(mode, file, &syntax));
    }

    if !violations.is_empty() {
        for violation in violations {
            eprintln!("{violation}");
        }
        return Err(match mode {
            LintMode::SemanticOwnerBoundedAwaits => {
                "semantic owner protocol violations remain in owner or handoff functions".to_string()
            }
            LintMode::BestEffortSideEffectBoundary => {
                "best-effort boundaries still own raw side effects or primary lifecycle publication".to_string()
            }
        });
    }

    println!("{}: clean (0 temporary exemptions)", mode.display_name());
    Ok(())
}

fn collect_rust_files(path: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    if path.is_file() {
        if path.extension() == Some(OsStr::new("rs")) {
            files.push(path.to_path_buf());
        }
        return Ok(());
    }
    if !path.is_dir() {
        return Err(format!("path does not exist: {}", path.display()));
    }

    for entry in fs::read_dir(path)
        .map_err(|error| format!("failed to read directory {}: {error}", path.display()))?
    {
        let entry = entry
            .map_err(|error| format!("failed to read directory entry {}: {error}", path.display()))?;
        let entry_path = entry.path();
        if entry_path.is_dir() {
            collect_rust_files(&entry_path, files)?;
        } else if entry_path.extension() == Some(OsStr::new("rs")) {
            files.push(entry_path);
        }
    }

    Ok(())
}

fn scan_file(mode: LintMode, file: &Path, syntax: &File) -> Vec<String> {
    let mut violations = Vec::new();
    for item in &syntax.items {
        scan_item(mode, file, item, &mut violations);
    }
    violations
}

fn scan_item(mode: LintMode, file: &Path, item: &Item, violations: &mut Vec<String>) {
    match item {
        Item::Fn(item_fn) => scan_function(
            mode,
            file,
            &item_fn.attrs,
            &item_fn.sig.ident.to_string(),
            &item_fn.block,
            violations,
        ),
        Item::Impl(item_impl) => {
            for impl_item in &item_impl.items {
                if let ImplItem::Fn(item_fn) = impl_item {
                    scan_impl_function(mode, file, item_fn, violations);
                }
            }
        }
        Item::Mod(item_mod) => {
            if let Some((_, items)) = &item_mod.content {
                for nested in items {
                    scan_item(mode, file, nested, violations);
                }
            }
        }
        _ => {}
    }
}

fn scan_impl_function(
    mode: LintMode,
    file: &Path,
    item_fn: &ImplItemFn,
    violations: &mut Vec<String>,
) {
    scan_function(
        mode,
        file,
        &item_fn.attrs,
        &item_fn.sig.ident.to_string(),
        &item_fn.block,
        violations,
    );
}

fn scan_function(
    mode: LintMode,
    file: &Path,
    attrs: &[syn::Attribute],
    function_name: &str,
    block: &Block,
    violations: &mut Vec<String>,
) {
    let contains_handoff = function_contains_call(block, "handoff_to_app_workflow");
    let should_scan = match mode {
        LintMode::SemanticOwnerBoundedAwaits => {
            has_marker_attr(attrs, "semantic_owner") || contains_handoff
        }
        LintMode::BestEffortSideEffectBoundary => has_marker_attr(attrs, "best_effort_boundary"),
    };
    if !should_scan {
        return;
    }

    let mut visitor = OwnershipVisitor {
        mode,
        file,
        function_name,
        violations: Vec::new(),
        has_handoff: contains_handoff,
        first_await_line: None,
        first_handoff_line: None,
        first_terminal_publication_line: None,
        best_effort_awaits: Vec::new(),
    };
    visitor.visit_block(block);
    visitor.finish();
    violations.extend(visitor.violations);
}

fn has_marker_attr(attrs: &[syn::Attribute], name: &str) -> bool {
    attrs.iter().any(|attr| {
        matches!(attr.style, AttrStyle::Outer)
            && attr
                .path()
                .segments
                .last()
                .is_some_and(|segment| segment.ident == name)
    })
}

struct OwnershipVisitor<'a> {
    mode: LintMode,
    file: &'a Path,
    function_name: &'a str,
    violations: Vec<String>,
    has_handoff: bool,
    first_await_line: Option<usize>,
    first_handoff_line: Option<usize>,
    first_terminal_publication_line: Option<usize>,
    best_effort_awaits: Vec<(Span, String)>,
}

impl OwnershipVisitor<'_> {
    fn push_violation(&mut self, span: Span, message: String) {
        let start = span.start();
        self.violations.push(format!(
            "{}:{}:{}: {}",
            self.file.display(),
            start.line,
            start.column + 1,
            message
        ));
    }

    fn note_terminal_publication(&mut self, span: Span, call_name: &str, tokens: &str) {
        if is_terminal_publication_call(call_name, tokens) {
            let line = span.start().line;
            self.first_terminal_publication_line = Some(
                self.first_terminal_publication_line
                    .map_or(line, |existing| existing.min(line)),
            );
        }
    }

    fn finish(&mut self) {
        if self.mode == LintMode::SemanticOwnerBoundedAwaits && self.has_handoff {
            if let (Some(first_await_line), Some(handoff_line)) =
                (self.first_await_line, self.first_handoff_line)
            {
                if first_await_line < handoff_line {
                    self.violations.push(format!(
                        "{}:{}:{}: callback/workflow boundary `{}` awaits before canonical handoff",
                        self.file.display(),
                        first_await_line,
                        1,
                        self.function_name
                    ));
                }
            }
        }

        if self.mode == LintMode::SemanticOwnerBoundedAwaits {
            for (span, call_name) in self.best_effort_awaits.clone() {
                let await_line = span.start().line;
                let published_terminal = self
                    .first_terminal_publication_line
                    .is_some_and(|line| line <= await_line);
                if !published_terminal {
                    self.push_violation(
                        span,
                        format!(
                            "semantic owner `{}` awaits best-effort helper before terminal publication: {}",
                            self.function_name, call_name
                        ),
                    );
                }
            }
        }
    }
}

impl<'ast> Visit<'ast> for OwnershipVisitor<'_> {
    fn visit_expr_method_call(&mut self, node: &'ast ExprMethodCall) {
        if node.method == "handoff_to_app_workflow" {
            let line = node.span().start().line;
            self.first_handoff_line = Some(
                self.first_handoff_line
                    .map_or(line, |existing| existing.min(line)),
            );
        }

        let method_name = node.method.to_string();
        let tokens = node.to_token_stream().to_string();
        self.note_terminal_publication(node.span(), &method_name, &tokens);

        visit::visit_expr_method_call(self, node);
    }

    fn visit_expr_call(&mut self, node: &'ast ExprCall) {
        if let Some(call_name) = expr_call_name(&node.func) {
            let tokens = node.to_token_stream().to_string();
            self.note_terminal_publication(node.span(), &call_name, &tokens);

            if self.mode == LintMode::BestEffortSideEffectBoundary
                && is_primary_lifecycle_publication_name(&call_name)
            {
                self.push_violation(
                    node.span(),
                    format!(
                        "best-effort function `{}` publishes primary lifecycle directly: {}",
                        self.function_name,
                        node.to_token_stream()
                    ),
                );
            }
        }

        visit::visit_expr_call(self, node);
    }

    fn visit_expr_await(&mut self, node: &'ast ExprAwait) {
        let line = node.span().start().line;
        self.first_await_line = Some(self.first_await_line.map_or(line, |existing| existing.min(line)));

        match self.mode {
            LintMode::SemanticOwnerBoundedAwaits => {
                if let Some(method_call) = method_call_on_ident(&node.base, "runtime") {
                    self.push_violation(
                        node.span(),
                        format!(
                            "raw runtime await inside semantic owner `{}`: {}",
                            self.function_name,
                            method_call.to_token_stream()
                        ),
                    );
                }
                if let Some(method_call) = method_call_on_ident(&node.base, "effects") {
                    self.push_violation(
                        node.span(),
                        format!(
                            "raw effects await inside semantic owner `{}`: {}",
                            self.function_name,
                            method_call.to_token_stream()
                        ),
                    );
                }
                if let Some(call_name) = awaited_call_name(&node.base) {
                    if call_name.starts_with("best_effort_") {
                        self.best_effort_awaits.push((node.span(), call_name));
                    }
                }
            }
            LintMode::BestEffortSideEffectBoundary => {
                if let Some(method_call) = method_call_on_ident(&node.base, "effects") {
                    let method_name = method_call.method.to_string();
                    if matches!(
                        method_name.as_str(),
                        "send_envelope" | "join_channel" | "create_channel"
                    ) {
                        self.push_violation(
                            node.span(),
                            format!(
                                "raw awaited side effect inside best-effort function `{}`: {}",
                                self.function_name,
                                method_call.to_token_stream()
                            ),
                        );
                    }
                }
                if let Some(call_name) = awaited_call_name(&node.base) {
                    if is_primary_lifecycle_publication_name(&call_name) {
                        self.push_violation(
                            node.span(),
                            format!(
                                "best-effort function `{}` awaits primary lifecycle publication directly: {}",
                                self.function_name, call_name
                            ),
                        );
                    }
                }
            }
        }

        visit::visit_expr_await(self, node);
    }
}

fn function_contains_call(block: &Block, call_name: &str) -> bool {
    struct CallFinder<'a> {
        call_name: &'a str,
        found: bool,
    }

    impl<'ast> Visit<'ast> for CallFinder<'_> {
        fn visit_expr_method_call(&mut self, node: &'ast ExprMethodCall) {
            if node.method == self.call_name {
                self.found = true;
            }
            visit::visit_expr_method_call(self, node);
        }

        fn visit_expr_call(&mut self, node: &'ast ExprCall) {
            if expr_call_name(&node.func).as_deref() == Some(self.call_name) {
                self.found = true;
            }
            visit::visit_expr_call(self, node);
        }
    }

    let mut finder = CallFinder {
        call_name,
        found: false,
    };
    finder.visit_block(block);
    finder.found
}

fn method_call_on_ident<'a>(expr: &'a Expr, receiver_ident: &str) -> Option<&'a ExprMethodCall> {
    let expr = strip_expression(expr);
    let Expr::MethodCall(method_call) = expr else {
        return None;
    };
    receiver_is_ident(&method_call.receiver, receiver_ident).then_some(method_call)
}

fn receiver_is_ident(expr: &Expr, expected_ident: &str) -> bool {
    match strip_expression(expr) {
        Expr::Path(path) => path.path.is_ident(expected_ident),
        _ => false,
    }
}

fn awaited_call_name(expr: &Expr) -> Option<String> {
    match strip_expression(expr) {
        Expr::Call(expr_call) => expr_call_name(&expr_call.func),
        Expr::MethodCall(method_call) => Some(method_call.method.to_string()),
        _ => None,
    }
}

fn expr_call_name(expr: &Expr) -> Option<String> {
    match strip_expression(expr) {
        Expr::Path(path) => path.path.segments.last().map(|segment| segment.ident.to_string()),
        _ => None,
    }
}

fn is_primary_lifecycle_publication_name(name: &str) -> bool {
    matches!(
        name,
        "publish_authoritative_operation_phase"
            | "publish_authoritative_operation_phase_with_instance"
            | "publish_authoritative_operation_failure"
            | "publish_authoritative_operation_failure_with_instance"
            | "publish_invitation_operation_status"
            | "publish_invitation_operation_failure"
            | "publish_pending_channel_accept_success"
    )
}

fn is_terminal_publication_call(name: &str, tokens: &str) -> bool {
    is_primary_lifecycle_publication_name(name)
        && (name.contains("failure")
            || tokens.contains("SemanticOperationPhase :: Succeeded")
            || tokens.contains("SemanticOperationPhase :: Failed")
            || tokens.contains("SemanticOperationPhase :: Cancelled"))
}

fn strip_expression(mut expr: &Expr) -> &Expr {
    loop {
        expr = match expr {
            Expr::Group(ExprGroup { expr, .. })
            | Expr::Paren(ExprParen { expr, .. })
            | Expr::Reference(ExprReference { expr, .. }) => expr,
            _ => return expr,
        };
    }
}
