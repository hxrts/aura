//! Required canonical byte origins at the three owned Guardian verifier sites.
use anyhow::Result;
use std::collections::BTreeSet;
use syn::visit::Visit;

#[derive(Default)]
pub(super) struct Analysis {
    pub lines: BTreeSet<usize>,
    pub violations: Vec<String>,
}
fn bare(expr: &syn::Expr) -> &syn::Expr {
    match expr {
        syn::Expr::Reference(expr) => bare(&expr.expr),
        syn::Expr::Paren(expr) => bare(&expr.expr),
        _ => expr,
    }
}
fn name(expr: &syn::Expr) -> Option<String> {
    match bare(expr) {
        syn::Expr::Path(path) if path.path.segments.len() == 1 => {
            Some(path.path.segments[0].ident.to_string())
        }
        _ => None,
    }
}
fn encoder(file: &syn::File) -> bool {
    file.items.iter().any(|item| {
        let syn::Item::Fn(item) = item else { return false };
        if item.sig.ident != "guardian_transcript" || item.block.stmts.len() != 1 { return false; }
        let bounded = item.sig.generics.params.iter().any(|param| matches!(param,
            syn::GenericParam::Type(param) if param.ident == "T" && param.bounds.iter().any(|bound|
                matches!(bound, syn::TypeParamBound::Trait(bound) if bound.path.is_ident("SecurityTranscript")))));
        let input = item.sig.inputs.iter().any(|arg| matches!(arg, syn::FnArg::Typed(arg)
            if matches!(arg.pat.as_ref(), syn::Pat::Ident(pat) if pat.ident == "transcript")
                && matches!(arg.ty.as_ref(), syn::Type::Reference(reference)
                    if matches!(reference.elem.as_ref(), syn::Type::Path(path) if path.path.is_ident("T")))));
        let Some(syn::Stmt::Expr(syn::Expr::MethodCall(map), None)) = item.block.stmts.last() else { return false };
        bounded && input && map.method == "map_err"
            && matches!(bare(&map.receiver), syn::Expr::MethodCall(required)
                if required.method == "required_transcript_bytes" && required.args.is_empty()
                    && name(&required.receiver).as_deref() == Some("transcript"))
    })
}
struct Scope {
    encoder: bool,
    expected: &'static str,
    declared: bool,
    confirmation_factory: bool,
    transcripts: BTreeSet<String>,
    bytes: BTreeSet<String>,
    result: Analysis,
}
impl Scope {
    fn typed(&self, expr: &syn::Expr) -> bool {
        if !self.declared {
            return false;
        }
        match bare(expr) {
            syn::Expr::Struct(expr) => expr.path.is_ident(self.expected),
            syn::Expr::Path(_) => name(expr).is_some_and(|name| self.transcripts.contains(&name)),
            syn::Expr::Call(call)
                if self.expected == "GuardianConfirmationPayload" && self.confirmation_factory =>
            {
                matches!(call.func.as_ref(), syn::Expr::Path(path) if path.path.is_ident("guardian_confirmation_payload"))
                    && call.args.len() == 1
            }
            _ => false,
        }
    }
}
impl<'ast> Visit<'ast> for Scope {
    fn visit_local(&mut self, local: &'ast syn::Local) {
        let syn::Pat::Ident(pat) = &local.pat else {
            self.bytes.clear();
            self.transcripts.clear();
            syn::visit::visit_local(self, local);
            return;
        };
        let binding = pat.ident.to_string();
        let typed = local
            .init
            .as_ref()
            .is_some_and(|init| self.typed(&init.expr));
        let encoded = self.encoder && local.init.as_ref().is_some_and(|init| {
            matches!(init.expr.as_ref(), syn::Expr::Try(required)
                if matches!(bare(&required.expr), syn::Expr::Call(call)
                    if matches!(call.func.as_ref(), syn::Expr::Path(path) if path.path.is_ident("guardian_transcript"))
                        && call.args.len() == 1 && call.args.first().is_some_and(|expr| self.typed(expr))))
        });
        self.bytes.remove(&binding);
        self.transcripts.remove(&binding);
        syn::visit::visit_local(self, local);
        if pat.mutability.is_none()
            && local
                .init
                .as_ref()
                .is_some_and(|init| init.diverge.is_none())
        {
            if typed {
                self.transcripts.insert(binding.clone());
            }
            if encoded {
                self.bytes.insert(binding);
            }
        }
    }
    fn visit_expr_closure(&mut self, expr: &'ast syn::ExprClosure) {
        let bytes = std::mem::take(&mut self.bytes);
        let transcripts = std::mem::take(&mut self.transcripts);
        syn::visit::visit_expr_closure(self, expr);
        self.bytes = bytes;
        self.transcripts = transcripts;
    }
    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        if call.method == "ed25519_verify" {
            let line = call.method.span().start().line;
            if call
                .args
                .first()
                .and_then(name)
                .is_some_and(|name| self.bytes.contains(&name))
            {
                self.result.lines.insert(line);
            } else {
                self.result.violations.push(format!(
                    "guardian.rs:{line} verifier bytes lack required canonical typed origin"
                ));
            }
        }
        syn::visit::visit_expr_method_call(self, call);
    }
}
pub(super) fn analyze(source: &str) -> Result<Analysis> {
    let file = syn::parse_file(source)?;
    let mut result = Analysis::default();
    for item in &file.items {
        let syn::Item::Fn(item) = item else { continue };
        let expected = match item.sig.ident.to_string().as_str() {
            "verify_guardian_pair_required" => "GuardianRecoveryKeyContinuityTranscript",
            "verify_guardian_confirmation_required" => "GuardianConfirmationPayload",
            "verify_guardian_possession_required" => "GuardianInvitationAcceptanceTranscript",
            _ => continue,
        };
        let declared = file.items.iter().any(|item| matches!(item, syn::Item::Impl(item)
            if item.trait_.as_ref().is_some_and(|(_, path, _)| path.is_ident("SecurityTranscript"))
                && matches!(item.self_ty.as_ref(), syn::Type::Path(path) if path.path.segments.len() == 1 && path.path.segments[0].ident == expected)));
        let confirmation_factory = file.items.iter().any(|item| matches!(item, syn::Item::Fn(item)
            if item.sig.ident == "guardian_confirmation_payload"
                && matches!(&item.sig.output, syn::ReturnType::Type(_, ty) if matches!(ty.as_ref(), syn::Type::Path(path) if path.path.is_ident("GuardianConfirmationPayload")))
                && matches!(item.block.stmts.last(), Some(syn::Stmt::Expr(syn::Expr::Struct(expr), None)) if expr.path.is_ident("GuardianConfirmationPayload"))));
        let mut scope = Scope {
            encoder: encoder(&file),
            expected,
            declared,
            confirmation_factory,
            transcripts: BTreeSet::new(),
            bytes: BTreeSet::new(),
            result: Analysis::default(),
        };
        scope.visit_block(&item.block);
        result.lines.extend(scope.result.lines);
        result.violations.extend(scope.result.violations);
    }
    Ok(result)
}

pub(super) fn analyze_in_repo(
    root: &std::path::Path,
    path: &std::path::Path,
    source: &str,
) -> Result<Analysis> {
    if path.strip_prefix(root).is_ok_and(|relative| {
        relative == std::path::Path::new("crates/aura-agent/src/handlers/invitation/guardian.rs")
    }) {
        analyze(source)
    } else {
        Ok(Analysis::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn absolute_inventory_paths_select_only_the_rooted_guardian_contract() {
        let root = std::path::Path::new("/repo");
        let source =
            include_str!("../../../../crates/aura-agent/src/handlers/invitation/guardian.rs");
        let path = root.join("crates/aura-agent/src/handlers/invitation/guardian.rs");
        assert_eq!(analyze_in_repo(root, &path, source).unwrap().lines.len(), 3);
        for path in [
            std::path::PathBuf::from(
                "/other/crates/aura-agent/src/handlers/invitation/guardian.rs",
            ),
            root.join("crates/peer/src/handlers/invitation/guardian.rs"),
        ] {
            assert!(analyze_in_repo(root, &path, source)
                .unwrap()
                .lines
                .is_empty());
        }
    }
    #[test]
    fn actual_guardian_verifiers_require_canonical_bytes_without_shadow_or_fallback() {
        let source =
            include_str!("../../../../crates/aura-agent/src/handlers/invitation/guardian.rs");
        let actual = analyze(source).unwrap();
        assert!(actual.violations.is_empty(), "{:?}", actual.violations);
        assert_eq!(actual.lines.len(), 3);
        for invalid in [
            source.replace(
                "let bytes = guardian_transcript(&transcript)?;",
                "let bytes = peer_bytes;",
            ),
            source.replace(
                "guardian_transcript(&transcript)?",
                "guardian_transcript(&transcript).unwrap_or(peer_bytes)",
            ),
            source.replace(".ed25519_verify(&bytes,", ".ed25519_verify(&peer_bytes,"),
            source.replace(
                "transcript.required_transcript_bytes().map_err",
                "peer.required_transcript_bytes().map_err",
            ),
            source.replace(
                "T: SecurityTranscript + ?Sized",
                "T: OtherTranscript + ?Sized",
            ),
            source.replace(
                "impl SecurityTranscript for GuardianRecoveryKeyContinuityTranscript",
                "impl OtherTranscript for GuardianRecoveryKeyContinuityTranscript",
            ),
            source.replace("-> GuardianConfirmationPayload", "-> OtherPayload"),
            source.replace(
                "let bytes = guardian_transcript(&transcript)?;",
                "let bytes = guardian_transcript(&transcript)?; let bytes = peer_bytes;",
            ),
        ] {
            assert!(
                !analyze(&invalid).unwrap().violations.is_empty(),
                "untyped bytes accepted"
            );
        }
    }
}
