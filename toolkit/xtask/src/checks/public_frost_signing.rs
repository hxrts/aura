//! Required audited public FROST primitive evidence; runtime ownership is separate.
use super::support::{command_stdout, repo_root};
use anyhow::{bail, Result};
use std::{collections::BTreeSet, fs};

pub fn run() -> Result<()> {
    let required = [
        "public_commitments_sign_real_two_of_three_without_remote_nonces",
        "public_signing_rejects_substituted_intent_policy_and_inventory",
    ];
    let root = repo_root()?;
    let source = fs::read_to_string(root.join("crates/aura-effects/src/crypto/public_frost.rs"))?;
    super::vm_session_lifecycle::require_tests(&source, &required)?;
    let dealer_source = fs::read_to_string(root.join("crates/aura-effects/src/crypto.rs"))?;
    super::vm_session_lifecycle::require_tests(
        &dealer_source,
        &["test_frost_key_generation_basic"],
    )?;
    let base: Vec<String> = ["test", "-p", "hxrts-aura-effects", "--lib"]
        .into_iter()
        .map(str::to_owned)
        .collect();
    let mut discovery = base.clone();
    discovery.extend(["--".into(), "--list".into()]);
    let listing = command_stdout("cargo", &discovery)?;
    let published: BTreeSet<_> = listing
        .lines()
        .filter_map(|line| line.strip_suffix(": test"))
        .collect();
    for name in required {
        let exact = format!("crypto::public_frost::tests::{name}");
        if !published.contains(exact.as_str()) {
            bail!("public-frost-signing: required test absent from actual harness: {exact}");
        }
    }
    if !published.contains("crypto::frost_tests::test_frost_key_generation_basic") {
        bail!("public-frost-signing: actual dealer regression absent from harness");
    }
    let mut dealer = base.clone();
    dealer.push("crypto::frost_tests::test_frost_key_generation_basic".into());
    let output = command_stdout("cargo", &dealer)?;
    super::vm_session_lifecycle::require_executed_tests(
        &output,
        &["crypto::frost_tests::test_frost_key_generation_basic".into()],
    )?;
    let mut tests = base;
    tests.push("crypto::public_frost::tests::".into());
    let output = command_stdout("cargo", &tests)?;
    let exact: Vec<String> = required
        .iter()
        .map(|name| format!("crypto::public_frost::tests::{name}"))
        .collect();
    super::vm_session_lifecycle::require_executed_tests(&output, &exact)?;
    let docs: Vec<String> = [
        "test",
        "-p",
        "hxrts-aura-core",
        "--doc",
        "frost_create_public_signing_package",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    let mut discovery = docs.clone();
    discovery.extend(["--".into(), "--list".into()]);
    let listing = command_stdout("cargo", &discovery)?;
    let required_docs: Vec<String> = listing
        .lines()
        .filter_map(|line| {
            line.strip_suffix(": test")
                .filter(|name| name.contains("frost_create_public_signing_package"))
                .map(str::to_owned)
        })
        .collect();
    if required_docs.len() < 2 {
        bail!("public-frost-signing: positive consumer and public-only API guard absent from actual doctest harness");
    }
    let output = command_stdout("cargo", &docs)?;
    super::vm_session_lifecycle::require_executed_tests(&output, &required_docs)?;
    require_signature_ingress_regressions(&root)?;
    require_raw_threshold_owner_boundary(&root)?;
    println!("public-frost-signing: audited primitive, ingress provenance and public-only API evidence clean");
    Ok(())
}

fn require_signature_ingress_regressions(root: &std::path::Path) -> Result<()> {
    let suites: &[(&str, &str, &str, &[&str])] = &[
        (
            "hxrts-aura-core",
            "crates/aura-core/src/crypto/signature_input.rs",
            "crypto::signature_input::tests::",
            &[
                "single_signer_ingress_parsing_preserves_native_input_failures",
                "threshold_ingress_parsing_rejects_malformed_public_package_and_signature",
            ],
        ),
        (
            "hxrts-aura-invitation",
            "crates/aura-invitation/src/enrollment_setup.rs",
            "enrollment_setup::tests::",
            &["malformed_peer_encoding_is_distinct_from_required_provider_failure"],
        ),
    ];
    for (package, source_path, prefix, functions) in suites {
        let source = fs::read_to_string(root.join(source_path))?;
        super::vm_session_lifecycle::require_tests(&source, functions)?;
        let base: Vec<String> = ["test", "-p", package, "--lib"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        let mut discovery = base.clone();
        discovery.extend(["--".into(), "--list".into()]);
        let listing = command_stdout("cargo", &discovery)?;
        let published: BTreeSet<_> = listing
            .lines()
            .filter_map(|line| line.strip_suffix(": test"))
            .collect();
        for function in *functions {
            let exact = format!("{prefix}{function}");
            if !published.contains(exact.as_str()) {
                bail!("public-frost-signing: signature ingress regression absent from actual harness: {exact}");
            }
            let mut execution = base.clone();
            execution.extend([
                exact.clone(),
                "--".into(),
                "--exact".into(),
                "--format".into(),
                "pretty".into(),
                "--test-threads=1".into(),
            ]);
            let output = command_stdout("cargo", &execution)?;
            super::vm_session_lifecycle::require_executed_tests(&output, &[exact])?;
        }
    }
    Ok(())
}

/// The raw SigningContext surface may never coordinate a quorum or aggregate
/// shares. Owned distributed round producers live in separate owner modules.
fn require_raw_threshold_owner_boundary(root: &std::path::Path) -> Result<()> {
    let source = fs::read_to_string(
        root.join("crates/aura-agent/src/runtime/services/threshold_signing.rs"),
    )?;
    validate_raw_threshold_owner_boundary(&syn::parse_file(&source)?)?;
    Ok(())
}

fn forbidden_raw_quorum_primitive(name: &str) -> bool {
    matches!(
        name,
        "frost_aggregate"
            | "frost_aggregate_signatures"
            | "frost_sign_share"
            | "frost_sign_share_for_message"
            | "frost_generate_nonces"
            | "frost_create_signing_package"
            | "frost_create_public_signing_package"
    )
}

fn validate_raw_threshold_owner_boundary(file: &syn::File) -> Result<()> {
    use syn::visit::Visit;
    #[derive(Default)]
    struct RawSurface {
        violations: Vec<String>,
    }
    impl<'ast> Visit<'ast> for RawSurface {
        fn visit_item(&mut self, item: &'ast syn::Item) {
            let attributes: &[syn::Attribute] = match item {
                syn::Item::Fn(item) => &item.attrs,
                syn::Item::Mod(item) => &item.attrs,
                syn::Item::Impl(item) => &item.attrs,
                syn::Item::Const(item) => &item.attrs,
                syn::Item::Static(item) => &item.attrs,
                _ => &[],
            };
            if super::policy::is_rust_test_only(attributes) {
                return;
            }
            syn::visit::visit_item(self, item);
        }
        fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
            if super::policy::is_rust_test_only(&item.attrs) {
                return;
            }
            syn::visit::visit_impl_item_fn(self, item);
        }
        fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
            if let syn::Expr::Path(path) = call.func.as_ref() {
                if let Some(segment) = path.path.segments.last() {
                    let name = segment.ident.to_string();
                    if forbidden_raw_quorum_primitive(&name) {
                        self.violations.push(name);
                    }
                }
            }
            syn::visit::visit_expr_call(self, call);
        }
        fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
            let name = call.method.to_string();
            if forbidden_raw_quorum_primitive(&name) {
                self.violations.push(name);
            }
            syn::visit::visit_expr_method_call(self, call);
        }
    }
    let mut boundary = RawSurface::default();
    boundary.visit_file(file);
    if !boundary.violations.is_empty() {
        bail!("public-frost-signing: raw SigningContext service cannot own distributed quorum primitives: {:?}", boundary.violations);
    }
    Ok(())
}

#[cfg(test)]
mod raw_threshold_owner_boundary_tests {
    use super::validate_raw_threshold_owner_boundary;
    #[test]
    fn raw_context_cannot_regain_private_share_aggregation_via_renamed_helper() -> anyhow::Result<()>
    {
        let invalid = syn::parse_file("impl RawService { async fn renamed(&self) { self.effects.frost_aggregate(&package, &shares, &public).await; } }")?;
        assert!(validate_raw_threshold_owner_boundary(&invalid).is_err());
        for expression in [
            "effects.frost_aggregate_signatures(&package, &shares)",
            "effects.frost_create_signing_package(&message, &nonces, &public, 2)",
            "CryptoExtendedEffects::frost_aggregate_signatures(effects, &package, &shares)",
            "<Effects as CryptoExtendedEffects>::frost_create_signing_package(effects, &message, &nonces, &public, 2)",
        ] {
            let source = format!("impl RawService {{ async fn renamed(&self) {{ {expression}.await; }} }}");
            assert!(validate_raw_threshold_owner_boundary(&syn::parse_file(&source)?).is_err());
        }
        let mixed = syn::parse_file("#[cfg(any(test, feature = \"production\"))] mod tests { async fn fixture() { effects.frost_aggregate_signatures(&package, &shares).await; } }")?;
        assert!(validate_raw_threshold_owner_boundary(&mixed).is_err());
        let actual_test = syn::parse_file("#[cfg(all(unix, test))] impl RawService { async fn fixture() { effects.frost_aggregate_signatures(&package, &shares).await; } }")?;
        validate_raw_threshold_owner_boundary(&actual_test)?;
        let valid = syn::parse_file("impl RawService { async fn sign(&self) { self.require_actual_local_material().await; } } #[cfg(test)] mod tests { async fn fixture(effects: Effects) { effects.frost_aggregate(&package, &shares, &public).await; } }")?;
        validate_raw_threshold_owner_boundary(&valid)?;
        Ok(())
    }
}
