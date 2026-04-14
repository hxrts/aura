use rustc_errors::DiagDecorator;
use rustc_hir::{Expr, ExprKind, QPath};
use rustc_lint::{LateContext, LateLintPass, LintContext};

use crate::support::{normalized_source_path, path_matches_scope, qpath_def_path, SeenSpans};

const SCOPES: &[&str] = &[
    "/crates/aura-terminal/src/",
    "/crates/aura-web/src/",
    "/crates/aura-harness/src/",
];

const VARIANT_SUFFIXES: &[&str] = &[
    "AuthoritativeSemanticFact::OperationStatus",
    "AuthoritativeSemanticFact::PendingHomeInvitationReady",
    "AuthoritativeSemanticFact::ContactLinkReady",
    "AuthoritativeSemanticFact::ChannelMembershipReady",
    "AuthoritativeSemanticFact::RecipientPeersResolved",
    "AuthoritativeSemanticFact::PeerChannelReady",
    "AuthoritativeSemanticFact::MessageDeliveryReady",
];

rustc_session::declare_lint! {
    pub HARNESS_AUTHORITATIVE_FACT_BOUNDARY,
    Deny,
    "frontend-facing modules must not construct authoritative semantic facts",
}

#[derive(Default)]
pub(crate) struct HarnessAuthoritativeFactBoundary {
    seen: SeenSpans,
}

rustc_session::impl_lint_pass!(
    HarnessAuthoritativeFactBoundary => [HARNESS_AUTHORITATIVE_FACT_BOUNDARY]
);

impl<'tcx> LateLintPass<'tcx> for HarnessAuthoritativeFactBoundary {
    fn check_expr(&mut self, cx: &LateContext<'tcx>, expr: &'tcx Expr<'tcx>) {
        if expr.span.from_expansion() {
            return;
        }
        let qpath = match expr.kind {
            ExprKind::Path(qpath) => Some(qpath),
            ExprKind::Struct(qpath, _, _) => Some(*qpath),
            _ => None,
        };
        let Some(qpath) = qpath else {
            return;
        };
        if !is_forbidden_fact_path(cx, &qpath) {
            return;
        }
        emit_if_in_scope(cx, &mut self.seen, expr.span);
    }
}

fn is_forbidden_fact_path<'tcx>(cx: &LateContext<'tcx>, qpath: &QPath<'tcx>) -> bool {
    qpath_def_path(cx, qpath)
        .is_some_and(|path| VARIANT_SUFFIXES.iter().any(|suffix| path.ends_with(suffix)))
}

fn emit_if_in_scope<'tcx>(cx: &LateContext<'tcx>, seen: &mut SeenSpans, span: rustc_span::Span) {
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
        HARNESS_AUTHORITATIVE_FACT_BOUNDARY,
        span,
        DiagDecorator(|diag| {
            diag.primary_message(
                "frontend-facing modules may not construct `AuthoritativeSemanticFact` variants outside approved Aura workflow ownership boundaries",
            );
        }),
    );
}
