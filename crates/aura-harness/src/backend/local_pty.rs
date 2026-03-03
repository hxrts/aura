use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use portable_pty::{native_pty_system, Child, CommandBuilder, PtySize};
use tokio::sync::Mutex;

use crate::backend::InstanceBackend;
use crate::config::InstanceConfig;
use crate::screen_normalization::{authoritative_screen, has_nav_header};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum BackendState {
    Stopped,
    Running,
}

struct RunningSession {
    child: Box<dyn Child + Send>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    parser: Arc<Mutex<vt100::Parser>>,
    parse_generation: Arc<AtomicU64>,
    reader_thread: Option<thread::JoinHandle<()>>,
}

pub struct LocalPtyBackend {
    config: InstanceConfig,
    state: BackendState,
    session: Option<RunningSession>,
    pty_rows: u16,
    pty_cols: u16,
    last_authoritative_screen: Arc<Mutex<Option<String>>>,
}

impl LocalPtyBackend {
    pub fn new(config: InstanceConfig, pty_rows: Option<u16>, pty_cols: Option<u16>) -> Self {
        Self {
            config,
            state: BackendState::Stopped,
            session: None,
            pty_rows: pty_rows.unwrap_or(40),
            pty_cols: pty_cols.unwrap_or(120),
            last_authoritative_screen: Arc::new(Mutex::new(None)),
        }
    }

    fn default_command(&self) -> (String, Vec<String>) {
        let program = std::env::var("AURA_HARNESS_AURA_BIN")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(PathBuf::from)
            .map(Self::absolutize_path)
            .or_else(|| {
                std::env::current_dir()
                    .ok()
                    .map(|cwd| cwd.join("target/debug/aura"))
            })
            .filter(|candidate| candidate.exists())
            .unwrap_or_else(|| PathBuf::from("aura"))
            .to_string_lossy()
            .to_string();

        let mut args = vec![
            "tui".to_string(),
            "--data-dir".to_string(),
            Self::absolutize_path(self.config.data_dir.clone())
                .to_string_lossy()
                .to_string(),
            "--bind-address".to_string(),
            self.config.bind_address.clone(),
        ];
        if let Some(device_id) = self.config.device_id.as_deref() {
            args.push("--device-id".to_string());
            args.push(device_id.to_string());
        }
        if self.config.demo_mode {
            args.push("--demo".to_string());
        }
        (program, args)
    }

    fn command_spec(&self) -> (String, Vec<String>) {
        match &self.config.command {
            Some(command) => (command.clone(), self.config.args.clone()),
            None => self.default_command(),
        }
    }

    fn parser_size(&self) -> (u16, u16) {
        (self.pty_rows, self.pty_cols)
    }

    fn read_screen(parser: &Arc<Mutex<vt100::Parser>>) -> String {
        let parser = parser.blocking_lock();
        parser.screen().contents()
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

    fn select_authoritative_screen(&self, current_screen: String) -> String {
        let current_authoritative = authoritative_screen(&current_screen);
        let mut cached = self.last_authoritative_screen.blocking_lock();

        if has_nav_header(&current_authoritative) {
            *cached = Some(current_authoritative.clone());
            return current_authoritative;
        }

        cached.clone().unwrap_or(current_authoritative)
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
        *self.last_authoritative_screen.blocking_lock() = None;

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

        // Harness runs default to clipboard isolation so local user clipboard is not mutated
        // by agent-driven TUI interactions. Per-instance env can explicitly override this.
        if Self::env_value("AURA_CLIPBOARD_MODE", &self.config.env).is_none() {
            command.env("AURA_CLIPBOARD_MODE", "file_only");
        }
        if Self::env_value("AURA_CLIPBOARD_FILE", &self.config.env).is_none() {
            let clipboard_file =
                Self::absolutize_path(self.config.data_dir.join(".harness-clipboard.txt"));
            command.env(
                "AURA_CLIPBOARD_FILE",
                clipboard_file.to_string_lossy().to_string(),
            );
        }

        for item in &self.config.env {
            if let Some((key, value)) = item.split_once('=') {
                if key.trim() == "AURA_CLIPBOARD_FILE" {
                    let resolved = Self::absolutize_path(PathBuf::from(value.trim()));
                    command.env(
                        "AURA_CLIPBOARD_FILE",
                        resolved.to_string_lossy().to_string(),
                    );
                    continue;
                }
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
        let parse_generation = Arc::new(AtomicU64::new(0));
        let parser_for_thread = Arc::clone(&parser);
        let generation_for_thread = Arc::clone(&parse_generation);
        let reader_thread = thread::spawn(move || {
            let mut buffer = [0u8; 4096];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(read) => {
                        parser_for_thread.blocking_lock().process(&buffer[..read]);
                        generation_for_thread.fetch_add(1, Ordering::Release);
                    }
                    Err(_) => break,
                }
            }
        });

        self.session = Some(RunningSession {
            child,
            writer: Arc::new(Mutex::new(writer)),
            parser,
            parse_generation,
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
        const SETTLE_DELAY_MS: u64 = 25;
        const MAX_SETTLE_ATTEMPTS: u8 = 8;
        const HEADER_RECOVERY_DELAY_MS: u64 = 20;
        const HEADER_RECOVERY_ATTEMPTS: u8 = 30;

        let mut last_generation = session.parse_generation.load(Ordering::Acquire);
        let mut last_screen = Self::read_screen(&session.parser);

        for _ in 0..MAX_SETTLE_ATTEMPTS {
            thread::sleep(Duration::from_millis(SETTLE_DELAY_MS));
            let current_generation = session.parse_generation.load(Ordering::Acquire);
            let current_screen = Self::read_screen(&session.parser);
            if current_generation == last_generation && current_screen == last_screen {
                return Ok(self.select_authoritative_screen(current_screen));
            }
            last_generation = current_generation;
            last_screen = current_screen;
        }

        // Transitional frames can briefly miss the nav header while a full-screen redraw
        // is still in flight. Sample for a short bounded window before falling back.
        let mut recovered_screen = last_screen;
        if !has_nav_header(&recovered_screen) {
            for _ in 0..HEADER_RECOVERY_ATTEMPTS {
                thread::sleep(Duration::from_millis(HEADER_RECOVERY_DELAY_MS));
                recovered_screen = Self::read_screen(&session.parser);
                if has_nav_header(&recovered_screen) {
                    break;
                }
            }
        }

        Ok(self.select_authoritative_screen(recovered_screen))
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

        let mut candidates = Vec::with_capacity(2);
        candidates.push(path.clone());
        candidates.push(PathBuf::from(format!("{}.dat", path.display())));

        let mut body: Option<String> = None;
        for candidate in candidates {
            let bytes = match fs::read(&candidate) {
                Ok(bytes) => bytes,
                Err(_) => continue,
            };
            body = Some(String::from_utf8_lossy(&bytes).into_owned());
            break;
        }
        let Some(body) = body else {
            return Ok(Vec::new());
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
    use std::fs;
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

    fn wait_for_screen_contains(
        backend: &mut LocalPtyBackend,
        needle: &str,
        timeout: Duration,
    ) -> String {
        const POLL_INTERVAL_MS: u128 = 30;
        let max_attempts_u128 = timeout
            .as_millis()
            .max(POLL_INTERVAL_MS)
            .div_ceil(POLL_INTERVAL_MS);
        let max_attempts = usize::try_from(max_attempts_u128).unwrap_or(usize::MAX);

        for attempt in 0..max_attempts {
            let screen = match backend.snapshot() {
                Ok(screen) => screen,
                Err(error) => panic!("snapshot failed: {error}"),
            };
            if screen.contains(needle) {
                return screen;
            }
            if attempt + 1 == max_attempts {
                panic!("timed out waiting for screen to contain {needle:?}; got: {screen:?}");
            }
            thread::sleep(Duration::from_millis(POLL_INTERVAL_MS as u64));
        }

        panic!("wait_for_screen_contains reached unreachable state");
    }

    #[test]
    fn local_backend_injects_default_clipboard_isolation_env() {
        let mut config = test_config();
        config.id = "local-test-clipboard-default".to_string();
        config.data_dir = std::env::temp_dir().join("aura-harness-local-clipboard-default");
        config.command = Some("bash".to_string());
        config.args = vec![
            "-lc".to_string(),
            "printf \"mode=%s file=%s\\n\" \"$AURA_CLIPBOARD_MODE\" \"$AURA_CLIPBOARD_FILE\"; cat"
                .to_string(),
        ];
        config.env = vec![];

        let mut backend = LocalPtyBackend::new(config, Some(20), Some(120));
        if let Err(error) = backend.start() {
            panic!("backend must start: {error}");
        }
        let screen =
            wait_for_screen_contains(&mut backend, "mode=file_only", Duration::from_secs(3));
        assert!(
            screen.contains("mode=file_only"),
            "expected default clipboard mode to be file_only, got: {screen:?}"
        );
        assert!(
            screen.contains("file="),
            "expected clipboard file to be present, got: {screen:?}"
        );
        if let Err(error) = backend.stop() {
            panic!("backend must stop: {error}");
        }
    }

    #[test]
    fn local_backend_default_command_targets_aura_tui() {
        let mut config = test_config();
        config.command = None;
        config.args.clear();
        config.data_dir = std::env::temp_dir().join("aura-harness-local-default-cmd");
        config.device_id = Some("local-test-device".to_string());
        config.bind_address = "127.0.0.1:49999".to_string();

        let backend = LocalPtyBackend::new(config, Some(20), Some(120));
        let (program, args) = backend.command_spec();

        assert!(
            program.ends_with("aura"),
            "default program must target aura binary, got: {program}"
        );
        assert!(
            args.iter().any(|arg| arg == "tui"),
            "default args must include tui subcommand: {args:?}"
        );
        assert!(
            args.windows(2)
                .any(|window| window == ["--bind-address", "127.0.0.1:49999"]),
            "default args must include bind address: {args:?}"
        );
        assert!(
            args.windows(2)
                .any(|window| window == ["--device-id", "local-test-device"]),
            "default args must include device id: {args:?}"
        );
        assert!(
            args.windows(2).any(|window| window[0] == "--data-dir"),
            "default args must include data dir: {args:?}"
        );
    }

    #[test]
    fn local_backend_respects_clipboard_env_override() {
        let mut config = test_config();
        config.id = "local-test-clipboard-override".to_string();
        config.data_dir = std::env::temp_dir().join("aura-harness-local-clipboard-override");
        config.command = Some("bash".to_string());
        config.args = vec![
            "-lc".to_string(),
            "printf \"mode=%s file=%s\\n\" \"$AURA_CLIPBOARD_MODE\" \"$AURA_CLIPBOARD_FILE\"; cat"
                .to_string(),
        ];
        config.env = vec![
            "AURA_CLIPBOARD_MODE=system".to_string(),
            "AURA_CLIPBOARD_FILE=/tmp/custom-harness-clipboard.txt".to_string(),
        ];

        let mut backend = LocalPtyBackend::new(config, Some(20), Some(120));
        if let Err(error) = backend.start() {
            panic!("backend must start: {error}");
        }
        let screen = wait_for_screen_contains(&mut backend, "mode=system", Duration::from_secs(3));
        assert!(
            screen.contains("mode=system"),
            "expected custom clipboard mode override, got: {screen:?}"
        );
        assert!(
            screen.contains("file=/tmp/custom-harness-clipboard.txt"),
            "expected custom clipboard file override, got: {screen:?}"
        );
        if let Err(error) = backend.stop() {
            panic!("backend must stop: {error}");
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

    #[test]
    fn local_snapshot_is_bounded_by_pty_rows() {
        let mut backend = LocalPtyBackend::new(test_config(), Some(40), Some(120));
        if let Err(error) = backend.start() {
            panic!("backend must start: {error}");
        }
        thread::sleep(Duration::from_millis(50));
        let screen = match backend.snapshot() {
            Ok(screen) => screen,
            Err(error) => panic!("snapshot must succeed: {error}"),
        };
        let line_count = screen.lines().count();
        assert!(
            line_count <= 40,
            "snapshot should not exceed configured PTY rows (got {line_count})"
        );
        if let Err(error) = backend.stop() {
            panic!("backend must stop: {error}");
        }
    }

    #[test]
    fn local_snapshot_observes_recent_output_without_extra_sleep() {
        let mut backend = LocalPtyBackend::new(test_config(), Some(40), Some(120));
        if let Err(error) = backend.start() {
            panic!("backend must start: {error}");
        }
        if let Err(error) = backend.send_keys("freshness-check\n") {
            panic!("send_keys must succeed: {error}");
        }
        let screen = match backend.snapshot() {
            Ok(screen) => screen,
            Err(error) => panic!("snapshot must succeed: {error}"),
        };
        assert!(screen.contains("freshness-check"));
        if let Err(error) = backend.stop() {
            panic!("backend must stop: {error}");
        }
    }

    #[test]
    fn local_tail_log_reads_dat_fallback_path() {
        let temp_root = std::env::temp_dir().join("aura-harness-tail-log-dat");
        let _ = fs::remove_dir_all(&temp_root);
        if let Err(error) = fs::create_dir_all(&temp_root) {
            panic!("create temp dir: {error}");
        }

        let mut config = test_config();
        config.data_dir = temp_root.clone();
        config.log_path = Some(temp_root.join("instance.log"));

        if let Err(error) = fs::write(temp_root.join("instance.log.dat"), "line-1\nline-2\n") {
            panic!("write log: {error}");
        }

        let backend = LocalPtyBackend::new(config, Some(40), Some(120));
        let lines = match backend.tail_log(1) {
            Ok(lines) => lines,
            Err(error) => panic!("tail_log should succeed: {error}"),
        };
        assert_eq!(lines, vec!["line-2".to_string()]);
    }

    #[test]
    fn authoritative_snapshot_falls_back_to_cached_tui_frame() {
        let backend = LocalPtyBackend::new(test_config(), Some(40), Some(120));
        let tui_frame = "Neighborhood Chat Contacts Notifications Settings\nframe".to_string();
        let noisy = "2026-01-01T00:00:00Z INFO log line".to_string();

        let first = backend.select_authoritative_screen(tui_frame.clone());
        assert_eq!(first, tui_frame);

        let second = backend.select_authoritative_screen(noisy);
        assert_eq!(second, first);
    }

    #[test]
    fn authoritative_snapshot_strips_stale_prefix_rows() {
        let backend = LocalPtyBackend::new(test_config(), Some(40), Some(120));
        let mixed = "\
stale footer row\n\
Neighborhood Chat Contacts Notifications Settings\n\
latest frame row";

        let selected = backend.select_authoritative_screen(mixed.to_string());
        assert_eq!(
            selected,
            "Neighborhood Chat Contacts Notifications Settings\nlatest frame row"
        );
    }
}
