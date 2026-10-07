//! Multi-runtime fixture on virtual time for the CLI command model and
//! `aura rpc` (the terminal-side twin of aura-agent's tests/support).
//!
//! All runtimes share one transport and one [`QuiescentClock`]; tests run on
//! a paused tokio clock (`#[tokio::test(start_paused = true)]`), so virtual
//! time advances only when every runtime is idle.

use anyhow::{anyhow, Result};
use async_lock::RwLock;
use aura_agent::{AgentBuilder, AgentConfig, AuraAgent, SharedTransport};
use aura_app::ui::types::{AppConfig, AppCore};
use aura_core::context::EffectContext;
use aura_core::effects::ExecutionMode;
use aura_core::types::identifiers::{AuthorityId, ContextId, DeviceId};
use aura_terminal::command::CommandContext;
use aura_terminal::rpc;
use aura_testkit::time::QuiescentClock;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, Lines};

/// Virtual-time bound for one awaited condition.
pub const WAIT: Duration = Duration::from_secs(60);
const RECHECK: Duration = Duration::from_millis(50);

pub struct SimNet {
    transport: SharedTransport,
    clock: QuiescentClock,
}

pub struct Peer {
    _temp: tempfile::TempDir,
    _agent: Arc<AuraAgent>,
    pub app: Arc<RwLock<AppCore>>,
    pub ctx: CommandContext,
    pub id: AuthorityId,
}

impl SimNet {
    pub fn new() -> Self {
        Self {
            transport: SharedTransport::new(),
            clock: QuiescentClock::start(),
        }
    }

    /// A simulation runtime on the shared transport and clock.
    pub async fn peer(&self, seed: u8) -> Result<Peer> {
        let temp = tempfile::tempdir()?;
        let id = AuthorityId::new_from_entropy([seed; 32]);
        let ctx = EffectContext::new(
            id,
            ContextId::new_from_entropy([seed.wrapping_add(1); 32]),
            ExecutionMode::Testing,
        );
        let mut config = AgentConfig {
            device_id: DeviceId::new_from_entropy([seed.wrapping_add(2); 32]),
            ..AgentConfig::default()
        };
        config.storage.base_path = temp.path().join("aura");
        let agent = Box::pin(
            AgentBuilder::new()
                .with_authority(id)
                .with_config(config)
                .with_physical_time_provider(self.clock.provider())
                .build_simulation_async_with_shared_transport(
                    u64::from(seed),
                    &ctx,
                    self.transport.clone(),
                ),
        )
        .await?;
        let agent = Arc::new(agent);
        let app = Arc::new(RwLock::new(AppCore::with_runtime(
            AppConfig::default(),
            agent.clone().as_runtime_bridge(),
        )?));
        AppCore::init_signals_with_hooks(&app).await?;
        app.read()
            .await
            .bootstrap_signing_keys()
            .await
            .map_err(|e| anyhow!("bootstrap: {e}"))?;
        let ctx = CommandContext::new(app.clone(), agent.runtime().effects(), id);
        Ok(Peer {
            _temp: temp,
            _agent: agent,
            app,
            ctx,
            id,
        })
    }
}

/// A client attached to an `aura rpc` session over an in-memory pipe.
pub struct RpcClient {
    writer: DuplexStream,
    lines: Lines<BufReader<DuplexStream>>,
    next_id: u64,
    /// Event lines received while waiting for responses.
    pub events: Vec<Value>,
    pub hello: Value,
}

impl RpcClient {
    /// Start a session on `peer`; the server runs on the current task
    /// through the returned future, which the test must drive.
    pub async fn connect(
        peer: &Peer,
    ) -> Result<(
        Self,
        impl std::future::Future<Output = std::io::Result<()>> + '_,
    )> {
        let (client_out, server_in) = tokio::io::duplex(1 << 20);
        let (server_out, client_in) = tokio::io::duplex(1 << 20);
        let server = rpc::serve(&peer.ctx, BufReader::new(server_in), server_out);
        let client = Self {
            writer: client_out,
            lines: BufReader::new(client_in).lines(),
            next_id: 1,
            events: Vec::new(),
            hello: Value::Null,
        };
        Ok((client, server))
    }

    pub async fn send_line(&mut self, line: &str) -> Result<()> {
        if std::env::var_os("AURA_RPC_TRACE").is_some() {
            eprintln!("-> {line}");
        }
        self.writer.write_all(line.as_bytes()).await?;
        self.writer.write_all(b"\n").await?;
        Ok(())
    }

    /// Read the next line from the server.
    pub async fn read_line(&mut self) -> Result<Value> {
        let line = self
            .lines
            .next_line()
            .await?
            .ok_or_else(|| anyhow!("rpc session closed"))?;
        if std::env::var_os("AURA_RPC_TRACE").is_some() {
            eprintln!("<- {line}");
        }
        Ok(serde_json::from_str(&line)?)
    }

    /// Send `method`/`params` and return the response, keeping events.
    pub async fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        self.send_line(&json!({"id": id, "method": method, "params": params}).to_string())
            .await?;
        loop {
            let line = self.read_line().await?;
            match line["type"].as_str() {
                Some("hello") => self.hello = line,
                Some("event") => self.events.push(line),
                Some("response") if line["id"] == json!(id) => return Ok(line),
                _ => return Err(anyhow!("unexpected line {line}")),
            }
        }
    }

    /// `call`, requiring success; returns `result`.
    pub async fn ok(&mut self, method: &str, params: Value) -> Result<Value> {
        let response = self.call(method, params).await?;
        if response["ok"] != json!(true) {
            return Err(anyhow!("{method} failed: {response}"));
        }
        Ok(response["result"].clone())
    }

    /// Repeat `method` at each quiescent point until its response satisfies
    /// `pred` (for conditions no event reports).
    pub async fn call_until(
        &mut self,
        what: &str,
        method: &str,
        params: Value,
        pred: impl Fn(&Value) -> bool,
    ) -> Result<Value> {
        let deadline = tokio::time::Instant::now() + WAIT;
        loop {
            let response = self.call(method, params.clone()).await?;
            if pred(&response) {
                return Ok(response);
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(anyhow!(
                    "{what}: not reached within {WAIT:?}; last {response}"
                ));
            }
            tokio::time::sleep(RECHECK).await;
        }
    }

    /// The next event line matching `pred`; earlier events are kept.
    pub async fn wait_event(&mut self, what: &str, pred: impl Fn(&Value) -> bool) -> Result<Value> {
        if let Some(found) = self.events.iter().find(|e| pred(e)) {
            return Ok(found.clone());
        }
        let deadline = tokio::time::sleep(WAIT);
        tokio::pin!(deadline);
        loop {
            tokio::select! {
                line = self.read_line() => {
                    let line = line?;
                    if line["type"] == "event" {
                        let matched = pred(&line);
                        self.events.push(line.clone());
                        if matched {
                            return Ok(line);
                        }
                    }
                }
                () = &mut deadline => {
                    return Err(anyhow!("{what}: no event within {WAIT:?} of virtual time"));
                }
            }
        }
    }

    /// End the session.
    pub async fn shutdown(&mut self) -> Result<Value> {
        self.call("shutdown", Value::Null).await
    }
}
