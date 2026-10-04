//! Exercise the real linter CLI against tracked, new and ignored inputs.
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = loop {
            let candidate = std::env::temp_dir().join(format!(
                "aura-lint-inputs-{}",
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match std::fs::create_dir(&candidate) {
                Ok(()) => break candidate,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("create owned fixture: {error}"),
            }
        };
        let fixture = Self(root);
        fixture.git(&["init", "--quiet"]);
        std::fs::write(fixture.0.join("tracked.rs"), "pub fn valid() {}\n")
            .expect("write tracked source");
        fixture.git(&["add", "tracked.rs"]);
        fixture
    }
    fn git(&self, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(&self.0)
            .output()
            .expect("run git fixture setup");
        assert!(output.status.success(), "{output:?}");
    }
    fn lint(&self, paths: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_arch_lints"))
            .arg("capability-boundaries")
            .args(paths)
            .current_dir(&self.0)
            .output()
            .expect("run real architecture linter")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("remove owned fixture");
    }
}

#[test]
fn mixed_tracked_and_missing_input_cannot_report_clean() {
    let fixture = Fixture::new();
    let output = fixture.lint(&["tracked.rs", "missing.rs"]);
    assert!(!output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("path does not exist: missing.rs"));
}

#[test]
fn new_source_is_parsed_alongside_tracked_source() {
    let fixture = Fixture::new();
    std::fs::write(fixture.0.join("new.rs"), "pub fn unfinished(\n")
        .expect("write malformed new source");
    let output = fixture.lint(&["."]);
    assert!(!output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("failed to parse new.rs"));
}

#[test]
fn ignored_artifact_is_excluded_unless_explicitly_requested() {
    let fixture = Fixture::new();
    std::fs::write(fixture.0.join(".gitignore"), "artifact.rs\n").expect("write ignore rule");
    std::fs::write(fixture.0.join("artifact.rs"), "pub fn unfinished(\n")
        .expect("write ignored artifact");
    let directory = fixture.lint(&["."]);
    assert!(directory.status.success(), "{directory:?}");
    let explicit = fixture.lint(&["tracked.rs", "artifact.rs"]);
    assert!(!explicit.status.success(), "{explicit:?}");
    assert!(String::from_utf8_lossy(&explicit.stderr).contains("failed to parse artifact.rs"));
}

#[test]
fn ignored_only_directory_cannot_fall_back_to_scanning_artifacts() {
    let fixture = Fixture::new();
    std::fs::write(fixture.0.join(".gitignore"), "generated/\n").expect("write ignore rule");
    std::fs::create_dir(fixture.0.join("generated")).expect("create artifact directory");
    std::fs::write(
        fixture.0.join("generated/artifact.rs"),
        "pub fn valid() {}\n",
    )
    .expect("write ignored source");
    let output = fixture.lint(&["generated"]);
    assert!(!output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr)
        .contains("no Rust source files found in requested inputs"));
}
