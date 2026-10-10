//! Parsed production placeholder and frontend facade syntax checks.
use super::lint_support::{has_cfg_test_attr, is_cargo_test_target_path};
use std::path::Path;
use syn::parse::Parser;
use syn::{
    spanned::Spanned,
    visit::{self, Visit},
    Attribute, ImplItem, Item, TraitItem,
};

pub(super) fn test_attrs(attrs: &[Attribute]) -> bool {
    has_cfg_test_attr(attrs)
        || attrs
            .iter()
            .any(|a| a.path().segments.last().is_some_and(|s| s.ident == "test"))
}
pub(super) fn item_attrs(item: &Item) -> &[Attribute] {
    match item {
        Item::Const(x) => &x.attrs,
        Item::Enum(x) => &x.attrs,
        Item::ExternCrate(x) => &x.attrs,
        Item::Fn(x) => &x.attrs,
        Item::ForeignMod(x) => &x.attrs,
        Item::Impl(x) => &x.attrs,
        Item::Macro(x) => &x.attrs,
        Item::Mod(x) => &x.attrs,
        Item::Static(x) => &x.attrs,
        Item::Struct(x) => &x.attrs,
        Item::Trait(x) => &x.attrs,
        Item::TraitAlias(x) => &x.attrs,
        Item::Type(x) => &x.attrs,
        Item::Union(x) => &x.attrs,
        Item::Use(x) => &x.attrs,
        _ => &[],
    }
}
pub(super) fn impl_item_attrs(item: &ImplItem) -> &[Attribute] {
    match item {
        ImplItem::Fn(x) => &x.attrs,
        ImplItem::Const(x) => &x.attrs,
        ImplItem::Type(x) => &x.attrs,
        ImplItem::Macro(x) => &x.attrs,
        _ => &[],
    }
}
pub(super) fn trait_item_attrs(item: &TraitItem) -> &[Attribute] {
    match item {
        TraitItem::Fn(x) => &x.attrs,
        TraitItem::Const(x) => &x.attrs,
        TraitItem::Type(x) => &x.attrs,
        TraitItem::Macro(x) => &x.attrs,
        _ => &[],
    }
}
pub(super) fn scan(file: &Path, syntax: &syn::File) -> Vec<String> {
    let path = super::display_path(file);
    if is_cargo_test_target_path(file) || test_attrs(&syntax.attrs) {
        return Vec::new();
    }
    let mut scanner = Scanner {
        file,
        frontend: path.starts_with("crates/aura-terminal/src/")
            || path.contains("/crates/aura-terminal/src/"),
        violations: Vec::new(),
    };
    scanner.visit_file(syntax);
    scanner.violations
}
struct Scanner<'a> {
    file: &'a Path,
    frontend: bool,
    violations: Vec<String>,
}
impl Scanner<'_> {
    fn flag(&mut self, span: proc_macro2::Span, reason: &str) {
        self.violations.push(format!(
            "{}:{}: {reason}",
            self.file.display(),
            span.start().line
        ));
    }
    fn check_path(&mut self, names: &[String], span: proc_macro2::Span) {
        self.check_terminal_boundary(names, span);
        if self.frontend
            && names.first().is_some_and(|s| s == "aura_app")
            && names.get(1).is_some_and(|s| {
                matches!(
                    s.as_str(),
                    "workflows" | "signal_defs" | "views" | "runtime_bridge" | "authorization"
                )
            })
        {
            self.flag(
                span,
                "frontend crate-root app access is forbidden; use aura_app::ui facade",
            );
        }
    }
    fn inspect_expression(&mut self, expression: &syn::Expr) {
        let mut nested = Scanner {
            file: self.file,
            frontend: self.frontend,
            violations: Vec::new(),
        };
        nested.visit_expr(expression);
        self.violations.extend(nested.violations);
    }
    fn inspect_block(&mut self, block: &syn::Block) {
        let mut nested = Scanner {
            file: self.file,
            frontend: self.frontend,
            violations: Vec::new(),
        };
        nested.visit_block(block);
        self.violations.extend(nested.violations);
    }
    fn check_terminal_boundary(&mut self, names: &[String], span: proc_macro2::Span) {
        if !self.frontend {
            return;
        }
        let path = self.file.to_string_lossy().replace('\\', "/");
        let demo = path.contains("/demo/");
        if !demo
            && names.iter().any(|name| {
                matches!(
                    name.as_str(),
                    "FactRegistry" | "FactReducer" | "RelationalFact" | "JournalEffects"
                ) || (name.starts_with("commit_") && name.ends_with("facts"))
            })
        {
            self.flag(
                span,
                "frontend direct journal/protocol mutation is forbidden",
            );
        }
        if !demo
            && names
                .windows(2)
                .any(|pair| pair == ["RuntimeBridge", "commit"])
        {
            self.flag(span, "frontend direct runtime commit is forbidden");
        }
        if !demo
            && !path.contains("/scenarios/")
            && names.first().is_some_and(|name| {
                matches!(
                    name.as_str(),
                    "aura_journal"
                        | "aura_protocol"
                        | "aura_consensus"
                        | "aura_guards"
                        | "aura_amp"
                        | "aura_anti_entropy"
                        | "aura_transport"
                        | "aura_recovery"
                        | "aura_sync"
                        | "aura_invitation"
                        | "aura_authentication"
                        | "aura_relational"
                        | "aura_chat"
                )
            })
        {
            self.flag(
                span,
                "frontend direct protocol/domain crate access is forbidden",
            );
        }
    }
    fn use_tree(&mut self, tree: &syn::UseTree, prefix: &mut Vec<String>) {
        match tree {
            syn::UseTree::Path(p) => {
                prefix.push(p.ident.to_string());
                self.use_tree(&p.tree, prefix);
                prefix.pop();
            }
            syn::UseTree::Group(g) => {
                for x in &g.items {
                    self.use_tree(x, prefix);
                }
            }
            syn::UseTree::Name(n) => {
                prefix.push(n.ident.to_string());
                self.check_path(prefix, n.ident.span());
                prefix.pop();
            }
            syn::UseTree::Rename(n) => {
                prefix.push(n.ident.to_string());
                self.check_path(prefix, n.ident.span());
                prefix.pop();
            }
            syn::UseTree::Glob(g) => {
                self.check_path(prefix, g.span());
            }
        }
    }
}
impl<'ast> Visit<'ast> for Scanner<'_> {
    fn visit_item(&mut self, item: &'ast Item) {
        if !test_attrs(item_attrs(item)) {
            visit::visit_item(self, item);
        }
    }
    fn visit_impl_item(&mut self, item: &'ast ImplItem) {
        let attrs = impl_item_attrs(item);
        if !test_attrs(attrs) {
            visit::visit_impl_item(self, item);
        }
    }
    fn visit_trait_item(&mut self, item: &'ast TraitItem) {
        let attrs = trait_item_attrs(item);
        if !test_attrs(attrs) {
            visit::visit_trait_item(self, item);
        }
    }
    fn visit_stmt_macro(&mut self, statement: &'ast syn::StmtMacro) {
        if !test_attrs(&statement.attrs) {
            visit::visit_stmt_macro(self, statement);
        }
    }
    fn visit_local(&mut self, local: &'ast syn::Local) {
        if !test_attrs(&local.attrs) {
            visit::visit_local(self, local);
        }
    }
    fn visit_expr_block(&mut self, expression: &'ast syn::ExprBlock) {
        if !test_attrs(&expression.attrs) {
            visit::visit_expr_block(self, expression);
        }
    }
    fn visit_expr_macro(&mut self, expression: &'ast syn::ExprMacro) {
        if !test_attrs(&expression.attrs) {
            visit::visit_expr_macro(self, expression);
        }
    }
    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        self.use_tree(&item.tree, &mut Vec::new());
    }
    fn visit_path(&mut self, path: &'ast syn::Path) {
        self.check_path(
            &path
                .segments
                .iter()
                .map(|s| s.ident.to_string())
                .collect::<Vec<_>>(),
            path.span(),
        );
        visit::visit_path(self, path);
    }
    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        if self.frontend && call.method == "views" {
            self.flag(call.span(), "frontend direct ViewState access is forbidden");
        }
        self.check_terminal_boundary(&[call.method.to_string()], call.method.span());
        visit::visit_expr_method_call(self, call);
    }
    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let syn::Expr::Path(path) = call.func.as_ref() {
            let names = path
                .path
                .segments
                .iter()
                .map(|s| s.ident.to_string())
                .collect::<Vec<_>>();
            if call.args.is_empty()
                && (names == ["Uuid", "nil"]
                    || names == ["uuid", "Uuid", "nil"]
                    || names == ["uuid", "nil"])
            {
                self.flag(call.span(), "production placeholder UUID is forbidden");
            }
        }
        visit::visit_expr_call(self, call);
    }
    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        self.check_path(
            &mac.path
                .segments
                .iter()
                .map(|s| s.ident.to_string())
                .collect::<Vec<_>>(),
            mac.path.span(),
        );
        if mac
            .path
            .segments
            .last()
            .is_some_and(|s| s.ident == "todo" || s.ident == "unimplemented")
        {
            self.flag(
                mac.span(),
                "production incomplete implementation macro is forbidden",
            );
            return;
        }
        if mac
            .path
            .segments
            .last()
            .is_some_and(|s| s.ident == "panic" || s.ident == "bail")
        {
            if let Ok(arguments) =
                syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated
                    .parse2(mac.tokens.clone())
            {
                for argument in arguments.into_iter().take(1) {
                    if let syn::Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Str(literal),
                        ..
                    }) = argument
                    {
                        let value = literal.value().to_ascii_lowercase();
                        if [
                            "placeholder implementation",
                            "to be implemented",
                            "in a full implementation",
                        ]
                        .iter()
                        .any(|marker| value.contains(marker))
                        {
                            self.flag(
                                literal.span(),
                                "production incomplete implementation panic is forbidden",
                            );
                        }
                    }
                }
            }
        }
        // Inspect actual expression/block syntax inside ordinary macros without
        // treating strings/comments or identifier substrings as executable code.
        if let Ok(expr) = syn::parse2::<syn::Expr>(mac.tokens.clone()) {
            self.inspect_expression(&expr);
        } else if let Ok(block) = syn::parse2::<syn::Block>(mac.tokens.clone()) {
            self.inspect_block(&block);
        } else if let Ok(expressions) =
            syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated
                .parse2(mac.tokens.clone())
        {
            for expr in expressions {
                self.inspect_expression(&expr);
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn violations(source: &str) -> Vec<String> {
        scan(
            Path::new("crates/aura-terminal/src/module.rs"),
            &syn::parse_file(source).unwrap(),
        )
    }
    #[test]
    fn genuine_tests_and_ordinary_identifiers_are_accepted() {
        assert!(violations(r#"fn real() { let temporary=path(); let prototype=lookup(); let stub_count=3; let text="Uuid::nil()"; } #[cfg(all(unix,test))] mod fixture {fn helper(){uuid::Uuid::nil();unimplemented!();}} #[test] fn direct(){Uuid::nil();todo!();}"#).is_empty());
    }
    #[test]
    fn production_nil_and_stub_remain_rejected_after_test_items() {
        let errors = violations(
            r#"#[cfg(test)]mod fixtures{fn fake(){todo!();}} fn real(){uuid::Uuid::nil();Uuid::nil();todo!();unimplemented!();}"#,
        );
        assert_eq!(errors.len(), 4, "{errors:?}");
    }
    #[test]
    fn mixed_and_named_test_modules_remain_production() {
        let errors = violations(
            r#"#[cfg(any(test,feature="native"))]fn mixed(){todo!();} #[cfg(not(test))]fn real(){Uuid::nil();} mod tests{fn not_a_fixture(){unimplemented!();}}"#,
        );
        assert_eq!(errors.len(), 3, "{errors:?}");
    }
    #[test]
    fn facade_paths_and_macro_tokens_use_lexical_scope() {
        let errors = violations(
            r#"use aura_app::{runtime_bridge::RuntimeBridge,ui::types::Screen}; fn real(){aura_app::views::read();forward!({Uuid::nil();});} #[cfg(test)]fn fake(){aura_app::runtime_bridge::read();} #[cfg(any(test,unix))]fn mixed(){aura_app::workflows::run();}"#,
        );
        assert_eq!(errors.len(), 4, "{errors:?}");
    }
    #[test]
    fn terminal_mutation_and_view_fences_are_lexical() {
        let errors = violations(
            r#"use aura_chat::Channel; fn real(){bridge.views(); RuntimeBridge::commit(); registry.commit_channel_facts();} #[cfg(test)]mod fixture{use aura_journal::FactRegistry;fn fake(){bridge.views();}} #[cfg(any(test,feature="native"))]fn mixed(){bridge.views();}"#,
        );
        assert_eq!(errors.len(), 5, "{errors:?}");
    }
    #[test]
    fn ordinary_descriptions_and_diagnostic_arguments_are_accepted() {
        assert!(violations(r#"const DESCRIPTION:&str="placeholder implementation";fn real(){tracing::debug!("to be implemented");panic!("{}", "placeholder implementation");}"#).is_empty());
    }
    #[test]
    fn executable_incomplete_panic_and_bail_messages_are_rejected() {
        assert_eq!(
            violations(
                r#"fn real(){panic!("placeholder implementation");bail!("to be implemented");}"#
            )
            .len(),
            2
        );
    }
    #[test]
    fn absolute_and_relative_frontend_paths_have_identical_enforcement() {
        let syntax = syn::parse_file("fn real(){aura_app::runtime_bridge::read();}").unwrap();
        assert_eq!(
            scan(
                Path::new("/checkout/crates/aura-terminal/src/module.rs"),
                &syntax
            )
            .len(),
            scan(Path::new("crates/aura-terminal/src/module.rs"), &syntax).len()
        );
    }
    #[test]
    fn production_src_tests_path_is_not_a_cargo_test_target() {
        let syntax = syn::parse_file("fn real(){Uuid::nil();todo!();}").unwrap();
        assert_eq!(
            scan(
                &Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tests/owner.rs"),
                &syntax
            )
            .len(),
            2
        );
        assert!(scan(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/owner.rs"),
            &syntax
        )
        .is_empty());
    }
    #[test]
    fn positive_local_and_block_cfg_excludes_only_actual_test_scope() {
        let syntax = r#"fn real(){#[cfg(test)]let fixture=Uuid::nil(); #[cfg(all(unix,test))]{unimplemented!();} #[cfg(test)]todo!(); #[cfg(any(test,feature="native"))]let mixed=Uuid::nil(); #[cfg(any(test,unix))]{todo!();} let production=Uuid::nil();}"#;
        assert_eq!(violations(syntax).len(), 3);
    }
}
