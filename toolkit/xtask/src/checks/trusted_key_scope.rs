//! Syntax governance for the exact enrollment verifier ownership contracts.
//! This validates key origin, not cryptographic validity or runtime freshness.
use anyhow::{Context, Result};
use std::collections::BTreeSet;
use syn::{spanned::Spanned, visit::Visit};

#[derive(Default)]
pub(super) struct Analysis {
    pub test_lines: BTreeSet<usize>,
    pub typed_contract: bool,
    pub violations: Vec<String>,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Contract {
    Legacy,
    Retained,
    ResponseOwner,
    Admitted,
    Integrity,
    TransferredRecord,
}
struct Scope<'a> {
    path: &'a str,
    result: Analysis,
    testing: bool,
    implementation: Option<String>,
    function: Option<syn::Signature>,
    admitted_bindings: BTreeSet<String>,
    sealed_retained: bool,
    sealed_response_owner: bool,
    sealed_integrity: bool,
    canonical_admitted_import: bool,
    transferred_record_origin: bool,
}
fn cfg_test(attributes: &[syn::Attribute]) -> bool {
    fn contains(meta: &syn::Meta) -> bool {
        match meta {
            syn::Meta::Path(path) => path.is_ident("test"),
            syn::Meta::List(list) if list.path.is_ident("all") || list.path.is_ident("any") => {
                use syn::parse::Parser;
                syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated
                    .parse2(list.tokens.clone())
                    .is_ok_and(|items| {
                        if list.path.is_ident("all") {
                            items.iter().any(contains)
                        } else {
                            !items.is_empty() && items.iter().all(contains)
                        }
                    })
            }
            _ => false,
        }
    }
    attributes.iter().any(|attribute| {
        attribute.path().is_ident("cfg")
            && attribute
                .parse_args::<syn::Meta>()
                .is_ok_and(|meta| contains(&meta))
    })
}
fn path_name(ty: &syn::Type) -> Option<String> {
    match ty {
        syn::Type::Reference(reference) => path_name(&reference.elem),
        syn::Type::Path(path) => path
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string()),
        _ => None,
    }
}
fn naked(expr: &syn::Expr) -> &syn::Expr {
    match expr {
        syn::Expr::Reference(reference) => naked(&reference.expr),
        syn::Expr::Paren(paren) => naked(&paren.expr),
        _ => expr,
    }
}
fn identifier(expr: &syn::Expr) -> Option<String> {
    match naked(expr) {
        syn::Expr::Path(path) if path.path.segments.len() == 1 => {
            Some(path.path.segments[0].ident.to_string())
        }
        _ => None,
    }
}
fn named_field<'a>(expr: &'a syn::Expr, name: &str) -> Option<&'a syn::Expr> {
    match naked(expr) {
        syn::Expr::Field(field) if matches!(&field.member, syn::Member::Named(member) if member == name) => {
            Some(&field.base)
        }
        _ => None,
    }
}
fn retained_package(expr: &syn::Expr) -> bool {
    let Some(setup) =
        named_field(expr, "public_key_package").and_then(|base| named_field(base, "setup"))
    else {
        return false;
    };
    match naked(setup) {
        syn::Expr::Field(field) => {
            matches!(&field.member, syn::Member::Unnamed(index) if index.index == 0)
                && identifier(&field.base).as_deref() == Some("self")
        }
        _ => false,
    }
}
fn sealed_definition(file: &syn::File, name: &str) -> bool {
    file.items.iter().any(|item| {
        let syn::Item::Struct(item) = item else { return false };
        item.ident == name && !cfg_test(&item.attrs) && !item.fields.is_empty() && item.fields.iter().all(|field| matches!(field.vis, syn::Visibility::Inherited)) && !item.attrs.iter().any(|attribute| {
            attribute.path().is_ident("derive") && attribute.parse_args_with(syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated).is_ok_and(|paths| paths.iter().any(|path| path.segments.last().is_some_and(|segment| segment.ident == "Deserialize")))
        })
    })
}
fn canonical_import(file: &syn::File) -> bool {
    fn flatten(tree: &syn::UseTree, parts: &mut Vec<String>) -> bool {
        match tree {
            syn::UseTree::Path(path) => {
                parts.push(path.ident.to_string());
                flatten(&path.tree, parts)
            }
            syn::UseTree::Name(name) => {
                parts.push(name.ident.to_string());
                parts
                    == &[
                        "super",
                        "enrollment_manifest_admission",
                        "AdmittedEnrollmentManifest",
                    ]
            }
            _ => false,
        }
    }
    file.items
        .iter()
        .any(|item| matches!(item, syn::Item::Use(item) if flatten(&item.tree, &mut Vec::new())))
}
fn transferred_record_origin(file: &syn::File) -> bool {
    struct Origin {
        testing: bool,
        admitted_pin: Option<String>,
        seen: bool,
        valid: bool,
    }
    impl<'ast> Visit<'ast> for Origin {
        fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
            let old = self.testing;
            self.testing |= cfg_test(&item.attrs);
            syn::visit::visit_item_mod(self, item);
            self.testing = old;
        }
        fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
            let old_testing = self.testing;
            self.testing |= cfg_test(&item.attrs);
            let old_pin = self.admitted_pin.take();
            if item.sig.ident == "admit_user_transfer" {
                for arg in &item.sig.inputs {
                    let syn::FnArg::Typed(arg) = arg else {
                        continue;
                    };
                    let syn::Type::Reference(reference) = arg.ty.as_ref() else {
                        continue;
                    };
                    let syn::Type::Path(path) = reference.elem.as_ref() else {
                        continue;
                    };
                    if path
                        .path
                        .segments
                        .iter()
                        .map(|segment| segment.ident.to_string())
                        .eq([
                            "aura_app",
                            "ui",
                            "workflows",
                            "ceremonies",
                            "UserTransferredEnrollmentManifest",
                        ]
                        .map(str::to_owned))
                    {
                        if let syn::Pat::Ident(name) = arg.pat.as_ref() {
                            self.admitted_pin = Some(name.ident.to_string());
                        }
                    }
                }
            }
            syn::visit::visit_item_fn(self, item);
            self.admitted_pin = old_pin;
            self.testing = old_testing;
        }
        fn visit_local(&mut self, item: &'ast syn::Local) {
            if matches!(&item.pat, syn::Pat::Ident(name) if self.admitted_pin.as_ref().is_some_and(|pin| name.ident == pin))
            {
                self.admitted_pin = None;
            }
            syn::visit::visit_local(self, item);
        }
        fn visit_expr_closure(&mut self, item: &'ast syn::ExprClosure) {
            let old = self.admitted_pin.clone();
            if !item.inputs.is_empty() {
                self.admitted_pin = None;
            }
            syn::visit::visit_expr_closure(self, item);
            self.admitted_pin = old;
        }
        fn visit_expr_struct(&mut self, expr: &'ast syn::ExprStruct) {
            if !self.testing
                && expr
                    .path
                    .segments
                    .last()
                    .is_some_and(|segment| segment.ident == "AdmissionRecord")
            {
                self.seen = true;
                let valid = expr.rest.is_none() && expr.fields.iter().find(|field| matches!(&field.member, syn::Member::Named(name) if name == "selected_verifier")).is_some_and(|field| {
                    let syn::Expr::MethodCall(clone) = naked(&field.expr) else { return false };
                    if clone.method != "clone" || !clone.args.is_empty() { return false }
                    let Some(base) = named_field(&clone.receiver, "initiator_confirmation_verifier") else { return false };
                    matches!(naked(base), syn::Expr::MethodCall(manifest) if manifest.method == "manifest" && manifest.args.is_empty() && identifier(&manifest.receiver).as_ref() == self.admitted_pin.as_ref() && self.admitted_pin.is_some())
                });
                self.valid &= valid;
            }
            syn::visit::visit_expr_struct(self, expr);
        }
    }
    let private_record = file.items.iter().any(|item| matches!(item, syn::Item::Struct(item) if item.ident == "AdmissionRecord" && matches!(item.vis, syn::Visibility::Inherited) && !item.fields.is_empty() && item.fields.iter().all(|field| matches!(field.vis, syn::Visibility::Inherited))));
    let mut origin = Origin {
        testing: false,
        admitted_pin: None,
        seen: false,
        valid: true,
    };
    origin.visit_file(file);
    private_record && origin.seen && origin.valid
}

impl Scope<'_> {
    fn contract(&self) -> Contract {
        match (self.path, self.implementation.as_deref()) {
            (
                "crates/aura-agent/src/handlers/invitation/enrollment_trust.rs",
                Some("RetainedEnrollmentVerifier"),
            ) if self.sealed_retained => Contract::Retained,
            (
                "crates/aura-agent/src/handlers/invitation/enrollment_vm_admission.rs",
                Some("EnrollmentControlFrame"),
            ) if self.canonical_admitted_import => Contract::Admitted,
            (
                "crates/aura-invitation/src/enrollment_manifest.rs",
                Some("EnrollmentTrustManifest"),
            ) if self.sealed_integrity => Contract::Integrity,
            (
                "crates/aura-agent/src/handlers/invitation/enrollment_manifest_admission.rs",
                None,
            ) if self.transferred_record_origin
                && self
                    .function
                    .as_ref()
                    .is_some_and(|signature| signature.ident == "validate") =>
            {
                Contract::TransferredRecord
            }
            (
                "crates/aura-agent/src/handlers/invitation/enrollment_trust.rs",
                Some("PinnedEnrollmentResponseVerifierCapability"),
            ) if self.sealed_response_owner => Contract::ResponseOwner,
            _ => Contract::Legacy,
        }
    }
    fn key_allowed(&self, expr: &syn::Expr) -> bool {
        match self.contract() {
            Contract::Retained => retained_package(expr),
            Contract::Admitted => {
                let Some(base) = named_field(expr, "initiator_confirmation_verifier") else {
                    return false;
                };
                matches!(naked(base), syn::Expr::MethodCall(call) if call.method == "manifest" && call.args.is_empty() && identifier(&call.receiver).is_some_and(|name| self.admitted_bindings.contains(&name)))
            }
            Contract::Integrity => {
                let Some(signature) = &self.function else {
                    return false;
                };
                if signature.ident != "verify_signature"
                    || identifier(expr).as_deref() != Some("independently_supplied_verifier")
                    || !self
                        .admitted_bindings
                        .contains("independently_supplied_verifier")
                {
                    return false;
                }
                let syn::ReturnType::Type(_, result) = &signature.output else {
                    return false;
                };
                let syn::Type::Path(result) = result.as_ref() else {
                    return false;
                };
                let Some(segment) = result.path.segments.last() else {
                    return false;
                };
                let syn::PathArguments::AngleBracketed(args) = &segment.arguments else {
                    return false;
                };
                segment.ident == "Result" && args.args.iter().any(|arg| matches!(arg, syn::GenericArgument::Type(ty) if path_name(ty).as_deref() == Some("VerifiedEnrollmentManifestSignature"))) && signature.inputs.iter().any(|arg| matches!(arg, syn::FnArg::Typed(arg) if matches!(arg.pat.as_ref(), syn::Pat::Ident(name) if name.ident == "independently_supplied_verifier") && matches!(arg.ty.as_ref(), syn::Type::Reference(reference) if matches!(reference.elem.as_ref(), syn::Type::Slice(slice) if path_name(&slice.elem).as_deref() == Some("u8")))))
            }
            Contract::TransferredRecord => named_field(expr, "selected_verifier")
                .and_then(identifier)
                .is_some_and(|name| self.admitted_bindings.contains(&name)),
            Contract::ResponseOwner | Contract::Legacy => false,
        }
    }
    fn record(&mut self, span: proc_macro2::Span, safe: bool) {
        if self.result.typed_contract && !self.testing && !safe {
            self.result.violations.push(format!("{}:{} signature verifier key is not attached to the declared independent integrity or sealed enrollment owner", self.path, span.start().line));
        }
    }
    fn function(
        &mut self,
        signature: &syn::Signature,
        attributes: &[syn::Attribute],
        span: proc_macro2::Span,
        visit: impl FnOnce(&mut Self),
    ) {
        let old_testing = self.testing;
        self.testing |= cfg_test(attributes);
        if self.testing {
            self.result
                .test_lines
                .extend(span.start().line..=span.end().line);
        }
        let old_function = self.function.replace(signature.clone());
        let old_bindings = std::mem::take(&mut self.admitted_bindings);
        for arg in &signature.inputs {
            if let syn::FnArg::Typed(arg) = arg {
                if matches!(arg.ty.as_ref(), syn::Type::Reference(reference) if matches!(reference.elem.as_ref(), syn::Type::Path(path) if path.path.is_ident("AdmittedEnrollmentManifest")))
                    || matches!(arg.ty.as_ref(), syn::Type::Reference(reference) if matches!(reference.elem.as_ref(), syn::Type::Path(path) if path.path.is_ident("AdmissionRecord")))
                    || matches!(arg.pat.as_ref(), syn::Pat::Ident(name) if name.ident == "independently_supplied_verifier" && name.mutability.is_none())
                {
                    if let syn::Pat::Ident(name) = arg.pat.as_ref() {
                        self.admitted_bindings.insert(name.ident.to_string());
                    }
                }
            }
        }
        visit(self);
        self.admitted_bindings = old_bindings;
        self.function = old_function;
        self.testing = old_testing;
    }
}
impl<'ast> Visit<'ast> for Scope<'_> {
    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        let old = self.testing;
        self.testing |= cfg_test(&item.attrs);
        if self.testing {
            self.result
                .test_lines
                .extend(item.span().start().line..=item.span().end().line);
        }
        syn::visit::visit_item_mod(self, item);
        self.testing = old;
    }
    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        let old = self
            .implementation
            .replace(path_name(&item.self_ty).unwrap_or_default());
        syn::visit::visit_item_impl(self, item);
        self.implementation = old;
    }
    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        self.function(&item.sig, &item.attrs, item.span(), |scope| {
            syn::visit::visit_item_fn(scope, item)
        });
    }
    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        self.function(&item.sig, &item.attrs, item.span(), |scope| {
            syn::visit::visit_impl_item_fn(scope, item)
        });
    }
    fn visit_local(&mut self, item: &'ast syn::Local) {
        struct Bindings(BTreeSet<String>);
        impl<'ast> Visit<'ast> for Bindings {
            fn visit_pat_ident(&mut self, pattern: &'ast syn::PatIdent) {
                self.0.insert(pattern.ident.to_string());
                syn::visit::visit_pat_ident(self, pattern);
            }
        }
        let mut bindings = Bindings(BTreeSet::new());
        bindings.visit_pat(&item.pat);
        for name in bindings.0 {
            self.admitted_bindings.remove(&name);
        }
        syn::visit::visit_local(self, item);
    }
    fn visit_expr_closure(&mut self, item: &'ast syn::ExprClosure) {
        let old = self.admitted_bindings.clone();
        if !item.inputs.is_empty() {
            self.admitted_bindings.clear();
        }
        syn::visit::visit_expr_closure(self, item);
        self.admitted_bindings = old;
    }
    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        let name = call.method.to_string();
        let key_index = match name.as_str() {
            "ed25519_verify" => Some(2),
            "verify_signature" if named_field(&call.receiver, "manifest").is_none() => Some(2),
            "frost_verify" => Some(2),
            _ => None,
        };
        if let Some(index) = key_index {
            self.record(
                call.span(),
                call.args
                    .get(index)
                    .is_some_and(|key| self.key_allowed(key)),
            );
        }
        if name == "verify_threshold_signing_context_transcript" {
            self.record(
                call.span(),
                (self.contract() == Contract::Retained
                    && identifier(&call.receiver).as_deref() == Some("self"))
                    || (self.contract() == Contract::ResponseOwner
                        && named_field(&call.receiver, "verifier")
                            .and_then(identifier)
                            .as_deref()
                            == Some("self")),
            );
        }
        syn::visit::visit_expr_method_call(self, call);
    }
    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let syn::Expr::Path(path) = call.func.as_ref() {
            let name = path
                .path
                .segments
                .last()
                .map(|segment| segment.ident.to_string())
                .unwrap_or_default();
            if matches!(
                name.as_str(),
                "verify_ed25519_transcript"
                    | "verify_frost_transcript"
                    | "verify_threshold_signing_context_transcript"
            ) {
                self.record(
                    call.span(),
                    call.args.last().is_some_and(|key| self.key_allowed(key)),
                );
            }
        }
        syn::visit::visit_expr_call(self, call);
    }
}
pub(super) fn analyze(path: &str, source: &str) -> Result<Analysis> {
    // Policy callers currently pass absolute repository paths. Canonicalize
    // only this local contract inventory; diagnostics elsewhere keep their path.
    let canonical_path = path.to_owned();
    let canonical_path = canonical_path
        .rsplit_once("/crates/")
        .map(|(_, suffix)| format!("crates/{suffix}"))
        .unwrap_or(canonical_path);
    let path = canonical_path.as_str();

    let file = syn::parse_file(source).context("parse trusted key boundary source")?;
    let typed_contract = matches!(
        path,
        "crates/aura-agent/src/handlers/invitation/enrollment_trust.rs"
            | "crates/aura-agent/src/handlers/invitation/enrollment_vm_admission.rs"
            | "crates/aura-invitation/src/enrollment_manifest.rs"
            | "crates/aura-agent/src/handlers/invitation/enrollment_manifest_admission.rs"
    );
    let mut scope = Scope {
        path,
        result: Analysis {
            typed_contract,
            ..Default::default()
        },
        testing: false,
        implementation: None,
        function: None,
        admitted_bindings: BTreeSet::new(),
sealed_retained: sealed_definition(&file, "RetainedEnrollmentVerifier"),
sealed_response_owner: sealed_definition(&file, "PinnedEnrollmentResponseVerifierCapability") && sealed_definition(&file, "RetainedEnrollmentVerifier") && file.items.iter().any(|item| matches!(item, syn::Item::Struct(item) if item.ident == "PinnedEnrollmentResponseVerifierCapability" && item.fields.iter().any(|field| field.ident.as_ref().is_some_and(|name| name == "verifier") && matches!(&field.ty, syn::Type::Path(path) if path.path.is_ident("RetainedEnrollmentVerifier"))))),
        sealed_integrity: sealed_definition(&file, "VerifiedEnrollmentManifestSignature"),
        canonical_admitted_import: canonical_import(&file),
        transferred_record_origin: transferred_record_origin(&file),
    };
    scope.visit_file(&file);
    Ok(scope.result)
}
#[cfg(test)]
mod tests {
    use super::*;
    const ADMITTED_PATH: &str =
        "crates/aura-agent/src/handlers/invitation/enrollment_vm_admission.rs";
    fn admitted(body: &str) -> String {
        format!("use super::enrollment_manifest_admission::AdmittedEnrollmentManifest; impl EnrollmentControlFrame {{ async fn check(&self, admitted: &AdmittedEnrollmentManifest) {{ {body} }} }}")
    }
    #[test]
    fn exact_admitted_key_origin_is_required() {
        assert!(analyze(ADMITTED_PATH, &admitted("crypto.ed25519_verify(&bytes, &sig, &admitted.manifest().initiator_confirmation_verifier).await;")).unwrap().violations.is_empty());
        for body in [
            "// trusted_key resolver proves nothing\n crypto.ed25519_verify(&bytes, &sig, &self.initiator_confirmation_verifier).await;",
            "let trusted_key = proof.public_key; crypto.ed25519_verify(&bytes, &sig, &trusted_key).await;",
            "let admitted = peer; crypto.ed25519_verify(&bytes, &sig, &admitted.manifest().initiator_confirmation_verifier).await;",
        ] { assert_eq!(analyze(ADMITTED_PATH, &admitted(body)).unwrap().violations.len(), 1); }
        let wrong_import = admitted("crypto.ed25519_verify(&bytes, &sig, &admitted.manifest().initiator_confirmation_verifier).await;").replace("super::enrollment_manifest_admission", "peer");
        assert_eq!(
            analyze(ADMITTED_PATH, &wrong_import)
                .unwrap()
                .violations
                .len(),
            1
        );
    }
    #[test]
    fn lexical_tests_do_not_hide_later_production_calls() {
        let source = "#[cfg(test)] mod real_crypto_tests { fn check() { crypto.ed25519_verify(&b, &s, &peer); } }\nimpl EnrollmentControlFrame { fn prod(&self) { crypto.ed25519_verify(&b, &s, &peer); } }";
        let result = analyze(ADMITTED_PATH, source).unwrap();
        assert_eq!(result.violations.len(), 1);
        assert!(result.test_lines.contains(&1));
        assert!(!result.test_lines.contains(&2));
    }
    #[test]
    fn retained_owner_cannot_verify_a_peer_package_or_deserialized_owner() {
        let path = "crates/aura-agent/src/handlers/invitation/enrollment_trust.rs";
        let source = "struct RetainedEnrollmentVerifier(Stored); impl RetainedEnrollmentVerifier { fn verify(&self) { effects.verify_signature(&b, &s, &self.0.setup.public_key_package, mode); } }";
        assert!(analyze(path, source).unwrap().violations.is_empty());
        assert_eq!(
            analyze(
                path,
                &source.replace(
                    "self.0.setup.public_key_package",
                    "proof.public_key_package"
                )
            )
            .unwrap()
            .violations
            .len(),
            1
        );
        assert_eq!(
            analyze(
                path,
                &source.replace(
                    "struct RetainedEnrollmentVerifier",
                    "#[derive(Deserialize)] struct RetainedEnrollmentVerifier"
                )
            )
            .unwrap()
            .violations
            .len(),
            1
        );
    }
    #[test]
    fn manifest_integrity_contract_cannot_use_embedded_or_shadowed_input() {
        let path = "crates/aura-invitation/src/enrollment_manifest.rs";
        let source = "struct VerifiedEnrollmentManifestSignature { manifest: Manifest } impl EnrollmentTrustManifest { async fn verify_signature(&self, independently_supplied_verifier: &[u8]) -> Result<VerifiedEnrollmentManifestSignature, Error> { verify_ed25519_transcript(c, self, s, independently_supplied_verifier).await; } }";
        assert!(analyze(path, source).unwrap().violations.is_empty());
        assert_eq!(
            analyze(
                path,
                &source.replace(
                    "s, independently_supplied_verifier",
                    "s, &self.initiator_confirmation_verifier"
                )
            )
            .unwrap()
            .violations
            .len(),
            1
        );
        assert_eq!(analyze(path, &source.replace("{ verify_ed25519", "{ let independently_supplied_verifier = &self.initiator_confirmation_verifier; verify_ed25519")).unwrap().violations.len(), 1);
        assert_eq!(
            analyze(
                path,
                &source.replace(
                    "Result<VerifiedEnrollmentManifestSignature, Error>",
                    "Result<AdmittedEnrollmentManifest, Error>"
                )
            )
            .unwrap()
            .violations
            .len(),
            1
        );
    }
    #[test]
    fn any_test_or_production_feature_is_not_a_test_only_scope() {
        let source = "#[cfg(any(test, feature = \"live\"))] mod mixed { fn check() { crypto.ed25519_verify(&b, &s, &peer); } }";
        let result = analyze(ADMITTED_PATH, source).unwrap();
        assert_eq!(result.violations.len(), 1);
        assert!(result.test_lines.is_empty());
    }
    #[test]
    fn absolute_repository_paths_use_exact_same_contract() {
        let source = admitted("crypto.ed25519_verify(&bytes, &sig, &admitted.manifest().initiator_confirmation_verifier).await;");
        let absolute = format!("/workspace/aura/{ADMITTED_PATH}");
        assert!(analyze(&absolute, &source).unwrap().typed_contract);
        assert!(analyze(&absolute, &source).unwrap().violations.is_empty());
        assert_eq!(
            analyze(
                &absolute,
                &source.replace("admitted.manifest()", "self.manifest()")
            )
            .unwrap()
            .violations
            .len(),
            1
        );
    }
    #[test]
    fn persisted_selected_key_requires_actual_opaque_pin_constructor_origin() {
        let path = "crates/aura-agent/src/handlers/invitation/enrollment_manifest_admission.rs";
        let source = "struct AdmissionRecord { selected_verifier: Vec<u8> } fn admit_user_transfer(pin: &aura_app::ui::workflows::ceremonies::UserTransferredEnrollmentManifest) { let record = AdmissionRecord { selected_verifier: pin.manifest().initiator_confirmation_verifier.clone() }; } fn validate(record: &AdmissionRecord) { verify_ed25519_transcript(effects, &transcript, &proof.signature, &record.selected_verifier); }";
        assert!(analyze(path, source).unwrap().violations.is_empty());
        assert_eq!(
            analyze(
                path,
                &source.replace(
                    "pin.manifest().initiator_confirmation_verifier.clone()",
                    "proof.public_key.clone()"
                )
            )
            .unwrap()
            .violations
            .len(),
            1
        );
        assert_eq!(
            analyze(
                path,
                &source.replace("&record.selected_verifier", "&proof.public_key")
            )
            .unwrap()
            .violations
            .len(),
            1
        );
        assert_eq!(
            analyze(
                path,
                &source.replace(
                    "&aura_app::ui::workflows::ceremonies::UserTransferredEnrollmentManifest",
                    "&RemoteManifest"
                )
            )
            .unwrap()
            .violations
            .len(),
            1
        );
    }
    #[test]
    fn response_owner_delegates_only_to_its_actual_sealed_retained_verifier() {
        let path = "crates/aura-agent/src/handlers/invitation/enrollment_trust.rs";
        let source = "struct RetainedEnrollmentVerifier(Stored); struct PinnedEnrollmentResponseVerifierCapability { verifier: RetainedEnrollmentVerifier } impl PinnedEnrollmentResponseVerifierCapability { fn verify(&self) { self.verifier.verify_threshold_signing_context_transcript(effects, transcript, proof); } }";
        assert!(analyze(path, source).unwrap().violations.is_empty());
        assert_eq!(
            analyze(path, &source.replace("self.verifier", "peer.verifier"))
                .unwrap()
                .violations
                .len(),
            1
        );
        assert_eq!(
            analyze(
                path,
                &source.replace(
                    "verifier: RetainedEnrollmentVerifier",
                    "verifier: RemoteVerifier"
                )
            )
            .unwrap()
            .violations
            .len(),
            1
        );
        assert_eq!(
            analyze(
                path,
                &source.replace(
                    "struct PinnedEnrollmentResponseVerifierCapability",
                    "#[derive(Deserialize)] struct PinnedEnrollmentResponseVerifierCapability"
                )
            )
            .unwrap()
            .violations
            .len(),
            1
        );
    }
}
