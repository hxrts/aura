//! Lexical Rust test-only scopes for security checks; mixed cfg stays production.
use anyhow::Result;
use std::collections::BTreeSet;
use syn::{spanned::Spanned, visit::Visit};

#[derive(Default)]
struct Scope {
    testing: bool,
    lines: BTreeSet<usize>,
}
impl<'ast> Visit<'ast> for Scope {
    fn visit_item(&mut self, item: &'ast syn::Item) {
        let attrs: &[syn::Attribute] = match item {
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
        };
        let previous = self.testing;
        self.testing |= super::trusted_key_scope::cfg_test(attrs);
        if self.testing {
            self.lines
                .extend(item.span().start().line..=item.span().end().line);
        }
        syn::visit::visit_item(self, item);
        self.testing = previous;
    }
    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        if self.testing || super::trusted_key_scope::cfg_test(&item.attrs) {
            self.lines
                .extend(item.span().start().line..=item.span().end().line);
        }
        syn::visit::visit_impl_item_fn(self, item);
    }
}
pub(super) fn test_lines(source: &str) -> Result<BTreeSet<usize>> {
    let mut scope = Scope::default();
    scope.visit_file(&syn::parse_file(source)?);
    Ok(scope.lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn actual_contact_fixture_is_test_only() {
        let source = include_str!(
            "../../../../crates/aura-agent/src/handlers/invitation/contact_confirmation.rs"
        );
        let lines = test_lines(source).unwrap();
        let fixture = source
            .lines()
            .position(|line| line.contains("signature: Vec::new()"))
            .unwrap()
            + 1;
        assert!(lines.contains(&fixture));
        let production = source.replace("#[cfg(test)]", "");
        let fixture = production
            .lines()
            .position(|line| line.contains("signature: Vec::new()"))
            .unwrap()
            + 1;
        assert!(
            !test_lines(&production).unwrap().contains(&fixture),
            "a test-like module name cannot exempt production signatures"
        );
    }
    #[test]
    fn mixed_cfg_and_later_production_are_not_exempted() {
        let source = "#[cfg(test)]\nmod tests {\n fn fixture() { unsigned(); }\n}\n#[cfg(any(test, feature = \"production\"))]\nfn mixed() { unsigned(); }\nfn production() { unsigned(); }\n#[cfg(all(unix, test))]\nimpl Owner { fn fixture() { unsigned(); } }\n";
        let lines = test_lines(source).unwrap();
        assert!(lines.contains(&3));
        assert!(!lines.contains(&6));
        assert!(!lines.contains(&7));
        assert!(lines.contains(&9));
    }
    #[test]
    fn text_and_test_like_names_do_not_exempt_later_macro_tokens() {
        let source = r#"
#[cfg(test)]
mod tests { fn genuine() { unsigned(); } }
const DECOY: &str = "mod tests #[cfg(test)]";
// mod tests and #[cfg(test)] are text, not ownership.
fn helper_for_tests() { production_call! { unsigned(); } }
#[cfg(any(test, feature = "production"))]
production_macro! { unsigned(); }
#[cfg(all(unix, test))]
fixture_macro! { unsigned(); }
"#;
        let lines = test_lines(source).unwrap();
        for (idx, line) in source.lines().enumerate() {
            if line.contains("fn genuine") || line.contains("fixture_macro!") {
                assert!(lines.contains(&(idx + 1)), "{line}");
            } else if line.contains("production_call!") || line.contains("production_macro!") {
                assert!(!lines.contains(&(idx + 1)), "{line}");
            }
        }
    }
}
