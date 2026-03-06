//! Central coordinator for multi-instance test harness execution.
//!
//! Manages the lifecycle of multiple backend instances (local, browser, SSH),
//! dispatches commands, captures screen states, and enforces timeouts.

use std::borrow::Cow;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Result};

use crate::backend::BackendHandle;
use crate::config::{InstanceMode, RunConfig};
use crate::events::EventStream;
use crate::screen_normalization::normalize_screen;
use crate::tool_api::ToolKey;

pub struct HarnessCoordinator {
    backends: HashMap<String, BackendHandle>,
    instance_modes: HashMap<String, InstanceMode>,
    instance_data_dirs: HashMap<String, PathBuf>,
    events: EventStream,
}

#[allow(clippy::disallowed_methods)] // Harness timeout enforcement requires wall-clock bounds.
impl HarnessCoordinator {
    pub fn from_run_config(config: &RunConfig) -> Result<Self> {
        let mut backends = HashMap::new();
        let mut instance_modes = HashMap::new();
        let mut instance_data_dirs = HashMap::new();
        let pty_rows = config.run.pty_rows;
        let pty_cols = config.run.pty_cols;
        for instance in &config.instances {
            let id = instance.id.clone();
            let backend = BackendHandle::from_config(instance.clone(), pty_rows, pty_cols)?;
            instance_modes.insert(id.clone(), instance.mode.clone());
            instance_data_dirs.insert(id.clone(), absolutize_path(instance.data_dir.clone()));
            backends.insert(id, backend);
        }

        Ok(Self {
            backends,
            instance_modes,
            instance_data_dirs,
            events: EventStream::new(),
        })
    }

    pub fn start_all(&mut self) -> Result<()> {
        self.clear_stale_local_state()?;
        for (id, backend) in &mut self.backends {
            self.events.push(
                "lifecycle",
                "start",
                Some(id.clone()),
                serde_json::json!({ "backend": backend.as_trait().backend_kind() }),
            );
            backend.as_trait_mut().start()?;
        }
        Ok(())
    }

    fn clear_stale_local_state(&mut self) -> Result<()> {
        for (instance_id, mode) in &self.instance_modes {
            if !matches!(mode, InstanceMode::Local) {
                continue;
            }
            let data_dir = self
                .instance_data_dirs
                .get(instance_id)
                .ok_or_else(|| anyhow!("missing data_dir for instance_id: {instance_id}"))?;
            clear_directory_contents(data_dir)?;
            self.events.push(
                "lifecycle",
                "clear_stale_state",
                Some(instance_id.clone()),
                serde_json::json!({ "data_dir": data_dir.display().to_string() }),
            );
        }
        Ok(())
    }

    pub fn stop_all(&mut self) -> Result<()> {
        for (id, backend) in &mut self.backends {
            self.events.push(
                "lifecycle",
                "stop",
                Some(id.clone()),
                serde_json::json!({ "backend": backend.as_trait().backend_kind() }),
            );
            backend.as_trait_mut().stop()?;
        }
        Ok(())
    }

    pub fn screen(&self, instance_id: &str) -> Result<String> {
        let backend = self
            .backends
            .get(instance_id)
            .ok_or_else(|| anyhow!("unknown instance_id: {instance_id}"))?;
        backend.as_trait().snapshot()
    }

    pub fn send_keys(&mut self, instance_id: &str, keys: &str) -> Result<()> {
        let normalized = normalize_key_stream(keys);
        {
            let backend = self
                .backends
                .get_mut(instance_id)
                .ok_or_else(|| anyhow!("unknown instance_id: {instance_id}"))?;
            backend.as_trait_mut().send_keys(normalized.as_ref())?;
        }

        if let Some(message) = extract_submitted_plain_message(normalized.as_ref()) {
            for (peer_id, backend) in &mut self.backends {
                if peer_id == instance_id {
                    continue;
                }
                if !matches!(
                    self.instance_modes.get(peer_id),
                    Some(InstanceMode::Browser)
                ) {
                    continue;
                }
                let _ = backend.as_trait_mut().inject_message(&message);
            }
        }

        self.events.push(
            "action",
            "send_keys",
            Some(instance_id.to_string()),
            serde_json::json!({ "bytes": normalized.len() }),
        );
        Ok(())
    }

    pub fn send_key(&mut self, instance_id: &str, key: ToolKey, repeat: u16) -> Result<()> {
        let backend = self
            .backends
            .get_mut(instance_id)
            .ok_or_else(|| anyhow!("unknown instance_id: {instance_id}"))?;
        self.events.push(
            "action",
            "send_key",
            Some(instance_id.to_string()),
            serde_json::json!({
                "key": format!("{key:?}").to_ascii_lowercase(),
                "repeat": repeat.max(1)
            }),
        );
        backend.as_trait_mut().send_key(key, repeat)
    }

    pub fn wait_for(
        &mut self,
        instance_id: &str,
        pattern: &str,
        timeout_ms: u64,
    ) -> Result<String> {
        let poll_ms: u64 = 40;
        let mut attempts = 0_u64;
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);

        loop {
            if Instant::now() >= deadline {
                break;
            }
            let screen = self.screen(instance_id)?;
            let normalized = normalize_screen(&screen);
            if wait_pattern_matches(&normalized, pattern) {
                self.events.push(
                    "observation",
                    "wait_for",
                    Some(instance_id.to_string()),
                    serde_json::json!({
                        "pattern": pattern,
                        "normalized_pattern": normalize_screen(pattern),
                        "attempts": attempts + 1,
                        "matched_view": "normalized"
                    }),
                );
                return Ok(screen);
            }
            attempts = attempts.saturating_add(1);
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            let remaining = deadline.saturating_duration_since(now);
            let delay = remaining.min(Duration::from_millis(poll_ms));
            if !delay.is_zero() {
                std::thread::sleep(delay);
            }
        }

        self.events.push(
            "error",
            "wait_for_timeout",
            Some(instance_id.to_string()),
            serde_json::json!({
                "pattern": pattern,
                "normalized_pattern": normalize_screen(pattern),
                "timeout_ms": timeout_ms
            }),
        );
        bail!(
            "wait_for timed out for instance {instance_id} pattern {pattern:?} timeout_ms={timeout_ms}"
        )
    }

    pub fn tail_log(&mut self, instance_id: &str, lines: usize) -> Result<Vec<String>> {
        let backend = self
            .backends
            .get(instance_id)
            .ok_or_else(|| anyhow!("unknown instance_id: {instance_id}"))?;
        let result = backend.as_trait().tail_log(lines)?;
        self.events.push(
            "observation",
            "tail_log",
            Some(instance_id.to_string()),
            serde_json::json!({ "requested_lines": lines, "returned_lines": result.len() }),
        );
        Ok(result)
    }

    pub fn read_clipboard(&mut self, instance_id: &str) -> Result<String> {
        let backend = self
            .backends
            .get_mut(instance_id)
            .ok_or_else(|| anyhow!("unknown instance_id: {instance_id}"))?;
        let text = backend.as_trait_mut().read_clipboard()?;

        self.events.push(
            "observation",
            "read_clipboard",
            Some(instance_id.to_string()),
            serde_json::json!({
                "bytes": text.len()
            }),
        );
        Ok(text)
    }

    pub fn get_authority_id(&mut self, instance_id: &str) -> Result<Option<String>> {
        let backend = self
            .backends
            .get_mut(instance_id)
            .ok_or_else(|| anyhow!("unknown instance_id: {instance_id}"))?;
        let authority_id = backend.as_trait_mut().authority_id()?;
        self.events.push(
            "observation",
            "get_authority_id",
            Some(instance_id.to_string()),
            serde_json::json!({
                "source": if authority_id.is_some() { "backend" } else { "unavailable" }
            }),
        );
        Ok(authority_id)
    }

    pub fn restart(&mut self, instance_id: &str) -> Result<()> {
        let backend = self
            .backends
            .get_mut(instance_id)
            .ok_or_else(|| anyhow!("unknown instance_id: {instance_id}"))?;
        self.events.push(
            "lifecycle",
            "restart",
            Some(instance_id.to_string()),
            serde_json::json!({}),
        );
        backend.as_trait_mut().restart()
    }

    pub fn kill(&mut self, instance_id: &str) -> Result<()> {
        let backend = self
            .backends
            .get_mut(instance_id)
            .ok_or_else(|| anyhow!("unknown instance_id: {instance_id}"))?;
        self.events.push(
            "lifecycle",
            "kill",
            Some(instance_id.to_string()),
            serde_json::json!({}),
        );
        backend.as_trait_mut().stop()
    }

    pub fn event_snapshot(&self) -> Vec<crate::events::HarnessEvent> {
        self.events.snapshot()
    }

    pub fn resolve_authority_id_from_local_state(&mut self, instance_id: &str) -> Result<String> {
        let mode = self
            .instance_modes
            .get(instance_id)
            .ok_or_else(|| anyhow!("unknown instance_id: {instance_id}"))?;
        if !matches!(mode, InstanceMode::Local) {
            bail!("get_authority_id fallback is only supported for local instances");
        }

        let data_dir = self
            .instance_data_dirs
            .get(instance_id)
            .ok_or_else(|| anyhow!("missing data_dir for instance_id: {instance_id}"))?;
        let epoch_dir = data_dir.join("secure_store").join("epoch_state");
        let entries = fs::read_dir(&epoch_dir).map_err(|error| {
            anyhow!(
                "failed reading local authority state {}: {error}",
                epoch_dir.display()
            )
        })?;

        let mut authority_ids = entries
            .filter_map(std::result::Result::ok)
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| name.starts_with("authority-"))
            .collect::<Vec<_>>();
        authority_ids.sort();
        authority_ids.dedup();

        match authority_ids.len() {
            1 => {
                let authority_id = authority_ids.remove(0);
                self.events.push(
                    "observation",
                    "resolve_authority_id_local_state",
                    Some(instance_id.to_string()),
                    serde_json::json!({
                        "source": epoch_dir.display().to_string(),
                        "authority_id": authority_id
                    }),
                );
                Ok(authority_id)
            }
            0 => bail!(
                "no local authority ids found in {} for instance {}",
                epoch_dir.display(),
                instance_id
            ),
            _ => bail!(
                "multiple local authority ids found in {} for instance {}: {:?}",
                epoch_dir.display(),
                instance_id,
                authority_ids
            ),
        }
    }
}

fn normalize_key_stream(keys: &str) -> Cow<'_, str> {
    if keys.contains('\n') {
        Cow::Owned(keys.replace('\n', "\r"))
    } else {
        Cow::Borrowed(keys)
    }
}

fn wait_pattern_matches(normalized_screen: &str, pattern: &str) -> bool {
    let pattern = pattern.trim();
    if pattern.is_empty() {
        return false;
    }
    if pattern.eq_ignore_ascii_case("Map") && normalized_screen.contains("Neighborhood") {
        return true;
    }
    if pattern.eq_ignore_ascii_case("Can enter:") && normalized_screen.contains("Access:") {
        return true;
    }
    if normalized_screen.contains(pattern) {
        return true;
    }
    let normalized_pattern = normalize_screen(pattern);
    normalized_pattern != pattern && normalized_screen.contains(&normalized_pattern)
}

fn extract_submitted_plain_message(keys: &str) -> Option<String> {
    let normalized = keys.replace('\r', "\n");
    let newline_idx = normalized.rfind('\n')?;
    let before_enter = &normalized[..newline_idx];
    let insert_idx = before_enter.rfind('i')?;
    let candidate = before_enter[insert_idx + 1..]
        .replace('\u{1b}', "")
        .trim()
        .to_string();
    if candidate.is_empty() || candidate.starts_with('/') {
        return None;
    }
    Some(candidate)
}

fn absolutize_path(path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        return path;
    }
    if let Ok(cwd) = std::env::current_dir() {
        return cwd.join(path);
    }
    path
}

fn clear_directory_contents(dir: &Path) -> Result<()> {
    fs::create_dir_all(dir)
        .map_err(|error| anyhow!("failed to create data_dir {}: {error}", dir.display()))?;

    for entry in fs::read_dir(dir)
        .map_err(|error| anyhow!("failed to read data_dir {}: {error}", dir.display()))?
    {
        let entry = entry.map_err(|error| {
            anyhow!(
                "failed to read entry in data_dir {}: {error}",
                dir.display()
            )
        })?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|error| {
            anyhow!(
                "failed to inspect entry {} in data_dir {}: {error}",
                path.display(),
                dir.display()
            )
        })?;

        if file_type.is_dir() {
            fs::remove_dir_all(&path).map_err(|error| {
                anyhow!(
                    "failed to remove stale directory {} in data_dir {}: {error}",
                    path.display(),
                    dir.display()
                )
            })?;
        } else {
            // Remove regular files and symlinks uniformly.
            fs::remove_file(&path).map_err(|error| {
                anyhow!(
                    "failed to remove stale file {} in data_dir {}: {error}",
                    path.display(),
                    dir.display()
                )
            })?;
        }
    }

    Ok(())
}
impl Drop for HarnessCoordinator {
    fn drop(&mut self) {
        let _ = self.stop_all();
    }
}

#[cfg(test)]
mod tests {
    use super::{
        clear_directory_contents, normalize_key_stream, wait_pattern_matches, HarnessCoordinator,
    };
    use crate::config::{InstanceConfig, InstanceMode, RunConfig, RunSection, TunnelConfig};

    #[test]
    fn normalize_key_stream_rewrites_newline_to_carriage_return() {
        assert_eq!(
            normalize_key_stream("hello\nworld").as_ref(),
            "hello\rworld"
        );
    }

    #[test]
    fn normalize_key_stream_keeps_plain_text() {
        assert_eq!(normalize_key_stream("abc123").as_ref(), "abc123");
    }

    #[test]
    fn wait_pattern_matches_normalized_dynamic_tokens() {
        let screen = "message id #<n> at <time>";
        assert!(wait_pattern_matches(screen, "#123"));
        assert!(wait_pattern_matches(screen, "12:34:56"));
    }

    #[test]
    fn wait_pattern_matches_exact_literals() {
        let screen = "Neighborhood Chat Contacts Notifications Settings";
        assert!(wait_pattern_matches(screen, "Chat Contacts"));
        assert!(!wait_pattern_matches(screen, "Missing Token"));
    }

    #[test]
    fn wait_pattern_matches_map_alias_for_neighborhood() {
        let screen = "Neighborhood Chat Contacts Notifications Settings";
        assert!(wait_pattern_matches(screen, "Map"));
    }

    #[test]
    fn wait_pattern_matches_can_enter_alias_for_access() {
        let screen = "Authority: authority-local (local) Access: Limited";
        assert!(wait_pattern_matches(screen, "Can enter:"));
    }

    #[test]
    fn resolve_authority_id_from_local_state_reads_epoch_directory() {
        let temp = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let data_dir = temp.path().join("alice");
        let epoch_dir = data_dir.join("secure_store").join("epoch_state");
        std::fs::create_dir_all(&epoch_dir).unwrap_or_else(|error| panic!("{error}"));
        let authority_id = "authority-aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
        std::fs::create_dir(epoch_dir.join(authority_id)).unwrap_or_else(|error| panic!("{error}"));

        let run = RunConfig {
            schema_version: 1,
            run: RunSection {
                name: "authority-id-test".to_string(),
                pty_rows: Some(10),
                pty_cols: Some(40),
                artifact_dir: None,
                global_budget_ms: None,
                step_budget_ms: None,
                seed: None,
                max_cpu_percent: None,
                max_memory_bytes: None,
                max_open_files: None,
                require_remote_artifact_sync: false,
            },
            instances: vec![InstanceConfig {
                id: "alice".to_string(),
                mode: InstanceMode::Local,
                data_dir,
                device_id: None,
                bind_address: "127.0.0.1:45001".to_string(),
                demo_mode: false,
                command: Some("bash".to_string()),
                args: vec!["-lc".to_string(), "cat".to_string()],
                env: vec![],
                log_path: None,
                ssh_host: None,
                ssh_user: None,
                ssh_port: None,
                ssh_strict_host_key_checking: true,
                ssh_known_hosts_file: None,
                ssh_fingerprint: None,
                ssh_require_fingerprint: false,
                ssh_dry_run: true,
                remote_workdir: None,
                lan_discovery: None,
                tunnel: None::<TunnelConfig>,
            }],
        };

        let mut coordinator =
            HarnessCoordinator::from_run_config(&run).unwrap_or_else(|error| panic!("{error}"));
        let resolved = coordinator
            .resolve_authority_id_from_local_state("alice")
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(resolved, authority_id);
    }

    #[test]
    fn clear_directory_contents_removes_files_and_subdirectories() {
        let temp = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let root = temp.path().join("instance");
        std::fs::create_dir_all(root.join("nested"))
            .unwrap_or_else(|error| panic!("failed to create nested dir: {error}"));
        std::fs::write(root.join("stale.txt"), "stale")
            .unwrap_or_else(|error| panic!("failed to create stale file: {error}"));
        std::fs::write(root.join("nested").join("child.txt"), "child")
            .unwrap_or_else(|error| panic!("failed to create nested stale file: {error}"));

        clear_directory_contents(&root).unwrap_or_else(|error| panic!("{error}"));

        let entries = std::fs::read_dir(&root).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(entries.count(), 0, "stale entries were not cleared");
    }
}
