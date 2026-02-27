use std::collections::HashMap;

use anyhow::{anyhow, bail, Result};

use crate::backend::BackendHandle;
use crate::config::RunConfig;
use crate::events::EventStream;

pub struct HarnessCoordinator {
    backends: HashMap<String, BackendHandle>,
    events: EventStream,
}

impl HarnessCoordinator {
    pub fn from_run_config(config: &RunConfig) -> Result<Self> {
        let mut backends = HashMap::new();
        for instance in &config.instances {
            let id = instance.id.clone();
            let backend = BackendHandle::from_config(instance.clone())?;
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
        self.events.push(
            "action",
            "send_keys",
            Some(instance_id.to_string()),
            serde_json::json!({ "bytes": keys.len() }),
        );
        backend.as_trait_mut().send_keys(keys)
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
            if screen.contains(pattern) {
                self.events.push(
                    "observation",
                    "wait_for",
                    Some(instance_id.to_string()),
                    serde_json::json!({ "pattern": pattern, "attempts": attempts + 1 }),
                );
                return Ok(screen);
            }
            attempts = attempts.saturating_add(1);
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

impl Drop for HarnessCoordinator {
    fn drop(&mut self) {
        let _ = self.stop_all();
    }
}
