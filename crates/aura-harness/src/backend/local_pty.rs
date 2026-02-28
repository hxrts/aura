use std::fs;
use std::io::{Read, Write};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use portable_pty::{native_pty_system, Child, CommandBuilder, PtySize};
use tokio::sync::Mutex;

use crate::backend::InstanceBackend;
use crate::config::InstanceConfig;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum BackendState {
    Stopped,
    Running,
}

struct RunningSession {
    child: Box<dyn Child + Send>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    parser: Arc<Mutex<vt100::Parser>>,
    reader_thread: Option<thread::JoinHandle<()>>,
}

pub struct LocalPtyBackend {
    config: InstanceConfig,
    state: BackendState,
    session: Option<RunningSession>,
    pty_rows: u16,
    pty_cols: u16,
}

impl LocalPtyBackend {
    pub fn new(config: InstanceConfig, pty_rows: Option<u16>, pty_cols: Option<u16>) -> Self {
        Self {
            config,
            state: BackendState::Stopped,
            session: None,
            pty_rows: pty_rows.unwrap_or(40),
            pty_cols: pty_cols.unwrap_or(120),
        }
    }

    fn default_command() -> (String, Vec<String>) {
        (
            "bash".to_string(),
            vec!["-lc".to_string(), "cat".to_string()],
        )
    }

    fn command_spec(&self) -> (String, Vec<String>) {
        match &self.config.command {
            Some(command) => (command.clone(), self.config.args.clone()),
            None => Self::default_command(),
        }
    }

    fn parser_size(&self) -> (u16, u16) {
        (self.pty_rows, self.pty_cols)
    }
}

impl InstanceBackend for LocalPtyBackend {
    fn id(&self) -> &str {
        &self.config.id
    }

    fn backend_kind(&self) -> &'static str {
        "local_pty"
    }

    fn start(&mut self) -> Result<()> {
        if self.state == BackendState::Running {
            return Ok(());
        }

        let (rows, cols) = self.parser_size();
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .with_context(|| format!("failed to allocate PTY for {}", self.config.id))?;

        let (program, args) = self.command_spec();
        let mut command = CommandBuilder::new(program);
        for arg in args {
            command.arg(arg);
        }
        command.cwd(&self.config.data_dir);
        command.env("TERM", "xterm-256color");
        command.env("LANG", "C.UTF-8");
        for item in &self.config.env {
            if let Some((key, value)) = item.split_once('=') {
                command.env(key.trim(), value.trim());
            }
        }

        fs::create_dir_all(&self.config.data_dir).with_context(|| {
            format!(
                "failed to create instance data_dir {}",
                self.config.data_dir.display()
            )
        })?;

        let child = pair
            .slave
            .spawn_command(command)
            .with_context(|| format!("failed to spawn process for {}", self.config.id))?;
        drop(pair.slave);

        let mut reader = pair
            .master
            .try_clone_reader()
            .context("failed to clone PTY reader")?;
        let writer = pair
            .master
            .take_writer()
            .context("failed to acquire PTY writer")?;

        let parser = Arc::new(Mutex::new(vt100::Parser::new(rows, cols, 0)));
        let parser_for_thread = Arc::clone(&parser);
        let reader_thread = thread::spawn(move || {
            let mut buffer = [0u8; 4096];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(read) => parser_for_thread.blocking_lock().process(&buffer[..read]),
                    Err(_) => break,
                }
            }
        });

        self.session = Some(RunningSession {
            child,
            writer: Arc::new(Mutex::new(writer)),
            parser,
            reader_thread: Some(reader_thread),
        });
        self.state = BackendState::Running;
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        if self.state == BackendState::Stopped {
            return Ok(());
        }

        if let Some(mut session) = self.session.take() {
            let _ = session.child.kill();
            let _ = session.child.wait();
            drop(session.writer);
            if let Some(handle) = session.reader_thread.take() {
                let _ = handle.join();
            }
        }

        self.state = BackendState::Stopped;
        Ok(())
    }

    fn snapshot(&self) -> Result<String> {
        let session = self
            .session
            .as_ref()
            .with_context(|| format!("instance {} is not running", self.config.id))?;
        let parser = session.parser.blocking_lock();
        let screen = parser.screen();
        let (_, cols) = screen.size();
        let rows: Vec<String> = screen.rows(0, cols).collect();
        Ok(rows.join("\n"))
    }

    fn send_keys(&mut self, keys: &str) -> Result<()> {
        let session = self
            .session
            .as_ref()
            .with_context(|| format!("instance {} is not running", self.config.id))?;
        if !keys.as_bytes().contains(&0x1b) {
            let mut writer = session.writer.blocking_lock();
            writer
                .write_all(keys.as_bytes())
                .with_context(|| format!("failed writing keys for instance {}", self.config.id))?;
            writer.flush().context("failed flushing PTY writer")?;
            return Ok(());
        }

        let bytes = keys.as_bytes();
        let mut index = 0usize;
        while index < bytes.len() {
            {
                let mut writer = session.writer.blocking_lock();
                writer
                    .write_all(&bytes[index..index + 1])
                    .with_context(|| {
                        format!("failed writing keys for instance {}", self.config.id)
                    })?;
                writer.flush().context("failed flushing PTY writer")?;
            }

            if bytes[index] == 0x1b
                && bytes
                    .get(index + 1)
                    .map_or(true, |next| *next != b'[' && *next != b'O')
            {
                // Prevent accidental Alt-key combos when callers intend standalone Esc.
                thread::sleep(Duration::from_millis(40));
            }
            index += 1;
        }
        Ok(())
    }

    fn tail_log(&self, lines: usize) -> Result<Vec<String>> {
        let Some(path) = &self.config.log_path else {
            return Ok(Vec::new());
        };

        let body = match fs::read_to_string(path) {
            Ok(body) => body,
            Err(_) => return Ok(Vec::new()),
        };

        let mut result: Vec<String> = body.lines().map(ToOwned::to_owned).collect();
        if result.len() > lines {
            result = result.split_off(result.len() - lines);
        }
        Ok(result)
    }

    fn health_check(&self) -> Result<bool> {
        Ok(self.state == BackendState::Running && self.session.is_some())
    }

    fn is_healthy(&self) -> bool {
        self.state == BackendState::Running
    }
}

impl Drop for LocalPtyBackend {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

#[cfg(test)]
mod tests {
    use std::thread;
    use std::time::Duration;

    use super::*;
    use crate::config::InstanceMode;

    fn test_config() -> InstanceConfig {
        InstanceConfig {
            id: "local-test".to_string(),
            mode: InstanceMode::Local,
            data_dir: std::env::temp_dir().join("aura-harness-local-test"),
            device_id: None,
            bind_address: "127.0.0.1:41001".to_string(),
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
            tunnel: None,
        }
    }

    #[test]
    fn local_backend_start_send_snapshot_stop() {
        let mut backend = LocalPtyBackend::new(test_config(), Some(40), Some(120));
        if let Err(error) = backend.start() {
            panic!("backend must start: {error}");
        }
        if let Err(error) = backend.send_keys("hello-harness\n") {
            panic!("keys send failed: {error}");
        }
        thread::sleep(Duration::from_millis(80));
        let screen = match backend.snapshot() {
            Ok(screen) => screen,
            Err(error) => panic!("snapshot failed: {error}"),
        };
        assert!(screen.contains("hello-harness"));
        if let Err(error) = backend.stop() {
            panic!("backend must stop: {error}");
        }
    }
}
