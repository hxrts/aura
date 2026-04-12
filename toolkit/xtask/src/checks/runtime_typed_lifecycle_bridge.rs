use std::{
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::{bail, Context, Result};
use regex::Regex;

pub fn run() -> Result<()> {
    let repo_root = env::current_dir().context("reading current directory")?;

    let app_bridge_files = collect_app_bridge_files(&repo_root)?;
    let agent_bridge = repo_root.join("crates/aura-agent/src/runtime_bridge/mod.rs");
    let agent_rendezvous = repo_root.join("crates/aura-agent/src/runtime_bridge/rendezvous.rs");
    let mock_bridge = repo_root.join("crates/aura-testkit/src/mock_runtime_bridge.rs");

    check_absent(
        r"async fn process_ceremony_messages\(&self\) -> Result<\(\), IntentError>",
        app_bridge_files
            .iter()
            .chain([&agent_bridge, &mock_bridge].into_iter()),
    )?;
    check_absent(
        r"async fn trigger_discovery\(&self\) -> Result<\(\), IntentError>",
        app_bridge_files
            .iter()
            .chain([&agent_bridge, &agent_rendezvous, &mock_bridge].into_iter()),
    )?;
    check_absent(
        r"async fn accept_invitation\([^)]*\) -> Result<\(\), IntentError>",
        app_bridge_files
            .iter()
            .chain([&agent_bridge, &mock_bridge].into_iter()),
    )?;
    check_absent(
        r"async fn decline_invitation\([^)]*\) -> Result<\(\), IntentError>",
        app_bridge_files
            .iter()
            .chain([&agent_bridge, &mock_bridge].into_iter()),
    )?;
    check_absent(
        r"async fn cancel_invitation\([^)]*\) -> Result<\(\), IntentError>",
        app_bridge_files
            .iter()
            .chain([&agent_bridge, &mock_bridge].into_iter()),
    )?;

    check_present("enum CeremonyProcessingOutcome", app_bridge_files.iter())?;
    check_present("enum DiscoveryTriggerOutcome", app_bridge_files.iter())?;
    check_present("struct InvitationMutationOutcome", app_bridge_files.iter())?;

    println!("runtime-typed-lifecycle-bridge: clean");
    Ok(())
}

fn collect_app_bridge_files(repo_root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = vec![repo_root.join("crates/aura-app/src/runtime_bridge.rs")];
    collect_rs_files(
        &repo_root.join("crates/aura-app/src/runtime_bridge"),
        &mut files,
    )?;
    collect_rs_files(
        &repo_root.join("crates/aura-app/src/runtime_bridge/types"),
        &mut files,
    )?;
    files.sort();
    files.dedup();
    Ok(files)
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            out.push(path);
        }
    }
    Ok(())
}

fn check_absent<'a>(pattern: &str, files: impl Iterator<Item = &'a PathBuf>) -> Result<()> {
    let re = Regex::new(pattern).context("compiling absent pattern")?;
    for path in files {
        let contents =
            fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        if re.is_match(&contents) {
            bail!(
                "runtime-typed-lifecycle-bridge: forbidden unit-return lifecycle signature matched: {pattern}"
            );
        }
    }
    Ok(())
}

fn check_present<'a>(pattern: &str, files: impl Iterator<Item = &'a PathBuf>) -> Result<()> {
    for path in files {
        let contents =
            fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        if contents.contains(pattern) {
            return Ok(());
        }
    }
    bail!("runtime-typed-lifecycle-bridge: required typed lifecycle surface missing: {pattern}")
}
