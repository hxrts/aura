use rustc_errors::DiagDecorator;
use rustc_hir::{AmbigArg, Expr, ExprKind, QPath, Ty, TyKind};
use rustc_lint::{LateContext, LateLintPass, LintContext};

use crate::support::{normalized_source_path, path_matches_scope, qpath_def_path, SeenSpans};

const SCOPES: &[&str] = &[
    "/crates/aura-harness/src/executor.rs",
    "/crates/aura-harness/src/replay.rs",
    "/crates/aura-harness/src/backend/mod.rs",
    "/crates/aura-harness/src/backend/local_pty.rs",
    "/crates/aura-terminal/src/tui/harness_state/",
    "/crates/aura-web/src/harness_bridge.rs",
];

rustc_session::declare_lint! {
    pub HARNESS_TYPED_JSON_BOUNDARY,
    Deny,
    "shared semantic harness core must not use raw serde_json::Value plumbing",
}

#[derive(Default)]
pub(crate) struct HarnessTypedJsonBoundary {
    seen: SeenSpans,
}

rustc_session::impl_lint_pass!(HarnessTypedJsonBoundary => [HARNESS_TYPED_JSON_BOUNDARY]);

impl<'tcx> LateLintPass<'tcx> for HarnessTypedJsonBoundary {
    fn check_ty(&mut self, cx: &LateContext<'tcx>, ty: &'tcx Ty<'tcx, AmbigArg>) {
        if ty.span.from_expansion() {
            return;
        }
        let TyKind::Path(qpath) = ty.kind else {
            return;
        };
        if !is_serde_json_value(cx, &qpath) {
            return;
        }
        emit_if_in_scope(
            cx,
            &mut self.seen,
            ty.span,
            "shared semantic core must not use `serde_json::Value`; decode into typed payloads at the outer boundary",
        );
    }

    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        if expr.span.from_expansion() {
            return;
        }
        let ExprKind::Call(func, _) = expr.kind else {
            return;
        };
        let ExprKind::Path(qpath) = func.kind else {
            return;
        };
        if !is_serde_json_from_value(cx, &qpath) {
            return;
        }
        emit_if_in_scope(
            cx,
            &mut self.seen,
            expr.span,
            "shared semantic core must not call `serde_json::from_value`; decode into typed payloads before entering the semantic boundary",
        );
    }
}

fn is_serde_json_value<'tcx>(cx: &LateContext<'tcx>, qpath: &QPath<'tcx>) -> bool {
    qpath_def_path(cx, qpath).is_some_and(|path| {
        path.ends_with("serde_json::value::Value") || path.ends_with("serde_json::Value")
    })
}

fn is_serde_json_from_value<'tcx>(cx: &LateContext<'tcx>, qpath: &QPath<'tcx>) -> bool {
    qpath_def_path(cx, qpath).is_some_and(|path| path.ends_with("serde_json::value::from_value"))
}

fn emit_if_in_scope<'tcx>(
    cx: &LateContext<'tcx>,
    seen: &mut SeenSpans,
    span: rustc_span::Span,
    message: &'static str,
) {
    let source_map = cx.sess().source_map();
    let path = normalized_source_path(source_map, span);
    if !path_matches_scope(&path, SCOPES) {
        return;
    }
    let line = source_map
        .lookup_line(span.lo())
        .map_or(0, |info| info.line + 1);
    if !seen.insert(path, line as u32) {
        return;
    }
    cx.emit_span_lint(
        HARNESS_TYPED_JSON_BOUNDARY,
        span,
        DiagDecorator(|diag| {
            diag.primary_message(message);
        }),
    );
}
