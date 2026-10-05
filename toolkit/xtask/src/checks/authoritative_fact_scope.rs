//! Production frontend fact paths with lexical positive-test exclusions.
use anyhow::Result;
use proc_macro2::{TokenStream, TokenTree};
use std::collections::BTreeSet;
use syn::{spanned::Spanned, visit::Visit};

const FACTS: &[&str] = &[
    "OperationStatus",
    "PendingHomeInvitationReady",
    "ContactLinkReady",
    "ChannelMembershipReady",
    "RecipientPeersResolved",
    "PeerChannelReady",
    "MessageDeliveryReady",
];

#[derive(Default)]
struct FactPaths {
    lines: BTreeSet<usize>,
}
impl<'ast> Visit<'ast> for FactPaths {
    fn visit_path(&mut self, path: &'ast syn::Path) {
        let mut segments = path.segments.iter().rev();
        if let (Some(fact), Some(owner)) = (segments.next(), segments.next()) {
            if owner.ident == "AuthoritativeSemanticFact"
                && FACTS.contains(&fact.ident.to_string().as_str())
            {
                self.lines.insert(owner.span().start().line);
            }
        }
        syn::visit::visit_path(self, path);
    }
    fn visit_macro(&mut self, item: &'ast syn::Macro) {
        self.macro_tokens(item.tokens.clone());
        syn::visit::visit_macro(self, item);
    }
}
impl FactPaths {
    // Rust macros retain token trees rather than parsed expressions. Match only
    // actual path tokens; quoted strings and comments never become identifiers.
    fn macro_tokens(&mut self, tokens: TokenStream) {
        let tokens: Vec<_> = tokens.into_iter().collect();
        for window in tokens.windows(4) {
            if let [TokenTree::Ident(owner), TokenTree::Punct(a), TokenTree::Punct(b), TokenTree::Ident(fact)] =
                window
            {
                if owner == "AuthoritativeSemanticFact"
                    && a.as_char() == ':'
                    && b.as_char() == ':'
                    && FACTS.contains(&fact.to_string().as_str())
                {
                    self.lines.insert(owner.span().start().line);
                }
            }
        }
        for token in tokens {
            if let TokenTree::Group(group) = token {
                self.macro_tokens(group.stream());
            }
        }
    }
}

pub(super) fn production_fact_lines(source: &str) -> Result<BTreeSet<usize>> {
    let test_lines = super::security_test_scope::test_lines(source)?;
    let mut paths = FactPaths::default();
    paths.visit_file(&syn::parse_file(source)?);
    Ok(paths.lines.difference(&test_lines).copied().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lexical_test_scope_does_not_hide_adjacent_production_or_comments() {
        let source = r#"
// AuthoritativeSemanticFact::ChannelMembershipReady
#[cfg(test)] mod tests {
    fn fixture() { let _ = AuthoritativeSemanticFact::ChannelMembershipReady; }
}
fn production() { let _ = AuthoritativeSemanticFact::ContactLinkReady; }
fn diagnostic() { log!("AuthoritativeSemanticFact::MessageDeliveryReady"); }
fn production_macro() { publish!(AuthoritativeSemanticFact::PeerChannelReady); }
"#;
        assert_eq!(
            production_fact_lines(source).unwrap(),
            BTreeSet::from([6, 8])
        );
    }
    #[test]
    fn positive_test_predicates_exclude_only_genuine_test_scopes() {
        let source = r#"
#[cfg(all(test, unix))] fn test_only() { let _ = AuthoritativeSemanticFact::OperationStatus; }
#[cfg(any(test, unix))] fn mixed() { let _ = AuthoritativeSemanticFact::OperationStatus; }
#[cfg(any(test, all(test, feature = "fixture")))] fn test_alternatives() { let _ = AuthoritativeSemanticFact::RecipientPeersResolved; }
#[cfg(not(test))] fn native() { let _ = AuthoritativeSemanticFact::MessageDeliveryReady; }
"#;
        assert_eq!(
            production_fact_lines(source).unwrap(),
            BTreeSet::from([3, 5])
        );
    }
    #[test]
    fn actual_native_account_test_producers_remain_test_only() {
        let source = include_str!("../../../../crates/aura-terminal/src/handlers/tui/account.rs");
        let test_lines = super::super::security_test_scope::test_lines(source).unwrap();
        let mut paths = FactPaths::default();
        paths.visit_file(&syn::parse_file(source).unwrap());
        assert!(
            !paths.lines.is_empty(),
            "fixture must contain actual fact producers"
        );
        assert!(paths.lines.iter().all(|line| test_lines.contains(line)));
        assert!(production_fact_lines(source).unwrap().is_empty());
    }
}
