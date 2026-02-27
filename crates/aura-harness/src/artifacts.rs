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
    pub fn create(base_dir: &Path, run_name: &str) -> Result<Self> {
        let root = base_dir.join("harness");
        let run_dir = root.join(run_name);
        fs::create_dir_all(&run_dir).with_context(|| {
            format!("failed to create artifact directory {}", run_dir.display())
        })?;

        Ok(Self { root, run_dir })
    }

    pub fn write_json<T: Serialize>(&self, file_name: &str, value: &T) -> Result<PathBuf> {
        let out_path = self.run_dir.join(file_name);
        let payload = serde_json::to_vec_pretty(value).context("failed to serialize JSON")?;
        fs::write(&out_path, payload)
            .with_context(|| format!("failed to write artifact {}", out_path.display()))?;
        Ok(out_path)
    }
}
