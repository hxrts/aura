#![feature(rustc_private)]
#![deny(unsafe_code)]

extern crate rustc_errors;
extern crate rustc_hir;
extern crate rustc_lint;
extern crate rustc_session;
extern crate rustc_span;

mod harness_authoritative_fact_boundary;
mod harness_typed_json_boundary;
mod support;

dylint_linting::dylint_library!();

use rustc_lint::LintStore;
use rustc_session::Session;

#[allow(unsafe_code)]
#[expect(clippy::no_mangle_with_rust_abi)]
#[unsafe(no_mangle)]
pub fn register_lints(_sess: &Session, lint_store: &mut LintStore) {
    lint_store.register_lints(&[
        harness_authoritative_fact_boundary::HARNESS_AUTHORITATIVE_FACT_BOUNDARY,
        harness_typed_json_boundary::HARNESS_TYPED_JSON_BOUNDARY,
    ]);
    lint_store.register_late_pass(|_| {
        Box::new(harness_authoritative_fact_boundary::HarnessAuthoritativeFactBoundary::default())
    });
    lint_store.register_late_pass(|_| {
        Box::new(harness_typed_json_boundary::HarnessTypedJsonBoundary::default())
    });
}
