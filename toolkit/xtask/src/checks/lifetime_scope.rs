//! First-party syntax fence for trusted allocation lifetime implementation seams.
//! Cryptographic validity and physical custody remain runtime/type contracts.
use anyhow::{Context, Result};
use syn::{spanned::Spanned, visit::Visit};

pub(super) fn analyze_in_repo(
    root: &std::path::Path,
    path: &std::path::Path,
    source: &str,
) -> Result<Vec<String>> {
    let relative = path.strip_prefix(root).with_context(|| {
        format!(
            "allocation lifetime source {} outside {}",
            path.display(),
            root.display()
        )
    })?;
    analyze(&relative.to_string_lossy(), source)
}

pub(super) fn verify_rooted_path_boundary() -> Result<()> {
    let root = std::path::Path::new("/checkout/aura");
    let source = "impl FilesystemProfileStorageHandler { fn acquire_owned_native(&self) { SecretLifetimeProviderIdentity::new_trusted_provider_identity(); } }";
    anyhow::ensure!(
        analyze_in_repo(
            root,
            &root.join("crates/aura-effects/src/profile_storage.rs"),
            source
        )?
        .is_empty(),
        "rooted lifetime owner path must retain its sanctioned identity"
    );
    anyhow::ensure!(
        analyze_in_repo(
            root,
            &root.join("crates/aura-effects/src/unrelated.rs"),
            source
        )?
        .len()
            == 1,
        "rooted foreign lifetime path must reject owner impersonation"
    );
    anyhow::ensure!(
        analyze_in_repo(
            root,
            std::path::Path::new("/other/crates/aura-effects/src/profile_storage.rs"),
            source
        )
        .is_err(),
        "outside-checkout lifetime paths must fail closed"
    );
    Ok(())
}

fn test_scope(attributes: &[syn::Attribute]) -> bool {
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
fn type_named(ty: &syn::Type, expected: &str) -> bool {
    match ty {
        syn::Type::Reference(reference) => type_named(&reference.elem, expected),
        syn::Type::Path(path) if path.path.segments.len() == 1 => path
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == expected),
        _ => false,
    }
}
struct Scope<'a> {
    path: &'a str,
    testing: bool,
    implementation: Option<String>,
    signature: Option<syn::Signature>,
    attributes: Vec<syn::Attribute>,
    violations: Vec<String>,
}
impl Scope<'_> {
    fn owner(&self, implementation: &str, name: &str) -> bool {
        self.implementation.as_deref() == Some(implementation)
            && self
                .signature
                .as_ref()
                .is_some_and(|signature| signature.ident == name)
    }
    fn input(&self, capability: &str) -> bool {
        fn attached(ty: &syn::Type, name: &str) -> bool {
            match ty {
                syn::Type::Reference(reference) => attached(&reference.elem, name),
                syn::Type::Path(path) => {
                    let names: Vec<_> = path
                        .path
                        .segments
                        .iter()
                        .map(|segment| segment.ident.to_string())
                        .collect();
                    names == [name]
                        || ((name == "OwnedSecretNegativeCapability"
                            || name == "OwnedSecretPositiveCapability")
                            && names == ["crate", "runtime", "effects", name])
                }
                _ => false,
            }
        }
        self.signature.as_ref().is_some_and(|signature| signature.inputs.iter().any(|input| matches!(input,syn::FnArg::Typed(argument) if attached(&argument.ty,capability))))
    }
    fn registry_decision(&self, name: &str, capability: &str) -> bool {
        self.owner("SelectedProfileSecretInventory", name)
            && self.input(capability)
            && self.native_result(None)
            && self.metadata(
                "capability_boundary",
                &[
                    ("category", "capability_gated"),
                    ("capability", capability),
                    ("family", "runtime_helper"),
                ],
            )
    }
    fn violation(&mut self, expr: &impl Spanned, seam: &str) {
        self.violations.push(format!(
            "{}:{}: allocation lifetime {seam} escapes its original typed owner",
            self.path,
            expr.span().start().line
        ));
    }
    fn check_seam(&mut self, expression: &impl Spanned, name: &str) {
        if self.testing {
            return;
        }
        let core = self.path == "crates/aura-core/src/effects/secret_lifetime.rs";
        let registry = self.path == "crates/aura-agent/src/runtime/subsystems/crypto.rs";
        let approved = match name {
            "from_trusted_provider" => Some(self.provider_issuer()),
            "new_trusted_provider_identity" => Some(
                self.path == "crates/aura-effects/src/profile_storage.rs"
                    && self.owner("FilesystemProfileStorageHandler", "acquire_owned_native"),
            ),
            "into_selected_profile_lifetime_channel" => Some(
                self.path == "crates/aura-agent/src/runtime/effects.rs"
                    && self.owner("AuraEffectSystem", "build_internal_owned"),
            ),
            "decide_positive" => Some(
                (core && self.core_decision("decide_positive"))
                    || (registry
                        && self
                            .registry_decision("seal_positive", "OwnedSecretPositiveCapability")),
            ),
            "decide_negative" => Some(
                (core && self.core_decision("decide_negative"))
                    || (registry
                        && self
                            .registry_decision("retire_original", "OwnedSecretNegativeCapability")),
            ),
            "acknowledge_retirement" => {
                Some(core && self.owner("NegativeSecretDecisionCapability", "retire"))
            }
            _ => None,
        };
        if approved == Some(false) {
            self.violation(expression, name);
        }
    }
    fn metadata(&self, attribute: &str, expected: &[(&str, &str)]) -> bool {
        self.attributes.iter().any(|attr| {
            let names: Vec<_> = attr
                .path()
                .segments
                .iter()
                .map(|segment| segment.ident.to_string())
                .collect();
            if names != ["aura_macros", attribute] {
                return false;
            }
            let mut values = std::collections::BTreeMap::new();
            let parsed = attr.parse_nested_meta(|meta| {
                let Some(name) = meta.path.get_ident() else {
                    return Err(meta.error("expected declaration key"));
                };
                if name == "capability_type" || name == "receiver_type" {
                    let ty = meta.value()?.parse::<syn::Type>()?;
                    let expected_name = if name == "capability_type" {
                        expected
                            .iter()
                            .find(|(key, _)| *key == "capability")
                            .map(|(_, value)| *value)
                    } else {
                        self.implementation.as_deref()
                    };
                    let exact = expected_name.is_some_and(|name| {
                        if type_named(&ty, name) {
                            return true;
                        }
                        let syn::Type::Path(path) = &ty else {
                            return false;
                        };
                        let actual: Vec<_> = path
                            .path
                            .segments
                            .iter()
                            .map(|segment| segment.ident.to_string())
                            .collect();
                        (name == "OwnedSecretNegativeCapability"
                            || name == "OwnedSecretPositiveCapability")
                            && actual == ["crate", "runtime", "effects", name]
                            || name == "ProfileSecretLifetimeRecoveryCapability"
                                && actual == ["aura_core", "effects", "secret_lifetime", name]
                    });
                    if !exact {
                        return Err(meta.error("exact typed declaration attachment required"));
                    }
                    return Ok(());
                }
                let value = meta.value()?.parse::<syn::LitStr>()?;

                if values.insert(name.to_string(), value.value()).is_some() {
                    return Err(meta.error("duplicate declaration key"));
                }
                Ok(())
            });
            parsed.is_ok()
                && expected
                    .iter()
                    .all(|(name, value)| values.get(*name).is_some_and(|actual| actual == value))
        })
    }
    fn native_result(&self, success: Option<&str>) -> bool {
        self.signature.as_ref().is_some_and(|signature| {
            let syn::ReturnType::Type(_, ty) = &signature.output else {
                return false;
            };
            let syn::Type::Path(path) = ty.as_ref() else {
                return false;
            };
            if path.path.segments.len() != 1 {
                return false;
            }
            let Some(segment) = path.path.segments.last() else {
                return false;
            };
            if segment.ident != "Result" {
                return false;
            }
            let syn::PathArguments::AngleBracketed(args) = &segment.arguments else {
                return false;
            };
            let mut arguments = args.args.iter();
            let Some(syn::GenericArgument::Type(value)) = arguments.next() else {
                return false;
            };
            let Some(syn::GenericArgument::Type(error)) = arguments.next() else {
                return false;
            };
            arguments.next().is_none()
                && type_named(error, "AuraError")
                && match success {
                    Some(name) => type_named(value, name),
                    None => matches!(value,syn::Type::Tuple(tuple) if tuple.elems.is_empty()),
                }
        })
    }
    fn output(&self, expected: &str) -> bool {
        self.native_result(Some(expected)) || self.signature.as_ref().is_some_and(|signature| matches!(&signature.output,syn::ReturnType::Type(_,ty) if type_named(ty,expected)))
    }
    fn core_decision(&self, method: &str) -> bool {
        self.owner("SecretLifetimeOwner", method) && self.signature.as_ref().is_some_and(|signature| {
            signature.inputs.iter().any(|input| matches!(input,syn::FnArg::Receiver(receiver) if receiver.reference.is_some() && receiver.mutability.is_none()))
            && signature.inputs.iter().any(|input| matches!(input,syn::FnArg::Typed(argument) if matches!(argument.ty.as_ref(), syn::Type::Reference(reference) if matches!(reference.elem.as_ref(), syn::Type::Slice(slice) if type_named(&slice.elem,"u8")))))
        }) && self.native_result(if method == "decide_negative" { Some("NegativeSecretDecisionCapability") } else { None })
    }
    fn provider_issuer(&self) -> bool {
        self.path == "crates/aura-effects/src/secure/allocation_lifetime.rs"
            && self.implementation.is_none()
            && self
                .signature
                .as_ref()
                .is_some_and(|signature| signature.ident == "selected_profile_channel")
            && self.input("ProfileOwnedSecureStorage")
            && self.output("ProfileSecretLifetimeRecoveryCapability")
            && self.metadata("authoritative_source", &[("kind", "proof_issuer")])
            && self.metadata(
                "capability_boundary",
                &[
                    ("category", "capability_gated"),
                    ("capability", "ProfileSecretLifetimeRecoveryCapability"),
                    ("family", "proof_issuer"),
                ],
            )
    }
}
impl<'ast> Visit<'ast> for Scope<'_> {
    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        let previous = self.testing;
        self.testing |= test_scope(&item.attrs);
        syn::visit::visit_item_mod(self, item);
        self.testing = previous;
    }
    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        let previous = self.implementation.take();
        self.implementation = if let syn::Type::Path(path) = item.self_ty.as_ref() {
            path.path
                .segments
                .last()
                .map(|segment| segment.ident.to_string())
        } else {
            None
        };
        let testing = self.testing;
        self.testing |= test_scope(&item.attrs);
        syn::visit::visit_item_impl(self, item);
        self.testing = testing;
        self.implementation = previous;
    }
    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        let previous = (
            self.testing,
            self.signature.take(),
            std::mem::take(&mut self.attributes),
        );
        self.testing |= test_scope(&item.attrs);
        self.signature = Some(item.sig.clone());
        self.attributes = item.attrs.clone();
        syn::visit::visit_item_fn(self, item);
        (self.testing, self.signature, self.attributes) = previous;
    }
    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        let previous = (
            self.testing,
            self.signature.take(),
            std::mem::take(&mut self.attributes),
        );
        self.testing |= test_scope(&item.attrs);
        self.signature = Some(item.sig.clone());
        self.attributes = item.attrs.clone();
        syn::visit::visit_impl_item_fn(self, item);
        (self.testing, self.signature, self.attributes) = previous;
    }
    fn visit_expr_method_call(&mut self, expr: &'ast syn::ExprMethodCall) {
        self.check_seam(expr, &expr.method.to_string());
        syn::visit::visit_expr_method_call(self, expr);
    }
    fn visit_expr_path(&mut self, expr: &'ast syn::ExprPath) {
        if let Some(segment) = expr.path.segments.last() {
            self.check_seam(expr, &segment.ident.to_string());
        }
        syn::visit::visit_expr_path(self, expr);
    }
    fn visit_macro(&mut self, item: &'ast syn::Macro) {
        // Token trees retain factory/UFCS names inside opaque macro expansion.
        // This fence supplements the typed declaration and private API checks.
        fn names(tokens: proc_macro2::TokenStream, output: &mut Vec<proc_macro2::Ident>) {
            for token in tokens {
                match token {
                    proc_macro2::TokenTree::Group(group) => names(group.stream(), output),
                    proc_macro2::TokenTree::Ident(ident) => output.push(ident),
                    _ => {}
                }
            }
        }
        let mut identifiers = Vec::new();
        names(item.tokens.clone(), &mut identifiers);
        for ident in identifiers {
            self.check_seam(&ident, &ident.to_string());
        }
        syn::visit::visit_macro(self, item);
    }
}
pub(super) fn analyze(path: &str, source: &str) -> Result<Vec<String>> {
    let syntax = syn::parse_file(source)
        .with_context(|| format!("parsing allocation lifetime scope {path}"))?;
    let mut scope = Scope {
        path,
        testing: false,
        implementation: None,
        signature: None,
        attributes: Vec::new(),
        violations: Vec::new(),
    };
    scope.visit_file(&syntax);
    Ok(scope.violations)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rooted_paths_preserve_exact_owner_boundary() {
        verify_rooted_path_boundary().expect("gate path self-check");
        let root = std::path::Path::new("/checkout/aura");
        let source = "impl FilesystemProfileStorageHandler { fn acquire_owned_native(&self) { SecretLifetimeProviderIdentity::new_trusted_provider_identity(); } }";
        let owned = root.join("crates/aura-effects/src/profile_storage.rs");
        assert!(analyze_in_repo(root, &owned, source)
            .expect("owned source")
            .is_empty());
        let foreign = root.join("crates/aura-effects/src/unrelated.rs");
        assert_eq!(
            analyze_in_repo(root, &foreign, source)
                .expect("foreign source")
                .len(),
            1
        );
        assert!(analyze_in_repo(
            root,
            std::path::Path::new("/other/crates/aura-effects/src/profile_storage.rs"),
            source
        )
        .is_err());
    }
    #[test]
    fn registry_negative_requires_actual_capability_type() {
        let path = "crates/aura-agent/src/runtime/subsystems/crypto.rs";
        let source = "impl SelectedProfileSecretInventory { #[aura_macros::capability_boundary(category = \"capability_gated\", capability = \"OwnedSecretNegativeCapability\", family = \"runtime_helper\")] async fn retire_original(&self, authority: &crate::runtime::effects::OwnedSecretNegativeCapability) -> Result<(),AuraError> { original.decide_negative(authority.decision()).await; } }";
        assert!(analyze(path, source).expect("valid syntax").is_empty());
        assert_eq!(
            analyze(
                path,
                &source.replace("OwnedSecretNegativeCapability", "String")
            )
            .expect("valid syntax")
            .len(),
            1
        );
        assert_eq!(
            analyze(path, &source.replace("retire_original", "unowned_cleanup"))
                .expect("valid syntax")
                .len(),
            1
        );
    }
    #[test]
    fn provider_declaration_and_test_scope_are_required() {
        let source = "#[aura_macros::capability_boundary(category = \"capability_gated\", capability = \"ProfileSecretLifetimeRecoveryCapability\", family = \"proof_issuer\")] #[aura_macros::authoritative_source(kind = \"proof_issuer\")] fn selected_profile_channel(owned: &ProfileOwnedSecureStorage) -> Result<ProfileSecretLifetimeRecoveryCapability,AuraError> { Alias::from_trusted_provider(backend); }";
        let path = "crates/aura-effects/src/secure/allocation_lifetime.rs";
        assert!(analyze(path, source).expect("valid syntax").is_empty());
        for replacement in [
            source.replace("kind = \"proof_issuer\"", "kind = \"selector\""),
            source.replace(
                "capability = \"ProfileSecretLifetimeRecoveryCapability\"",
                "capability = \"Lookalike\"",
            ),
            source.replace("selected_profile_channel", "unrelated"),
            source.replace(
                "Result<ProfileSecretLifetimeRecoveryCapability,AuraError>",
                "Result<(),ProfileSecretLifetimeRecoveryCapability>",
            ),
            source.replace("&ProfileOwnedSecureStorage", "&ObservedProfile"),
            source.replace(
                "&ProfileOwnedSecureStorage",
                "&lookalike::ProfileOwnedSecureStorage",
            ),
            source.replace(
                "aura_macros::authoritative_source",
                "lookalike::authoritative_source",
            ),
            format!("impl Unrelated {{ {source} }}"),
        ] {
            assert_eq!(
                analyze(path, &replacement)
                    .expect("valid adversarial syntax")
                    .len(),
                1
            );
        }
        assert_eq!(
            analyze("crates/aura-effects/src/secure/unrelated.rs", source)
                .expect("valid syntax")
                .len(),
            1
        );
        assert!(analyze(
            path,
            "#[cfg(all(test, unix))] mod tests { fn f() { root.decide_negative(bytes); } }"
        )
        .expect("valid syntax")
        .is_empty());
        assert_eq!(
            analyze(
                path,
                "#[cfg(any(test, unix))] mod prod { fn f() { root.decide_negative(bytes); } }"
            )
            .expect("valid syntax")
            .len(),
            1
        );
    }
    #[test]
    fn aliases_ufcs_and_opaque_macros_cannot_hide_factory_or_decision() {
        let path = "crates/aura-terminal/src/bypass.rs";
        for source in [
            "fn f() { let factory = Alias::from_trusted_provider; factory(backend); }",
            "fn f() { let decide = SecretLifetimeOwner::decide_negative; decide(owner, bytes); }",
            "fn f() { hidden![Alias::from_trusted_provider(backend)]; }",
            "fn f() { hidden![nested![owner.acknowledge_retirement(bytes)]]; }",
        ] {
            assert_eq!(
                analyze(path, source)
                    .expect("valid adversarial syntax")
                    .len(),
                1
            );
        }
    }
    #[test]
    fn provider_identity_birth_is_restricted_to_actual_profile_acquisition() {
        let source = "impl FilesystemProfileStorageHandler { fn acquire_owned_native(&self) { SecretLifetimeProviderIdentity::new_trusted_provider_identity(); } }";
        let path = "crates/aura-effects/src/profile_storage.rs";
        assert!(analyze(path, source).expect("valid syntax").is_empty());
        assert_eq!(
            analyze(
                path,
                &source.replace("acquire_owned_native", "fabricate_identity")
            )
            .expect("valid syntax")
            .len(),
            1
        );
    }
    #[test]
    fn core_decision_dispatch_requires_original_owner_signature() {
        let path = "crates/aura-core/src/effects/secret_lifetime.rs";
        let source = "impl SecretLifetimeOwner { async fn decide_negative(&self, decision: &[u8]) -> Result<NegativeSecretDecisionCapability,AuraError> { self.backend.decide_negative(decision).await; } }";
        assert!(analyze(path, source).expect("valid syntax").is_empty());
        for replacement in [
            source.replace("&self", "&mut self"),
            source.replace("&[u8]", "String"),
            source.replace("SecretLifetimeOwner", "ObservedLifetime"),
            source.replace("NegativeSecretDecisionCapability", "ObservedDecision"),
        ] {
            assert_eq!(analyze(path, &replacement).expect("valid syntax").len(), 1);
        }
    }
    #[test]
    fn optional_type_metadata_requires_exact_actual_attachment() {
        let path = "crates/aura-effects/src/secure/allocation_lifetime.rs";
        let source = r#"#[aura_macros::capability_boundary(category = "capability_gated", capability = "ProfileSecretLifetimeRecoveryCapability", capability_type = ProfileSecretLifetimeRecoveryCapability, family = "proof_issuer")] #[aura_macros::authoritative_source(kind = "proof_issuer")] fn selected_profile_channel(owned: &ProfileOwnedSecureStorage) -> Result<ProfileSecretLifetimeRecoveryCapability,AuraError> { Alias::from_trusted_provider(backend); }"#;
        assert!(analyze(path, source)
            .expect("valid typed declaration")
            .is_empty());
        assert_eq!(
            analyze(
                path,
                &source.replace(
                    "capability_type = ProfileSecretLifetimeRecoveryCapability",
                    "capability_type = ObservedProfile"
                )
            )
            .expect("valid syntax")
            .len(),
            1
        );
        let path = "crates/aura-agent/src/runtime/subsystems/crypto.rs";
        let source = r#"impl SelectedProfileSecretInventory { #[aura_macros::capability_boundary(category = "capability_gated", capability = "OwnedSecretNegativeCapability", capability_type = crate::runtime::effects::OwnedSecretNegativeCapability, receiver_type = SelectedProfileSecretInventory, family = "runtime_helper")] fn retire_original(&self,capability: &crate::runtime::effects::OwnedSecretNegativeCapability)->Result<(),AuraError> { original.decide_negative(capability.decision()); } }"#;
        assert!(analyze(path, source)
            .expect("valid original receiver attachment")
            .is_empty());
        assert_eq!(
            analyze(
                path,
                &source.replace(
                    "receiver_type = SelectedProfileSecretInventory",
                    "receiver_type = ObservedInventory"
                )
            )
            .expect("valid syntax")
            .len(),
            1
        );
    }
}
