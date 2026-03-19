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
    ExprPath, ExprReference, File, ImplItem, ImplItemFn, Item, ItemFn, ItemStruct, ReturnType,
    Type, Visibility,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum LintMode {
    SemanticOwnerBoundedAwaits,
    BestEffortSideEffectBoundary,
    ActorOwnedTaskSpawn,
    AsyncSessionOwnership,
    FrontendSemanticHandoffBoundary,
    HarnessMoveOwnershipBoundary,
    HarnessReadinessOwnership,
    HarnessRecoveryOwnership,
    TimeoutPolicyBoundary,
    TimeDomainUsage,
}

impl LintMode {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "semantic-owner-bounded-awaits" => Ok(Self::SemanticOwnerBoundedAwaits),
            "best-effort-side-effect-boundary" => Ok(Self::BestEffortSideEffectBoundary),
            "actor-owned-task-spawn" => Ok(Self::ActorOwnedTaskSpawn),
            "async-session-ownership" => Ok(Self::AsyncSessionOwnership),
            "frontend-semantic-handoff-boundary" => Ok(Self::FrontendSemanticHandoffBoundary),
            "harness-move-ownership-boundary" => Ok(Self::HarnessMoveOwnershipBoundary),
            "harness-readiness-ownership" => Ok(Self::HarnessReadinessOwnership),
            "harness-recovery-ownership" => Ok(Self::HarnessRecoveryOwnership),
            "timeout-policy-boundary" => Ok(Self::TimeoutPolicyBoundary),
            "time-domain-usage" => Ok(Self::TimeDomainUsage),
            other => Err(format!("unknown lint mode: {other}")),
        }
    }

    fn display_name(self) -> &'static str {
        match self {
            Self::SemanticOwnerBoundedAwaits => "semantic-owner-bounded-awaits",
            Self::BestEffortSideEffectBoundary => "best-effort-side-effect-boundary",
            Self::ActorOwnedTaskSpawn => "actor-owned-task-spawn",
            Self::AsyncSessionOwnership => "async-session-ownership",
            Self::FrontendSemanticHandoffBoundary => "frontend-semantic-handoff-boundary",
            Self::HarnessMoveOwnershipBoundary => "harness-move-ownership-boundary",
            Self::HarnessReadinessOwnership => "harness-readiness-ownership",
            Self::HarnessRecoveryOwnership => "harness-recovery-ownership",
            Self::TimeoutPolicyBoundary => "timeout-policy-boundary",
            Self::TimeDomainUsage => "time-domain-usage",
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
        violations.extend(scan_file(mode, file, &source, &syntax));
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
            LintMode::ActorOwnedTaskSpawn => {
                "raw task spawning or async ownership escape hatches remain outside sanctioned modules"
                    .to_string()
            }
            LintMode::AsyncSessionOwnership => {
                "direct VM/session mutation bypasses runtime/session_ingress.rs".to_string()
            }
            LintMode::FrontendSemanticHandoffBoundary => {
                "frontend semantic handoff boundaries still expose bypass paths".to_string()
            }
            LintMode::HarnessMoveOwnershipBoundary => {
                "shared semantic move ownership escapes approved handle / receipt boundary modules"
                    .to_string()
            }
            LintMode::HarnessReadinessOwnership => {
                "frontend/harness modules author or refresh authoritative readiness outside approved coordinators"
                    .to_string()
            }
            LintMode::HarnessRecoveryOwnership => {
                "parity-critical observation code may not introduce sleeps, retries, or recovery helpers outside approved owner modules"
                    .to_string()
            }
            LintMode::TimeoutPolicyBoundary => {
                "timeout policy boundary still exposes raw time primitives".to_string()
            }
            LintMode::TimeDomainUsage => {
                "semantic layers are using direct wall-clock time primitives instead of typed time domains"
                    .to_string()
            }
        });
    }

    println!("{}: clean", mode.display_name());
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

fn scan_file(mode: LintMode, file: &Path, source: &str, syntax: &File) -> Vec<String> {
    match mode {
        LintMode::ActorOwnedTaskSpawn => return scan_actor_owned_task_spawn(file, syntax),
        LintMode::AsyncSessionOwnership => return scan_async_session_ownership(file, source),
        LintMode::FrontendSemanticHandoffBoundary => {
            return scan_frontend_semantic_handoff_boundary(file, syntax);
        }
        LintMode::HarnessMoveOwnershipBoundary => {
            return scan_harness_move_ownership_boundary(file, source);
        }
        LintMode::HarnessReadinessOwnership => {
            return scan_harness_readiness_ownership(file, source);
        }
        LintMode::HarnessRecoveryOwnership => {
            return scan_harness_recovery_ownership(file, source);
        }
        LintMode::TimeoutPolicyBoundary => return scan_timeout_policy_boundary(file, syntax),
        LintMode::TimeDomainUsage => return scan_time_domain_usage(file, syntax),
        LintMode::SemanticOwnerBoundedAwaits | LintMode::BestEffortSideEffectBoundary => {}
    }

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
        LintMode::ActorOwnedTaskSpawn
        | LintMode::AsyncSessionOwnership
        | LintMode::FrontendSemanticHandoffBoundary
        | LintMode::HarnessMoveOwnershipBoundary
        | LintMode::HarnessReadinessOwnership
        | LintMode::HarnessRecoveryOwnership
        | LintMode::TimeoutPolicyBoundary
        | LintMode::TimeDomainUsage => false,
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
            LintMode::ActorOwnedTaskSpawn
            | LintMode::AsyncSessionOwnership
            | LintMode::FrontendSemanticHandoffBoundary
            | LintMode::HarnessMoveOwnershipBoundary
            | LintMode::HarnessReadinessOwnership
            | LintMode::HarnessRecoveryOwnership
            | LintMode::TimeoutPolicyBoundary
            | LintMode::TimeDomainUsage => {}
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

const ACTOR_SPAWN_APPROVED_SUFFIXES: &[&str] = &[
    "crates/aura-agent/src/task_registry.rs",
    "crates/aura-agent/src/runtime/services/service_actor.rs",
    "crates/aura-effects/src/reactive/handler.rs",
    "crates/aura-effects/src/reactive/graph.rs",
    "crates/aura-harness/src/backend/local_pty.rs",
    "crates/aura-harness/src/backend/playwright_browser.rs",
    "crates/aura-harness/src/bin/tool_repl.rs",
    "crates/aura-harness/src/coordinator.rs",
    "crates/aura-harness/src/executor.rs",
    "crates/aura-terminal/src/tui/tasks.rs",
    "crates/aura-testkit/src/infrastructure/time.rs",
    "crates/aura-ui/src/app.rs",
    "crates/aura-web/src/harness_bridge.rs",
    "crates/aura-web/src/main.rs",
    "crates/aura-web/src/task_owner.rs",
    "crates/aura-web/src/web_clipboard.rs",
];
const HARNESS_MOVE_APPROVED_SUFFIXES: &[&str] = &[
    "crates/aura-app/src/ui_contract.rs",
    "crates/aura-app/src/scenario_contract.rs",
    "crates/aura-app/src/workflows/harness_determinism.rs",
    "crates/aura-app/tests/ui/ui_operation_handle_private_fields.rs",
    "crates/aura-app/tests/ui/harness_ui_operation_handle_private_fields.rs",
    "crates/aura-harness/src/backend/local_pty.rs",
    "crates/aura-harness/src/backend/mod.rs",
    "crates/aura-harness/src/executor.rs",
    "crates/aura-terminal/src/tui/harness_state.rs",
    "crates/aura-terminal/src/tui/harness_state/mod.rs",
    "crates/aura-terminal/src/tui/semantic_lifecycle.rs",
    "crates/aura-terminal/src/tui/screens/app/shell.rs",
];

const FRONTEND_INTERNAL_OWNER_SUFFIXES: &[&str] = &["crates/aura-terminal/src/tui/semantic_lifecycle.rs"];
const FRONTEND_SUBMIT_SUFFIXES: &[&str] = &[
    "crates/aura-terminal/src/tui/screens/app/shell.rs",
    "crates/aura-terminal/src/tui/semantic_lifecycle.rs",
];
const FRONTEND_HANDOFF_SUFFIXES: &[&str] = &[
    "crates/aura-terminal/src/tui/callbacks/factories/chat.rs",
    "crates/aura-terminal/src/tui/callbacks/factories/contacts.rs",
    "crates/aura-terminal/src/tui/callbacks/factories/mod.rs",
    "crates/aura-terminal/src/tui/semantic_lifecycle.rs",
];
const FRONTEND_AUTHORITATIVE_STATE_SUFFIXES: &[&str] = &[
    "crates/aura-terminal/src/tui/screens/app/shell.rs",
    "crates/aura-terminal/src/tui/state/mod.rs",
    "crates/aura-terminal/src/tui/harness_state/mod.rs",
];

fn file_matches_suffix(file: &Path, suffixes: &[&str]) -> bool {
    let display = file.to_string_lossy();
    suffixes.iter().any(|suffix| display.ends_with(suffix))
}

fn call_path_string(expr: &Expr) -> Option<String> {
    match strip_expression(expr) {
        Expr::Path(path) => Some(path.path.to_token_stream().to_string()),
        _ => None,
    }
}

fn is_pub(vis: &Visibility) -> bool {
    !matches!(vis, Visibility::Inherited)
}

fn type_contains_join_handle(ty: &Type) -> bool {
    ty.to_token_stream().to_string().contains("JoinHandle")
}

fn return_type_contains_join_handle(return_type: &ReturnType) -> bool {
    match return_type {
        ReturnType::Default => false,
        ReturnType::Type(_, ty) => type_contains_join_handle(ty),
    }
}

fn scan_actor_owned_task_spawn(file: &Path, syntax: &File) -> Vec<String> {
    struct Visitor<'a> {
        file: &'a Path,
        violations: Vec<String>,
        approved_file: bool,
    }

    impl Visitor<'_> {
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

        fn visit_public_fn(&mut self, item_fn: &ItemFn) {
            if is_pub(&item_fn.vis) && return_type_contains_join_handle(&item_fn.sig.output) {
                self.push_violation(
                    item_fn.sig.output.span(),
                    "public parity-critical API exposes raw JoinHandle".to_string(),
                );
            }
        }

        fn visit_public_impl_fn(&mut self, item_fn: &ImplItemFn) {
            if is_pub(&item_fn.vis) && return_type_contains_join_handle(&item_fn.sig.output) {
                self.push_violation(
                    item_fn.sig.output.span(),
                    "public parity-critical API exposes raw JoinHandle".to_string(),
                );
            }
        }

        fn visit_public_struct(&mut self, item_struct: &ItemStruct) {
            for field in &item_struct.fields {
                if is_pub(&field.vis) && type_contains_join_handle(&field.ty) {
                    self.push_violation(
                        field.ty.span(),
                        "public parity-critical API exposes raw JoinHandle field".to_string(),
                    );
                }
            }
        }
    }

    impl<'ast> Visit<'ast> for Visitor<'_> {
        fn visit_item_fn(&mut self, node: &'ast ItemFn) {
            self.visit_public_fn(node);
            visit::visit_item_fn(self, node);
        }

        fn visit_impl_item_fn(&mut self, node: &'ast ImplItemFn) {
            self.visit_public_impl_fn(node);
            visit::visit_impl_item_fn(self, node);
        }

        fn visit_item_struct(&mut self, node: &'ast ItemStruct) {
            self.visit_public_struct(node);
            visit::visit_item_struct(self, node);
        }

        fn visit_expr_call(&mut self, node: &'ast ExprCall) {
            if let Some(path) = call_path_string(&node.func) {
                let path = path.replace(' ', "");
                let is_raw_spawn = matches!(
                    path.as_str(),
                    "tokio::spawn" | "std::thread::spawn" | "thread::spawn" | "spawn_local"
                );
                let is_unbounded = matches!(
                    path.as_str(),
                    "mpsc::unbounded_channel"
                        | "tokio::sync::mpsc::unbounded_channel"
                        | "mpsc::unbounded"
                );
                if !self.approved_file && is_raw_spawn {
                    self.push_violation(
                        node.span(),
                        format!("raw task spawn outside sanctioned owner module: {path}"),
                    );
                }
                if is_unbounded {
                    self.push_violation(
                        node.span(),
                        format!("unbounded channel in parity-critical module: {path}"),
                    );
                }
            }
            visit::visit_expr_call(self, node);
        }
    }

    let mut visitor = Visitor {
        file,
        violations: Vec::new(),
        approved_file: file_matches_suffix(file, ACTOR_SPAWN_APPROVED_SUFFIXES),
    };
    visitor.visit_file(syntax);
    visitor.violations
}

fn is_comment_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("//") || trimmed.starts_with("/*") || trimmed.starts_with('*')
}

fn source_line_violations(
    file: &Path,
    source: &str,
    patterns: &[&str],
    approved_suffixes: &[&str],
) -> Vec<String> {
    let approved_file = file_matches_suffix(file, approved_suffixes);
    let mut violations = Vec::new();

    for (index, line) in source.lines().enumerate() {
        if is_comment_line(line) {
            continue;
        }
        if patterns.iter().any(|pattern| line.contains(pattern)) && !approved_file {
            violations.push(format!(
                "{}:{}:1: forbidden ownership escape hatch: {}",
                file.display(),
                index + 1,
                line.trim()
            ));
        }
    }

    violations
}

fn scan_async_session_ownership(file: &Path, source: &str) -> Vec<String> {
    source_line_violations(
        file,
        source,
        &[
            "open_manifest_vm_session_admitted",
            "ChoreographicEffects::start_session(",
            "ChoreographicEffects::end_session(",
            "inject_vm_receive",
            "effects.start_session(",
            "self.effects.start_session(",
            "effects.end_session(",
            "self.effects.end_session(",
        ],
        &[],
    )
}

fn scan_harness_move_ownership_boundary(file: &Path, source: &str) -> Vec<String> {
    source_line_violations(
        file,
        source,
        &[
            "UiOperationHandle::new(",
            "HarnessUiOperationHandle::new(",
            "record_submission_handle(",
            "HarnessUiCommandReceipt::Accepted",
            "instance_id = Some(",
        ],
        HARNESS_MOVE_APPROVED_SUFFIXES,
    )
}

fn scan_harness_readiness_ownership(file: &Path, source: &str) -> Vec<String> {
    source_line_violations(
        file,
        source,
        &[
            "refresh_authoritative_invitation_readiness",
            "refresh_authoritative_contact_link_readiness",
            "refresh_authoritative_channel_membership_readiness",
            "refresh_authoritative_recipient_resolution_readiness",
            "refresh_authoritative_delivery_readiness",
            "publish_authoritative_semantic_fact(",
            "replace_authoritative_semantic_facts_of_kind(",
        ],
        &[],
    )
}

fn scan_harness_recovery_ownership(file: &Path, source: &str) -> Vec<String> {
    source_line_violations(
        file,
        source,
        &[
            "std::thread::sleep",
            "thread::sleep",
            "tokio::time::sleep",
            "run_registered_recovery",
            "retry",
            "fallback",
        ],
        &[],
    )
}

fn scan_frontend_semantic_handoff_boundary(file: &Path, syntax: &File) -> Vec<String> {
    struct Visitor<'a> {
        file: &'a Path,
        violations: Vec<String>,
    }

    impl Visitor<'_> {
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
    }

    impl<'ast> Visit<'ast> for Visitor<'_> {
        fn visit_expr_path(&mut self, node: &'ast ExprPath) {
            let path = node.path.to_token_stream().to_string().replace(' ', "");
            if path.contains("SubmittedOperationOwner::")
                && !file_matches_suffix(self.file, FRONTEND_INTERNAL_OWNER_SUFFIXES)
            {
                self.push_violation(
                    node.span(),
                    "internal submitted-owner access escaped the sanctioned boundary".to_string(),
                );
            }
            visit::visit_expr_path(self, node);
        }

        fn visit_expr_call(&mut self, node: &'ast ExprCall) {
            if let Some(path) = call_path_string(&node.func) {
                let path = path.replace(' ', "");
                if path.contains("LocalTerminalOperationOwner::submit")
                    && !file_matches_suffix(self.file, FRONTEND_SUBMIT_SUFFIXES)
                {
                    self.push_violation(
                        node.span(),
                        "local semantic owner allocation escaped the sanctioned boundary".to_string(),
                    );
                }
                if path.contains("WorkflowHandoffOperationOwner::submit")
                    && !file_matches_suffix(self.file, FRONTEND_SUBMIT_SUFFIXES)
                {
                    self.push_violation(
                        node.span(),
                        "workflow handoff owner allocation escaped the sanctioned boundary".to_string(),
                    );
                }
            }
            visit::visit_expr_call(self, node);
        }

        fn visit_expr_method_call(&mut self, node: &'ast ExprMethodCall) {
            if node.method == "handoff_to_app_workflow"
                && !file_matches_suffix(self.file, FRONTEND_HANDOFF_SUFFIXES)
            {
                self.push_violation(
                    node.span(),
                    "frontend handoff escaped the sanctioned callback boundary".to_string(),
                );
            }
            if node.method == "set_authoritative_operation_state"
                && !file_matches_suffix(self.file, FRONTEND_AUTHORITATIVE_STATE_SUFFIXES)
            {
                self.push_violation(
                    node.span(),
                    "authoritative operation state mutation escaped the sanctioned boundary"
                        .to_string(),
                );
            }
            visit::visit_expr_method_call(self, node);
        }
    }

    let mut visitor = Visitor {
        file,
        violations: Vec::new(),
    };
    visitor.visit_file(syntax);
    visitor.violations
}

fn scan_timeout_policy_boundary(file: &Path, syntax: &File) -> Vec<String> {
    struct Visitor<'a> {
        file: &'a Path,
        violations: Vec<String>,
    }

    impl Visitor<'_> {
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
    }

    impl<'ast> Visit<'ast> for Visitor<'_> {
        fn visit_expr_call(&mut self, node: &'ast ExprCall) {
            if let Some(path) = call_path_string(&node.func) {
                let path = path.replace(' ', "");
                if matches!(
                    path.as_str(),
                    "tokio::time::timeout"
                        | "tokio::time::sleep"
                        | "tokio::time::interval"
                        | "std::thread::sleep"
                        | "thread::sleep"
                ) {
                    self.push_violation(
                        node.span(),
                        format!("raw time primitive outside shared timeout model: {path}"),
                    );
                }
            }
            visit::visit_expr_call(self, node);
        }
    }

    let mut visitor = Visitor {
        file,
        violations: Vec::new(),
    };
    visitor.visit_file(syntax);
    visitor.violations
}

fn scan_time_domain_usage(file: &Path, syntax: &File) -> Vec<String> {
    struct Visitor<'a> {
        file: &'a Path,
        violations: Vec<String>,
    }

    impl Visitor<'_> {
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
    }

    impl<'ast> Visit<'ast> for Visitor<'_> {
        fn visit_expr_call(&mut self, node: &'ast ExprCall) {
            if let Some(path) = call_path_string(&node.func) {
                let path = path.replace(' ', "");
                if matches!(
                    path.as_str(),
                    "tokio::time::timeout"
                        | "tokio::time::sleep"
                        | "SystemTime::now"
                        | "Instant::now"
                ) {
                    self.push_violation(
                        node.span(),
                        format!("direct wall-clock primitive in semantic layer: {path}"),
                    );
                }
            }
            visit::visit_expr_call(self, node);
        }
    }

    let mut visitor = Visitor {
        file,
        violations: Vec::new(),
    };
    visitor.visit_file(syntax);
    visitor.violations
}
