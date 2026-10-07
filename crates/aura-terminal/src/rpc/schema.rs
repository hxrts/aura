//! The published, versioned JSON Schema of the `aura rpc` protocol.
//!
//! Generated from the request, response, error and line types. The checked-in
//! copy lives at `crates/aura-terminal/schema/aura-rpc-v<N>.json`; the test
//! below fails when it drifts from the types. Regenerate with
//! `AURA_UPDATE_RPC_SCHEMA=1 cargo test -p aura-terminal --lib rpc::schema`.

use super::PROTOCOL_VERSION;
use crate::command::{CommandError, Request, Response};
use schemars::{schema_for, JsonSchema};
use serde_json::{json, Value};

/// A client line: a request plus its echoed id and optional deadline.
#[derive(JsonSchema)]
#[allow(dead_code)]
struct RequestLine {
    /// Any JSON value; echoed in the response.
    id: Value,
    #[serde(flatten)]
    request: Request,
    /// Fail with `timeout` when the request takes longer, on the runtime clock.
    timeout_secs: Option<u64>,
}

/// A control line: `shutdown`, `subscribe` (`params.topics`) or
/// `unsubscribe` (`params.subscription`).
#[derive(JsonSchema)]
#[allow(dead_code)]
struct ControlLine {
    id: Value,
    /// `shutdown`, `subscribe` or `unsubscribe`.
    method: String,
    params: Option<Value>,
}

/// A server line.
#[derive(JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
#[allow(dead_code)]
enum ServerLine {
    /// First line of every session.
    Hello {
        protocol: String,
        version: u32,
        aura_version: String,
        capabilities: Vec<String>,
        methods: Vec<String>,
        control: Vec<String>,
        topics: Vec<String>,
    },
    /// The outcome of one request: `result` when `ok`, else `error`.
    Response {
        id: Value,
        ok: bool,
        result: Option<Response>,
        error: Option<CommandError>,
    },
    /// A subscription event; `topic` is `resync` when the subscriber fell
    /// behind and `data` is a fresh snapshot.
    Event {
        subscription: u64,
        topic: String,
        kind: String,
        data: Value,
    },
}

/// The protocol schema document.
#[must_use]
pub fn schema() -> Value {
    json!({
        "$id": format!("aura-rpc/v{PROTOCOL_VERSION}"),
        "title": "aura rpc",
        "version": PROTOCOL_VERSION,
        "client_line": {"oneOf": [schema_for!(RequestLine), schema_for!(ControlLine)]},
        "server_line": schema_for!(ServerLine),
        "topics": super::events::Topic::ALL.map(super::events::Topic::name),
    })
}

/// Path of the checked-in schema for this protocol version.
#[must_use]
pub fn published_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("schema")
        .join(format!("aura-rpc-v{PROTOCOL_VERSION}.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn published_schema_matches_the_types() {
        let generated = serde_json::to_string_pretty(&schema()).unwrap() + "\n";
        let path = published_path();
        if std::env::var_os("AURA_UPDATE_RPC_SCHEMA").is_some() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &generated).unwrap();
        }
        let published = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(
            published == generated,
            "{} is out of date; run AURA_UPDATE_RPC_SCHEMA=1 cargo test -p aura-terminal --lib rpc::schema \
             (bump rpc::PROTOCOL_VERSION for incompatible changes)",
            path.display()
        );
    }

    #[test]
    fn every_request_method_appears_in_the_schema() {
        let text = schema().to_string();
        for method in super::super::request_methods() {
            assert!(text.contains(&format!("\"{method}\"")), "{method}");
        }
    }
}
