use std::borrow::Cow;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Result};

use crate::backend::BackendHandle;
use crate::config::{InstanceConfig, InstanceMode, RunConfig};
use crate::events::EventStream;
use crate::screen_normalization::normalize_screen;
use crate::tool_api::ToolKey;

pub struct HarnessCoordinator {
    backends: HashMap<String, BackendHandle>,
    clipboard_files: HashMap<String, PathBuf>,
    instance_modes: HashMap<String, InstanceMode>,
    events: EventStream,
}

#[allow(clippy::disallowed_methods)] // Harness timeout enforcement requires wall-clock bounds.
impl HarnessCoordinator {
    pub fn from_run_config(config: &RunConfig) -> Result<Self> {
        let mut backends = HashMap::new();
        let mut clipboard_files = HashMap::new();
        let mut instance_modes = HashMap::new();
        let pty_rows = config.run.pty_rows;
        let pty_cols = config.run.pty_cols;
        for instance in &config.instances {
            let id = instance.id.clone();
            let backend = BackendHandle::from_config(instance.clone(), pty_rows, pty_cols)?;
            clipboard_files.insert(id.clone(), clipboard_file_for_instance(instance));
            instance_modes.insert(id.clone(), instance.mode.clone());
            backends.insert(id, backend);
        }

        Ok(Self {
            backends,
            clipboard_files,
            instance_modes,
            events: EventStream::new(),
        })
    }

    pub fn start_all(&mut self) -> Result<()> {
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
        let backend = self
            .backends
            .get_mut(instance_id)
            .ok_or_else(|| anyhow!("unknown instance_id: {instance_id}"))?;
        let normalized = normalize_key_stream(keys);
        self.events.push(
            "action",
            "send_keys",
            Some(instance_id.to_string()),
            serde_json::json!({ "bytes": normalized.len() }),
        );
        backend.as_trait_mut().send_keys(normalized.as_ref())
    }

    pub fn send_key(&mut self, instance_id: &str, key: ToolKey, repeat: u16) -> Result<()> {
        let sequence = key_sequence(key);
        let repeat = repeat.max(1);
        for _ in 0..repeat {
            self.send_keys(instance_id, sequence)?;
        }
        Ok(())
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
        let mode = self
            .instance_modes
            .get(instance_id)
            .ok_or_else(|| anyhow!("unknown instance_id: {instance_id}"))?;
        if !matches!(mode, InstanceMode::Local) {
            bail!("read_clipboard is only supported for local instances");
        }

        let path = self
            .clipboard_files
            .get(instance_id)
            .ok_or_else(|| anyhow!("missing clipboard path for instance_id: {instance_id}"))?;
        let mut text = fs::read_to_string(path).map_err(|error| {
            anyhow!("failed reading clipboard file {}: {error}", path.display())
        })?;
        while matches!(text.chars().last(), Some('\n' | '\r')) {
            text.pop();
        }
        if text.is_empty() {
            bail!("clipboard for instance {instance_id} is empty");
        }

        self.events.push(
            "observation",
            "read_clipboard",
            Some(instance_id.to_string()),
            serde_json::json!({
                "path": path.display().to_string(),
                "bytes": text.len()
            }),
        );
        Ok(text)
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
}

fn key_sequence(key: ToolKey) -> &'static str {
    match key {
        ToolKey::Enter => "\r",
        ToolKey::Esc => "\x1b",
        ToolKey::Tab => "\t",
        ToolKey::BackTab => "\x1b[Z",
        ToolKey::Up => "\x1b[A",
        ToolKey::Down => "\x1b[B",
        ToolKey::Right => "\x1b[C",
        ToolKey::Left => "\x1b[D",
        ToolKey::Home => "\x1b[H",
        ToolKey::End => "\x1b[F",
        ToolKey::PageUp => "\x1b[5~",
        ToolKey::PageDown => "\x1b[6~",
        ToolKey::Backspace => "\x7f",
        ToolKey::Delete => "\x1b[3~",
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
    if normalized_screen.contains(pattern) {
        return true;
    }
    let normalized_pattern = normalize_screen(pattern);
    normalized_pattern != pattern && normalized_screen.contains(&normalized_pattern)
}

fn env_value(key: &str, env_entries: &[String]) -> Option<String> {
    env_entries.iter().find_map(|item| {
        let (entry_key, entry_value) = item.split_once('=')?;
        if entry_key.trim() == key {
            Some(entry_value.trim().to_string())
        } else {
            None
        }
    })
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

fn clipboard_file_for_instance(instance: &InstanceConfig) -> PathBuf {
    if let Some(path) = env_value("AURA_CLIPBOARD_FILE", &instance.env) {
        return absolutize_path(PathBuf::from(path));
    }
    absolutize_path(instance.data_dir.join(".harness-clipboard.txt"))
}

impl Drop for HarnessCoordinator {
    fn drop(&mut self) {
        let _ = self.stop_all();
    }
}

#[cfg(test)]
mod tests {
    use super::{clipboard_file_for_instance, normalize_key_stream, wait_pattern_matches};
    use crate::config::InstanceConfig;
    use crate::config::InstanceMode;
    use crate::config::TunnelConfig;
    use std::path::PathBuf;

    fn test_instance(env: Vec<String>) -> InstanceConfig {
        InstanceConfig {
            id: "alice".to_string(),
            mode: InstanceMode::Local,
            data_dir: PathBuf::from(".tmp/test/alice"),
            device_id: None,
            bind_address: "127.0.0.1:45001".to_string(),
            demo_mode: false,
            command: None,
            args: vec![],
            env,
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
        }
    }

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
    fn clipboard_file_uses_instance_default_path() {
        let instance = test_instance(vec![]);
        let expected = std::env::current_dir()
            .unwrap_or_else(|error| panic!("current_dir failed: {error}"))
            .join(".tmp/test/alice/.harness-clipboard.txt");
        assert_eq!(clipboard_file_for_instance(&instance), expected);
    }

    #[test]
    fn clipboard_file_uses_env_override() {
        let instance = test_instance(vec![
            "AURA_CLIPBOARD_MODE=file_only".to_string(),
            "AURA_CLIPBOARD_FILE=tmp/custom-clip.txt".to_string(),
        ]);
        let expected = std::env::current_dir()
            .unwrap_or_else(|error| panic!("current_dir failed: {error}"))
            .join("tmp/custom-clip.txt");
        assert_eq!(clipboard_file_for_instance(&instance), expected);
    }
}
