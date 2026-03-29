//! Test artifact bundle management for harness runs.
//!
//! Provides directory structure creation and JSON serialization for test artifacts
//! including logs, screenshots, traces, and scenario results.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Serialize;

#[derive(Debug, Clone)]
pub struct ArtifactBundle {
    pub root: PathBuf,
    pub run_dir: PathBuf,
}

impl ArtifactBundle {
    pub fn from_run_dir(run_dir: &Path) -> Result<Self> {
        let run_dir = run_dir.to_path_buf();
        fs::create_dir_all(&run_dir).with_context(|| {
            format!("failed to create artifact directory {}", run_dir.display())
        })?;
        let root = run_dir
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| run_dir.clone());

        Ok(Self { root, run_dir })
    }

    pub fn create(base_dir: &Path, run_name: &str) -> Result<Self> {
        let root = base_dir.join("harness");
        let run_dir = run_token()
            .map(|token| root.join(run_name).join(token))
            .unwrap_or_else(|| root.join(run_name));
        Self::from_run_dir(&run_dir).map(|bundle| Self { root, ..bundle })
    }

    pub fn write_json<T: Serialize>(&self, file_name: &str, value: &T) -> Result<PathBuf> {
        let out_path = self.run_dir.join(file_name);
        let payload = serde_json::to_vec_pretty(value).context("failed to serialize JSON")?;
        fs::write(&out_path, payload)
            .with_context(|| format!("failed to write artifact {}", out_path.display()))?;
        Ok(out_path)
    }
}

fn run_token() -> Option<String> {
    std::env::var("AURA_HARNESS_RUN_TOKEN")
        .ok()
        .map(|value| {
            value
                .chars()
                .filter_map(|ch| {
                    if ch.is_ascii_alphanumeric() {
                        Some(ch.to_ascii_lowercase())
                    } else if matches!(ch, '-' | '_' | ' ') {
                        Some('-')
                    } else {
                        None
                    }
                })
                .collect::<String>()
        })
        .filter(|value| !value.trim_matches('-').is_empty())
        .map(|value| value.trim_matches('-').to_string())
}

#[cfg(test)]
mod tests {
    use super::ArtifactBundle;

    #[test]
    fn explicit_run_dir_stays_authoritative() {
        let temp_dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let run_dir = temp_dir.path().join("isolated-run");
        let bundle = ArtifactBundle::from_run_dir(&run_dir)
            .unwrap_or_else(|error| panic!("artifact bundle creation should succeed: {error}"));

        assert_eq!(bundle.run_dir, run_dir);
        assert!(bundle.run_dir.exists());
        assert_eq!(
            bundle.root,
            temp_dir.path().to_path_buf(),
            "authoritative run_dir creation should not append an extra harness/run-name layer"
        );
    }
}
