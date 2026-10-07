//! Canonical domain fact model gate (docs/105_journal.md §2.1, §4.2.1).
//!
//! Every production `DomainFact` type must use the derive codec, declare its
//! one schema version through the derive (no in-payload version field and no
//! version embedded in the type id), and be registered in the agent
//! `FactRegistry`. Every reversible family in [`REVERSIBLE_FAMILIES`] must
//! keep a test-scope `assert_permutation_invariant` regression, and a fact
//! that carries `CausalMetadata` must belong to a listed family.
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{bail, Context, Result};
use regex::Regex;
use syn::{spanned::Spanned, visit::Visit};

use super::support::{read, repo_root, rust_files_under};

/// A fact family whose facts a later fact can reverse or overwrite.
pub(crate) struct ReversibleFamily {
    pub family: &'static str,
    /// Derived `DomainFact` types belonging to the family.
    pub fact_types: &'static [&'static str],
    /// Files holding the family's test-scope permutation regression.
    pub permutation_tests: &'static [&'static str],
}

/// Typed inventory of reversible fact families.
pub(crate) const REVERSIBLE_FAMILIES: &[ReversibleFamily] = &[
    ReversibleFamily {
        family: "home governance (moderation)",
        fact_types: &[
            "HomeMuteFact",
            "HomeUnmuteFact",
            "HomeBanFact",
            "HomeUnbanFact",
            "HomeKickFact",
            "HomeGrantModeratorFact",
            "HomeRevokeModeratorFact",
        ],
        permutation_tests: &["crates/aura-social/src/moderation/governance.rs"],
    },
    ReversibleFamily {
        family: "contacts",
        fact_types: &["ContactFact"],
        permutation_tests: &["crates/aura-relational/src/contacts.rs"],
    },
    ReversibleFamily {
        family: "web of trust",
        fact_types: &["FriendshipFact", "TrustIntroductionFact"],
        permutation_tests: &["crates/aura-relational/src/wot.rs"],
    },
    ReversibleFamily {
        family: "recovery",
        fact_types: &["RecoveryFact"],
        permutation_tests: &["crates/aura-recovery/src/state.rs"],
    },
    ReversibleFamily {
        family: "social lifecycle",
        fact_types: &["SocialFact"],
        permutation_tests: &["crates/aura-social/src/lifecycle.rs"],
    },
    ReversibleFamily {
        family: "AMP channel membership",
        fact_types: &["ChannelMembershipFact"],
        permutation_tests: &["crates/aura-amp/src/channel.rs"],
    },
    ReversibleFamily {
        family: "invitation outcomes",
        fact_types: &["InvitationFact"],
        permutation_tests: &["crates/aura-invitation/src/lifecycle.rs"],
    },
    ReversibleFamily {
        family: "chat revisions",
        fact_types: &["ChatFact"],
        permutation_tests: &["crates/aura-chat/src/revisions.rs"],
    },
];

/// A production file in a domain-fact crate that still writes JSON envelopes.
pub(crate) struct JsonCodecException {
    pub path: &'static str,
    pub reason: &'static str,
}

pub(crate) const JSON_CODEC_EXCEPTIONS: &[JsonCodecException] = &[JsonCodecException {
    path: "crates/aura-authentication/src/guardian_auth_relational.rs",
    reason: "ad hoc recovery_request/guardian notification Generic records, not DomainFact \
             payloads; typed replacement is work/8.md Task 135",
}];

const VERSION_FIELDS: &[&str] = &[
    "schema_version",
    "fact_version",
    "fact_schema_version",
    "payload_version",
    "format_version",
];

pub fn run() -> Result<()> {
    let root = repo_root()?;
    let files = workspace_sources(&root)?;
    let violations = analyze(&files, REVERSIBLE_FAMILIES, JSON_CODEC_EXCEPTIONS)?;
    if !violations.is_empty() {
        for violation in &violations {
            eprintln!("domain-fact-model: {violation}");
        }
        bail!("domain-fact-model: {} violation(s)", violations.len());
    }
    println!("domain-fact-model: clean");
    Ok(())
}

fn workspace_sources(root: &Path) -> Result<Vec<(String, String)>> {
    let crates = root.join("crates");
    let mut files = Vec::new();
    for entry in std::fs::read_dir(&crates).with_context(|| format!("reading {crates:?}"))? {
        let src = entry?.path().join("src");
        for path in rust_files_under(&src) {
            let relative = path
                .strip_prefix(root)
                .with_context(|| format!("{path:?} outside checkout"))?
                .to_string_lossy()
                .replace('\\', "/");
            files.push((relative, read(&path)?));
        }
    }
    files.sort();
    Ok(files)
}

struct DerivedFact {
    name: String,
    location: String,
    crate_name: String,
    type_id: Option<syn::Expr>,
    version_fields: Vec<String>,
    causal: bool,
}

#[derive(Default)]
struct Inventory {
    facts: Vec<DerivedFact>,
    manual_impls: Vec<String>,
    json_uses: Vec<(String, String)>,
    registered: BTreeSet<String>,
    permutation_files: BTreeSet<String>,
    str_consts: BTreeMap<String, String>,
    fn_bodies: BTreeMap<String, syn::Expr>,
}

struct FileVisitor<'a> {
    path: &'a str,
    testing: bool,
    inventory: &'a mut Inventory,
}

fn crate_of(path: &str) -> String {
    path.strip_prefix("crates/")
        .and_then(|rest| rest.split('/').next())
        .unwrap_or_default()
        .to_string()
}

fn last_ident(path: &syn::Path) -> Option<String> {
    path.segments
        .last()
        .map(|segment| segment.ident.to_string())
}

fn derives_domain_fact(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("derive")
            && attr
                .parse_args_with(
                    syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated,
                )
                .is_ok_and(|paths| {
                    paths
                        .iter()
                        .any(|path| last_ident(path).as_deref() == Some("DomainFact"))
                })
    })
}

fn domain_fact_type_id(attrs: &[syn::Attribute]) -> Option<syn::Expr> {
    attrs
        .iter()
        .filter(|attr| attr.path().is_ident("domain_fact"))
        .filter_map(|attr| {
            attr.parse_args_with(
                syn::punctuated::Punctuated::<syn::MetaNameValue, syn::Token![,]>::parse_terminated,
            )
            .ok()
        })
        .flatten()
        .find(|item| item.path.is_ident("type_id"))
        .map(|item| item.value)
}

#[derive(Default)]
struct CausalType(bool);
impl<'ast> Visit<'ast> for CausalType {
    fn visit_path_segment(&mut self, segment: &'ast syn::PathSegment) {
        self.0 |= segment.ident == "CausalMetadata";
        syn::visit::visit_path_segment(self, segment);
    }
}

fn scan_fields<'a>(
    fields: impl Iterator<Item = &'a syn::Field>,
    versions: &mut Vec<String>,
    causal: &mut CausalType,
) {
    for field in fields {
        if let Some(ident) = &field.ident {
            let name = ident.to_string();
            if VERSION_FIELDS.contains(&name.as_str()) {
                versions.push(name);
            }
        }
        causal.visit_type(&field.ty);
    }
}

impl FileVisitor<'_> {
    fn record_fact(&mut self, ident: &syn::Ident, attrs: &[syn::Attribute], data: FactData<'_>) {
        if self.testing || !derives_domain_fact(attrs) {
            return;
        }
        let mut version_fields = Vec::new();
        let mut causal = CausalType::default();
        match data {
            FactData::Struct(fields) => {
                scan_fields(fields.iter(), &mut version_fields, &mut causal)
            }
            FactData::Enum(variants) => {
                for variant in variants {
                    scan_fields(variant.fields.iter(), &mut version_fields, &mut causal);
                }
            }
        }
        self.inventory.facts.push(DerivedFact {
            name: ident.to_string(),
            location: format!("{}:{}", self.path, ident.span().start().line),
            crate_name: crate_of(self.path),
            type_id: domain_fact_type_id(attrs),
            version_fields,
            causal: causal.0,
        });
    }

    fn record_type_id_macro(&mut self, mac: &syn::Macro) {
        if last_ident(&mac.path).as_deref() != Some("define_fact_type_id") {
            return;
        }
        let mut prefix = None;
        let mut value = None;
        for token in mac.tokens.clone() {
            match token {
                proc_macro2::TokenTree::Ident(ident) if prefix.is_none() && ident != "str" => {
                    prefix = Some(ident.to_string());
                }
                proc_macro2::TokenTree::Literal(literal) if value.is_none() => {
                    if let Ok(syn::Lit::Str(lit)) = syn::parse_str::<syn::Lit>(&literal.to_string())
                    {
                        value = Some(lit.value());
                    }
                }
                _ => {}
            }
        }
        if let (Some(prefix), Some(value)) = (prefix, value) {
            let constant = format!("{}_FACT_TYPE_ID", prefix.to_uppercase());
            self.inventory.str_consts.insert(constant.clone(), value);
            let accessor: syn::Expr = syn::parse_str(&constant).expect("identifier expression");
            self.inventory
                .fn_bodies
                .insert(format!("{prefix}_fact_type_id"), accessor);
        }
    }
}

enum FactData<'a> {
    Struct(&'a syn::Fields),
    Enum(&'a syn::punctuated::Punctuated<syn::Variant, syn::Token![,]>),
}

fn item_attrs(item: &syn::Item) -> &[syn::Attribute] {
    match item {
        syn::Item::Fn(item) => &item.attrs,
        syn::Item::Mod(item) => &item.attrs,
        syn::Item::Impl(item) => &item.attrs,
        syn::Item::Trait(item) => &item.attrs,
        syn::Item::Const(item) => &item.attrs,
        syn::Item::Static(item) => &item.attrs,
        syn::Item::Struct(item) => &item.attrs,
        syn::Item::Enum(item) => &item.attrs,
        syn::Item::Type(item) => &item.attrs,
        syn::Item::Use(item) => &item.attrs,
        syn::Item::Macro(item) => &item.attrs,
        _ => &[],
    }
}

fn str_literal(expr: &syn::Expr) -> Option<String> {
    match expr {
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(lit),
            ..
        }) => Some(lit.value()),
        _ => None,
    }
}

impl<'ast> Visit<'ast> for FileVisitor<'_> {
    fn visit_item(&mut self, item: &'ast syn::Item) {
        let previous = self.testing;
        self.testing |= super::policy::is_rust_test_only(item_attrs(item));
        match item {
            syn::Item::Struct(item) => {
                self.record_fact(&item.ident, &item.attrs, FactData::Struct(&item.fields))
            }
            syn::Item::Enum(item) => {
                self.record_fact(&item.ident, &item.attrs, FactData::Enum(&item.variants))
            }
            syn::Item::Impl(item) if !self.testing => {
                if let Some((_, path, _)) = &item.trait_ {
                    if last_ident(path).as_deref() == Some("DomainFact") {
                        self.inventory.manual_impls.push(format!(
                            "{}:{}",
                            self.path,
                            item.span().start().line
                        ));
                    }
                }
            }
            syn::Item::Const(item) if !self.testing => {
                if let Some(value) = str_literal(&item.expr) {
                    self.inventory
                        .str_consts
                        .insert(item.ident.to_string(), value);
                }
            }
            syn::Item::Static(item) if !self.testing => {
                if let Some(value) = str_literal(&item.expr) {
                    self.inventory
                        .str_consts
                        .insert(item.ident.to_string(), value);
                }
            }
            syn::Item::Fn(item) if !self.testing && item.sig.inputs.is_empty() => {
                if let [syn::Stmt::Expr(expr, None)] = item.block.stmts.as_slice() {
                    self.inventory
                        .fn_bodies
                        .insert(item.sig.ident.to_string(), expr.clone());
                }
            }
            syn::Item::Macro(item) if !self.testing => self.record_type_id_macro(&item.mac),
            _ => {}
        }
        syn::visit::visit_item(self, item);
        self.testing = previous;
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        let previous = self.testing;
        self.testing |= super::policy::is_rust_test_only(&item.attrs);
        syn::visit::visit_impl_item_fn(self, item);
        self.testing = previous;
    }

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if self.testing {
            if let syn::Expr::Path(path) = call.func.as_ref() {
                if last_ident(&path.path).as_deref() == Some("assert_permutation_invariant") {
                    self.inventory
                        .permutation_files
                        .insert(self.path.to_string());
                }
            }
        }
        syn::visit::visit_expr_call(self, call);
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        if !self.testing && call.method == "register" {
            if let Some(turbofish) = &call.turbofish {
                if let [syn::GenericArgument::Type(syn::Type::Path(ty))] =
                    turbofish.args.iter().collect::<Vec<_>>().as_slice()
                {
                    if let Some(name) = last_ident(&ty.path) {
                        self.inventory.registered.insert(name);
                    }
                }
            }
        }
        syn::visit::visit_expr_method_call(self, call);
    }

    fn visit_path(&mut self, path: &'ast syn::Path) {
        let segments: Vec<String> = path
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect();
        if !self.testing && segments.ends_with(&["FactEncoding".into(), "Json".into()]) {
            self.inventory.json_uses.push((
                self.path.to_string(),
                format!("{}:{}", self.path, path.span().start().line),
            ));
        }
        syn::visit::visit_path(self, path);
    }
}

fn resolve_type_id(expr: &syn::Expr, inventory: &Inventory, depth: usize) -> Option<String> {
    if depth > 4 {
        return None;
    }
    match expr {
        syn::Expr::Lit(_) => str_literal(expr),
        syn::Expr::Path(path) => {
            let name = last_ident(&path.path)?;
            inventory.str_consts.get(&name).cloned()
        }
        syn::Expr::Call(call) if call.args.is_empty() => {
            let syn::Expr::Path(path) = call.func.as_ref() else {
                return None;
            };
            let body = inventory.fn_bodies.get(&last_ident(&path.path)?)?;
            resolve_type_id(body, inventory, depth + 1)
        }
        syn::Expr::MethodCall(call) if call.method == "as_str" && call.args.is_empty() => {
            resolve_type_id(&call.receiver, inventory, depth + 1)
        }
        syn::Expr::Reference(reference) => resolve_type_id(&reference.expr, inventory, depth + 1),
        syn::Expr::Paren(paren) => resolve_type_id(&paren.expr, inventory, depth + 1),
        _ => None,
    }
}

pub(crate) fn analyze(
    files: &[(String, String)],
    families: &[ReversibleFamily],
    exceptions: &[JsonCodecException],
) -> Result<Vec<String>> {
    let mut inventory = Inventory::default();
    for (path, source) in files {
        let parsed = syn::parse_file(source).with_context(|| format!("parsing {path}"))?;
        FileVisitor {
            path,
            testing: false,
            inventory: &mut inventory,
        }
        .visit_file(&parsed);
    }

    let versioned = Regex::new(r"(?i)(?:^|[^a-z0-9])v?[0-9]+$").expect("static regex");
    let mut violations = Vec::new();
    for location in &inventory.manual_impls {
        violations.push(format!(
            "{location}: hand-rolled `impl DomainFact`; use #[derive(DomainFact)]"
        ));
    }

    let fact_crates: BTreeSet<&str> = inventory
        .facts
        .iter()
        .map(|fact| fact.crate_name.as_str())
        .collect();
    for (path, location) in &inventory.json_uses {
        if fact_crates.contains(crate_of(path).as_str())
            && !exceptions.iter().any(|exception| exception.path == path)
        {
            violations.push(format!(
                "{location}: JSON fact envelope in a domain-fact crate; the derive codec is DAG-CBOR only"
            ));
        }
    }
    for exception in exceptions {
        if !inventory
            .json_uses
            .iter()
            .any(|(path, _)| path == exception.path)
        {
            violations.push(format!(
                "stale JSON codec exception {} ({}): no JSON envelope remains",
                exception.path, exception.reason
            ));
        }
    }

    let mut families_by_type = BTreeMap::new();
    for family in families {
        for fact_type in family.fact_types {
            families_by_type.insert(*fact_type, family.family);
        }
    }
    for fact in &inventory.facts {
        match fact
            .type_id
            .as_ref()
            .map(|expr| resolve_type_id(expr, &inventory, 0))
        {
            None => violations.push(format!(
                "{}: {} derives DomainFact without a `type_id`",
                fact.location, fact.name
            )),
            Some(None) => violations.push(format!(
                "{}: {} type_id must be a string literal, const or zero-argument accessor",
                fact.location, fact.name
            )),
            Some(Some(type_id)) if versioned.is_match(&type_id) => violations.push(format!(
                "{}: {} type_id `{type_id}` embeds a version; declare it via `schema_version`",
                fact.location, fact.name
            )),
            Some(Some(_)) => {}
        }
        for field in &fact.version_fields {
            violations.push(format!(
                "{}: {} carries an in-payload `{field}`; the envelope schema_version is the only version",
                fact.location, fact.name
            ));
        }
        if !inventory.registered.contains(&fact.name) {
            violations.push(format!(
                "{}: {} is not registered in the agent FactRegistry (`register::<{}>`)",
                fact.location, fact.name, fact.name
            ));
        }
        if fact.causal && !families_by_type.contains_key(fact.name.as_str()) {
            violations.push(format!(
                "{}: {} carries CausalMetadata but is not in the REVERSIBLE_FAMILIES inventory",
                fact.location, fact.name
            ));
        }
    }

    let derived: BTreeSet<&str> = inventory
        .facts
        .iter()
        .map(|fact| fact.name.as_str())
        .collect();
    for family in families {
        for fact_type in family.fact_types {
            if !derived.contains(fact_type) {
                violations.push(format!(
                    "reversible family `{}` lists {fact_type}, which is not a derived DomainFact",
                    family.family
                ));
            }
        }
        if family.permutation_tests.is_empty() {
            violations.push(format!(
                "reversible family `{}` declares no permutation test file",
                family.family
            ));
        }
        for file in family.permutation_tests {
            if !inventory.permutation_files.contains(*file) {
                violations.push(format!(
                    "reversible family `{}`: {file} has no test-scope assert_permutation_invariant call",
                    family.family
                ));
            }
        }
    }
    Ok(violations)
}

#[cfg(test)]
mod tests {
    use super::*;

    const REGISTRY: &str = "fn build() { registry.register::<GoodFact>(GOOD, Box::new(R)); }";
    const GOOD: &str = r#"
        pub const GOOD_FACT_TYPE_ID: &str = "good";
        #[derive(Serialize, DomainFact)]
        #[domain_fact(type_id = GOOD_FACT_TYPE_ID, schema_version = 2, context = "context_id")]
        pub enum GoodFact { Added { context_id: ContextId, causal: CausalMetadata } }
    "#;
    const GOOD_TEST: &str = r#"
        #[cfg(test)]
        mod tests { #[test] fn order() { assert_permutation_invariant(&[1], |x| x.len()); } }
    "#;
    const FAMILY: &[ReversibleFamily] = &[ReversibleFamily {
        family: "good",
        fact_types: &["GoodFact"],
        permutation_tests: &["crates/good/src/view.rs"],
    }];

    fn check(extra: &[(&str, &str)], families: &[ReversibleFamily]) -> Vec<String> {
        let mut files = vec![
            (
                "crates/agent/src/fact_registry.rs".to_string(),
                REGISTRY.to_string(),
            ),
            ("crates/good/src/facts.rs".to_string(), GOOD.to_string()),
            ("crates/good/src/view.rs".to_string(), GOOD_TEST.to_string()),
        ];
        files.extend(
            extra
                .iter()
                .map(|(path, source)| (path.to_string(), source.to_string())),
        );
        analyze(&files, families, &[]).unwrap()
    }

    fn assert_fires(violations: &[String], needle: &str) {
        assert!(
            violations
                .iter()
                .any(|violation| violation.contains(needle)),
            "expected `{needle}` in {violations:?}"
        );
    }

    #[test]
    fn canonical_fixture_is_clean() {
        assert_eq!(check(&[], FAMILY), Vec::<String>::new());
    }

    #[test]
    fn hand_rolled_codec_fires() {
        let source = "impl DomainFact for GoodFact { fn type_id(&self) -> &'static str { \"x\" } }";
        assert_fires(
            &check(&[("crates/good/src/codec.rs", source)], FAMILY),
            "hand-rolled",
        );
        let test_only = "#[cfg(test)] mod t { impl DomainFact for X {} }";
        assert_eq!(
            check(&[("crates/good/src/t.rs", test_only)], FAMILY),
            Vec::<String>::new()
        );
        let mixed = "#[cfg(any(test, feature = \"x\"))] impl DomainFact for X {}";
        assert_fires(
            &check(&[("crates/good/src/m.rs", mixed)], FAMILY),
            "hand-rolled",
        );
    }

    #[test]
    fn json_codec_fires_in_fact_crates_only() {
        let decode =
            "fn d(e: E) { match e.encoding { FactEncoding::Json => json(), _ => cbor() } }";
        assert_fires(
            &check(&[("crates/good/src/decode.rs", decode)], FAMILY),
            "JSON fact envelope",
        );
        let encode = "fn e() -> E { E { encoding: aura_core::types::facts::FactEncoding::Json } }";
        assert_fires(
            &check(&[("crates/good/src/encode.rs", encode)], FAMILY),
            "JSON fact envelope",
        );
        assert_eq!(
            check(&[("crates/other/src/x.rs", encode)], FAMILY),
            Vec::<String>::new()
        );
    }

    #[test]
    fn stale_json_exception_fires() {
        let files = vec![
            (
                "crates/agent/src/fact_registry.rs".to_string(),
                REGISTRY.to_string(),
            ),
            ("crates/good/src/facts.rs".to_string(), GOOD.to_string()),
            ("crates/good/src/view.rs".to_string(), GOOD_TEST.to_string()),
        ];
        let exception = [JsonCodecException {
            path: "crates/good/src/facts.rs",
            reason: "fixture",
        }];
        assert_fires(
            &analyze(&files, FAMILY, &exception).unwrap(),
            "stale JSON codec exception",
        );
    }

    #[test]
    fn versioned_type_id_fires() {
        for type_id in ["\"chat/v1\"", "\"aura.authenticate.v2\"", "\"chat_2\""] {
            let source = format!(
                "#[derive(DomainFact)] #[domain_fact(type_id = {type_id}, schema_version = 1)] \
                 pub struct VFact {{ context_id: ContextId }}\n\
                 fn r() {{ registry.register::<VFact>(X, R); }}"
            );
            assert_fires(
                &check(&[("crates/good/src/v.rs", &source)], FAMILY),
                "embeds a version",
            );
        }
        let accessor = "pub fn v_fact_type_id() -> &'static str { V.as_str() }\n\
             pub static V: FactTypeId = \"v-fact\";\n\
             #[derive(DomainFact)] #[domain_fact(type_id = v_fact_type_id(), schema_version = 1)] \
             pub struct VFact { context_id: ContextId }\n\
             fn r() { registry.register::<VFact>(X, R); }";
        assert_eq!(
            check(&[("crates/good/src/v.rs", accessor)], FAMILY),
            Vec::<String>::new()
        );
        let unresolved = "#[derive(DomainFact)] #[domain_fact(type_id = make(1), schema_version = 1)] \
             pub struct VFact { context_id: ContextId }\nfn r() { registry.register::<VFact>(X, R); }";
        assert_fires(
            &check(&[("crates/good/src/v.rs", unresolved)], FAMILY),
            "must be a string literal",
        );
    }

    #[test]
    fn in_payload_version_field_fires() {
        let source = "#[derive(DomainFact)] #[domain_fact(type_id = \"p\", schema_version = 1)] \
             pub enum PFact { Joined { context_id: ContextId, schema_version: u16 } }\n\
             fn r() { registry.register::<PFact>(X, R); }";
        assert_fires(
            &check(&[("crates/good/src/p.rs", source)], FAMILY),
            "in-payload `schema_version`",
        );
    }

    #[test]
    fn unregistered_fact_fires() {
        let source = "#[derive(Debug, aura_macros::DomainFact)] \
             #[domain_fact(type_id = \"u\", schema_version = 1)] pub struct UFact { context_id: ContextId }\n\
             #[cfg(test)] fn t() { registry.register::<UFact>(X, R); }";
        assert_fires(
            &check(&[("crates/good/src/u.rs", source)], FAMILY),
            "not registered",
        );
    }

    #[test]
    fn reversible_family_rules_fire() {
        let causal = "#[derive(DomainFact)] #[domain_fact(type_id = \"c\", schema_version = 1)] \
             pub struct CFact { context_id: ContextId, causal: Option<aura_core::time::CausalMetadata> }\n\
             fn r() { registry.register::<CFact>(X, R); }";
        assert_fires(
            &check(&[("crates/good/src/c.rs", causal)], FAMILY),
            "not in the REVERSIBLE_FAMILIES inventory",
        );
        let missing_type = [ReversibleFamily {
            family: "ghost",
            fact_types: &["GhostFact"],
            permutation_tests: &["crates/good/src/view.rs"],
        }];
        assert_fires(&check(&[], &missing_type), "not a derived DomainFact");
        let missing_test = [ReversibleFamily {
            family: "good",
            fact_types: &["GoodFact"],
            permutation_tests: &["crates/good/src/facts.rs"],
        }];
        assert_fires(
            &check(&[], &missing_test),
            "no test-scope assert_permutation_invariant",
        );
        let production_call = [ReversibleFamily {
            family: "good",
            fact_types: &["GoodFact"],
            permutation_tests: &["crates/good/src/prod.rs"],
        }];
        let prod = "fn not_a_test() { assert_permutation_invariant(&[1], |x| x.len()); }";
        assert_fires(
            &check(&[("crates/good/src/prod.rs", prod)], &production_call),
            "no test-scope assert_permutation_invariant",
        );
    }

    #[test]
    fn actual_workspace_is_clean() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let files = workspace_sources(&root).unwrap();
        assert!(files.len() > 100, "workspace sources discovered");
        let violations = analyze(&files, REVERSIBLE_FAMILIES, JSON_CODEC_EXCEPTIONS).unwrap();
        assert_eq!(violations, Vec::<String>::new());
    }
}
