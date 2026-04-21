#[path = "support/trybuild.rs"]
mod trybuild_support;

#[test]
fn service_surface_validation() {
    let _lock = trybuild_support::acquire_trybuild_lock("trybuild-lock-service-surface");
    let t = trybuild::TestCases::new();
    t.pass("tests/boundaries/service_surface_valid.rs");
    t.compile_fail("tests/boundaries/service_surface_missing_select.rs");
    t.compile_fail("tests/boundaries/service_surface_invalid_family.rs");
    t.compile_fail("tests/boundaries/service_surface_authoritative_cache.rs");
    t.compile_fail("tests/boundaries/service_surface_authoritative_social_role.rs");
    t.compile_fail("tests/boundaries/service_surface_runtime_local_social_role.rs");
}
