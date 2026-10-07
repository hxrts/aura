//! `aura rpc`: the command model as a long-lived JSON-lines protocol.
//!
//! One runtime stays online across requests, so the node keeps receiving
//! and converging between them. Every line is one JSON object.
//!
//! The server first writes a hello line:
//!
//! ```json
//! {"type":"hello","protocol":"aura-rpc","version":1,"aura_version":"0.2.0","capabilities":[..],"methods":[..]}
//! ```
//!
//! A client request is a [`Request`](crate::command::Request) plus an `id`
//! the server echoes (any JSON value):
//!
//! ```json
//! {"id":1,"method":"chat_send","params":{"channel":"general","message":"hi"}}
//! ```
//!
//! and its response carries the typed result or the typed error, exactly as
//! `aura --json` prints them:
//!
//! ```json
//! {"type":"response","id":1,"ok":true,"result":{"type":"message_sent","data":{..}}}
//! {"type":"response","id":1,"ok":false,"error":{"code":"not_found","message":".."}}
//! ```
//!
//! Requests run concurrently and may complete out of order. The session
//! ends on EOF or a `shutdown` request, after in-flight requests finish.
//! [`events`] adds `subscribe`/`unsubscribe` and event lines.

pub mod events;

use crate::command::{execute, CommandContext, CommandError, Request, Response};
use futures::stream::{FuturesUnordered, StreamExt};
use serde_json::{json, Value};
use std::future::Future;
use std::pin::Pin;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};

/// Wire protocol name in the hello line.
pub const PROTOCOL: &str = "aura-rpc";
/// Wire protocol version; bumped on incompatible changes.
pub const PROTOCOL_VERSION: u32 = 1;

/// Methods every request line may use besides the [`Request`] methods.
pub const CONTROL_METHODS: &[&str] = &["shutdown", "subscribe", "unsubscribe"];

/// The hello line written when a session starts.
#[must_use]
pub fn hello() -> Value {
    json!({
        "type": "hello",
        "protocol": PROTOCOL,
        "version": PROTOCOL_VERSION,
        "aura_version": env!("CARGO_PKG_VERSION"),
        "capabilities": ["requests", "concurrent_requests", "events"],
        "methods": request_methods(),
        "control": CONTROL_METHODS,
        "topics": events::Topic::ALL.iter().map(|t| t.name()).collect::<Vec<_>>(),
    })
}

/// The `method` names of every [`Request`].
#[must_use]
pub fn request_methods() -> Vec<String> {
    crate::command::request::all_request_examples()
        .iter()
        .map(Request::method)
        .collect()
}

/// The response line for a request's outcome.
#[must_use]
pub fn response_line(id: &Value, outcome: &Result<Response, CommandError>) -> Value {
    match outcome {
        Ok(result) => json!({"type": "response", "id": id, "ok": true, "result": result}),
        Err(error) => json!({"type": "response", "id": id, "ok": false, "error": error}),
    }
}

/// One parsed client line.
#[derive(Debug)]
pub enum Incoming {
    /// A command request.
    Request { id: Value, request: Request },
    /// End the session.
    Shutdown { id: Value },
    /// Start streaming events.
    Subscribe {
        id: Value,
        topics: Vec<events::Topic>,
    },
    /// Stop a subscription.
    Unsubscribe { id: Value, subscription: u64 },
}

/// Parse one client line; a malformed line fails with the id it carried
/// (or `null`).
pub fn parse_line(line: &str) -> Result<Incoming, (Value, CommandError)> {
    let mut value: Value = serde_json::from_str(line).map_err(|e| {
        (
            Value::Null,
            CommandError::invalid(format!("invalid JSON: {e}")),
        )
    })?;
    let Some(object) = value.as_object_mut() else {
        return Err((
            Value::Null,
            CommandError::invalid("a request line must be a JSON object"),
        ));
    };
    let id = object.remove("id").unwrap_or(Value::Null);
    let method = object
        .get("method")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| (id.clone(), CommandError::invalid("missing method")))?;
    let params = object.get("params").cloned().unwrap_or(Value::Null);
    match method.as_str() {
        "shutdown" => Ok(Incoming::Shutdown { id }),
        "subscribe" => {
            let topics = events::parse_topics(&params).map_err(|e| (id.clone(), e))?;
            Ok(Incoming::Subscribe { id, topics })
        }
        "unsubscribe" => {
            let subscription = params
                .get("subscription")
                .and_then(Value::as_u64)
                .ok_or_else(|| {
                    (
                        id.clone(),
                        CommandError::invalid("unsubscribe needs params.subscription"),
                    )
                })?;
            Ok(Incoming::Unsubscribe { id, subscription })
        }
        _ => serde_json::from_value::<Request>(Value::Object(object.clone()))
            .map(|request| Incoming::Request {
                id: id.clone(),
                request,
            })
            .map_err(|e| {
                (
                    id.clone(),
                    CommandError::invalid(format!("invalid {method} request: {e}")),
                )
            }),
    }
}

/// Execute one request line and return its response line. Control methods
/// are not handled here; [`serve`] handles them.
pub async fn handle_line(ctx: &CommandContext, line: &str) -> Value {
    match parse_line(line) {
        Ok(Incoming::Request { id, request }) => response_line(&id, &execute(ctx, request).await),
        Ok(Incoming::Shutdown { id })
        | Ok(Incoming::Subscribe { id, .. })
        | Ok(Incoming::Unsubscribe { id, .. }) => response_line(
            &id,
            &Err(CommandError::invalid(
                "control methods need a session; use serve",
            )),
        ),
        Err((id, error)) => response_line(&id, &Err(error)),
    }
}

async fn write_line<W: AsyncWrite + Unpin>(writer: &mut W, value: &Value) -> std::io::Result<()> {
    let mut line = value.to_string();
    line.push('\n');
    writer.write_all(line.as_bytes()).await?;
    writer.flush().await
}

type Pending<'a> = Pin<Box<dyn Future<Output = Value> + 'a>>;

/// Run one session: hello, then requests until EOF or `shutdown`, with
/// subscribed events interleaved.
pub async fn serve<R, W>(ctx: &CommandContext, reader: R, mut writer: W) -> std::io::Result<()>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    write_line(&mut writer, &hello()).await?;
    let mut lines = reader.lines();
    let mut inflight: FuturesUnordered<Pending<'_>> = FuturesUnordered::new();
    let mut subscriptions = events::Subscriptions::new(ctx.app_core().clone());
    let mut shutdown: Option<Value> = None;
    let mut reading = true;
    loop {
        if !reading && inflight.is_empty() {
            break;
        }
        tokio::select! {
            line = lines.next_line(), if reading => {
                match line? {
                    None => reading = false,
                    Some(line) if line.trim().is_empty() => {}
                    Some(line) => match parse_line(&line) {
                        Ok(Incoming::Request { id, request }) => {
                            inflight.push(Box::pin(async move {
                                response_line(&id, &execute(ctx, request).await)
                            }));
                        }
                        Ok(Incoming::Shutdown { id }) => {
                            shutdown = Some(id);
                            reading = false;
                        }
                        Ok(Incoming::Subscribe { id, topics }) => {
                            let outcome = subscriptions.subscribe(topics).await;
                            write_line(&mut writer, &events::subscribe_response(&id, outcome)).await?;
                        }
                        Ok(Incoming::Unsubscribe { id, subscription }) => {
                            let outcome = subscriptions.unsubscribe(subscription);
                            write_line(&mut writer, &events::unsubscribe_response(&id, outcome)).await?;
                        }
                        Err((id, error)) => {
                            write_line(&mut writer, &response_line(&id, &Err(error))).await?;
                        }
                    },
                }
            }
            Some(response) = inflight.next(), if !inflight.is_empty() => {
                write_line(&mut writer, &response).await?;
            }
            Some(event) = subscriptions.next_event() => {
                write_line(&mut writer, &event).await?;
            }
        }
    }
    if let Some(id) = shutdown {
        write_line(
            &mut writer,
            &json!({"type": "response", "id": id, "ok": true, "result": {"type": "shutdown"}}),
        )
        .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_lines_carry_ids_and_typed_requests() {
        let Ok(Incoming::Request { id, request }) =
            parse_line(r#"{"id":"a","method":"chat_send","params":{"channel":"c","message":"m"}}"#)
        else {
            panic!("expected a request");
        };
        assert_eq!(id, json!("a"));
        assert_eq!(
            request,
            Request::ChatSend {
                channel: "c".into(),
                message: "m".into()
            }
        );
    }

    #[test]
    fn malformed_lines_fail_with_their_id() {
        let Err((id, error)) = parse_line(r#"{"id":7,"method":"chat_send","params":{}}"#) else {
            panic!("expected an error");
        };
        assert_eq!(id, json!(7));
        assert_eq!(error.code, crate::command::ErrorCode::InvalidInput);
        assert!(parse_line("not json").is_err());
        assert!(matches!(
            parse_line(r#"{"id":1,"method":"shutdown"}"#),
            Ok(Incoming::Shutdown { .. })
        ));
    }

    #[test]
    fn advertised_methods_are_distinct_and_parse_back() {
        let methods = request_methods();
        let distinct: std::collections::BTreeSet<&String> = methods.iter().collect();
        assert_eq!(distinct.len(), methods.len());
        for example in crate::command::request::all_request_examples() {
            let line = serde_json::to_value(&example).unwrap().to_string();
            assert!(matches!(parse_line(&line), Ok(Incoming::Request { .. })));
        }
        assert_eq!(hello()["version"], PROTOCOL_VERSION);
    }
}
