use std::borrow::Cow;
use std::collections::HashMap;

use anyhow::{anyhow, bail, Result};

use crate::backend::BackendHandle;
use crate::config::RunConfig;
use crate::events::EventStream;
use crate::screen_normalization::normalize_screen;
use crate::tool_api::ToolKey;

pub struct HarnessCoordinator {
    backends: HashMap<String, BackendHandle>,
    events: EventStream,
}

impl HarnessCoordinator {
    pub fn from_run_config(config: &RunConfig) -> Result<Self> {
        let mut backends = HashMap::new();
        let pty_rows = config.run.pty_rows;
        let pty_cols = config.run.pty_cols;
        for instance in &config.instances {
            let id = instance.id.clone();
            let backend = BackendHandle::from_config(instance.clone(), pty_rows, pty_cols)?;
            backends.insert(id, backend);
        }

        Ok(Self {
            backends,
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
        let poll_ms: u64 = 20;
        let mut attempts = 0_u64;
        let max_attempts = (timeout_ms / poll_ms).saturating_add(1);

        while attempts < max_attempts {
            let screen = self.screen(instance_id)?;
            let normalized = normalize_screen(&screen);
            if normalized.contains(pattern) {
                self.events.push(
                    "observation",
                    "wait_for",
                    Some(instance_id.to_string()),
                    serde_json::json!({
                        "pattern": pattern,
                        "attempts": attempts + 1,
                        "matched_view": "normalized"
                    }),
                );
                return Ok(screen);
            }
            attempts = attempts.saturating_add(1);
            if attempts >= max_attempts {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(poll_ms));
        }

        self.events.push(
            "error",
            "wait_for_timeout",
            Some(instance_id.to_string()),
            serde_json::json!({ "pattern": pattern, "timeout_ms": timeout_ms }),
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

impl Drop for HarnessCoordinator {
    fn drop(&mut self) {
        let _ = self.stop_all();
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_key_stream;

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
}
