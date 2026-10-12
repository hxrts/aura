//! Holepunch NAT traversal tests (tiers 1-3).

mod holepunch {
    mod holepunch_e2e_runtime_patchbay;
    mod holepunch_tier2_patchbay;
    mod holepunch_tier3_stress;
}

/// Patchbay refuses to build a lab as real root outside a user namespace, and
/// the namespace must be entered before any thread exists. Upstream prescribes
/// a `ctor` that runs from the ELF init array; this is its libc-only variant.
#[cfg(all(target_os = "linux", feature = "patchbay-backend"))]
#[ctor::ctor(unsafe)]
fn enter_patchbay_user_namespace() {
    // SAFETY: called from the single-threaded ELF init context, before the Rust
    // standard library is initialized, as `init_userns_for_ctor` requires.
    unsafe { patchbay::init_userns_for_ctor() };
}
