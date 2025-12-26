use std::fs;
use std::path::{Path, PathBuf};

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_rs_files(&path, out);
            } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
                out.push(path);
            }
        }
    }
}

#[test]
fn test_no_impure_api_usage_outside_handlers() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .expect("repo root");

    let crates_dir = repo_root.join("crates");

    let allowlist = [
        "crates/aura-effects",
        "crates/aura-testkit",
        "crates/aura-simulator",
        "crates/aura-consensus",
        "crates/aura-agent",
        "crates/aura-core/src/crypto",
    ];

    let patterns = [
        ("SystemTime::now", "Use PhysicalTimeEffects instead of SystemTime::now"),
        ("chrono::", "Use unified TimeEffects instead of chrono"),
        ("thread_rng", "Use RandomEffects instead of thread_rng"),
        ("rand::", "Use RandomEffects instead of rand::*"),
    ];

    let mut files = Vec::new();
    collect_rs_files(&crates_dir, &mut files);

    let mut violations = Vec::new();
    for file in files {
        let rel = file.strip_prefix(repo_root).unwrap_or(&file);
        let rel_str = rel.to_string_lossy();

        if allowlist.iter().any(|prefix| rel_str.starts_with(prefix)) {
            continue;
        }

        let Ok(contents) = fs::read_to_string(&file) else {
            continue;
        };

        for (pattern, guidance) in patterns {
            if contents.contains(pattern) {
                violations.push(format!(
                    "{}: found '{}' ({} )",
                    rel_str, pattern, guidance
                ));
            }
        }
    }

    if !violations.is_empty() {
        panic!("Impure API usage detected:\n{}", violations.join("\n"));
    }
}
