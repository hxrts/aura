#![allow(missing_docs)]

fn trybuild_available() -> bool {
    std::env::var_os("CARGO").is_some()
        || std::process::Command::new("cargo")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok()
}

#[test]
fn callback_owner_shapes_compile_fail() {
    if !trybuild_available() {
        eprintln!("skipping trybuild callback-owner guards: cargo is unavailable");
        return;
    }
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/*.rs");
}
