//! The CLI command model and `aura rpc` on the virtual-time multi-runtime
//! fixture (work/8.md Tasks 156-159):
//!
//! - CLI and RPC give identical responses for the same request.
//! - Two agents link contacts, share a home and exchange messages through
//!   `aura rpc` alone; the receiver waits for the message on an event.
//! - A message sent by the CLI command model reaches the same semantic
//!   outcome as one sent through the TUI's effect dispatch.

#![allow(missing_docs, clippy::unwrap_used, clippy::expect_used)]

#[path = "cli_rpc/simnet.rs"]
mod simnet;

use anyhow::{anyhow, Result};
use aura_terminal::cli::commands::{cli_parser, Commands};
use aura_terminal::command::{execute, Outcome, OutputMode, Request};
use aura_terminal::rpc;
use bpaf::{Args, Parser};
use serde_json::{json, Value};
use simnet::{Peer, RpcClient, SimNet};

fn request(argv: &[&str]) -> Request {
    match cli_parser()
        .to_options()
        .run_inner(Args::from(argv))
        .unwrap_or_else(|e| panic!("{argv:?}: {e:?}"))
        .command
    {
        Commands::Run(request) => request,
        other => panic!("{argv:?} is not a request: {other:?}"),
    }
}

fn data(result: &Value, field: &str) -> Result<String> {
    result["data"][field]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| anyhow!("missing data.{field} in {result}"))
}

fn ok(response: &Value) -> bool {
    response["ok"] == json!(true)
}

fn lists(response: &Value, field: &str, value: &str) -> bool {
    ok(response)
        && response["result"]["data"]
            .as_array()
            .is_some_and(|items| items.iter().any(|item| item[field] == value))
}

/// `aura --json` and `aura rpc` give the same `ok` and `result`/`error` for
/// the same request, success or failure.
#[tokio::test(start_paused = true)]
async fn cli_and_rpc_give_identical_responses() -> Result<()> {
    let net = SimNet::new();
    let peer = net.peer(21).await?;
    let cases: &[&[&str]] = &[
        &["authority", "list"],
        &["contact", "list"],
        &["chat", "list"],
        &["invite", "list"],
        &["sync", "status"],
        &["recovery", "status"],
        &["chat", "search", "hello"],
        &["chat", "history", "no-such-channel"],
        &["amp", "inspect", "--context", "bad", "--channel", "bad"],
        &["invite", "accept", "--invitation-id", "missing"],
    ];
    for argv in cases {
        let request = request(argv);
        // The CLI path: execute, then the `--json` document.
        let cli = match execute(&peer.ctx, request.clone()).await {
            Ok(response) => json!({"ok": true, "result": Outcome::from_response(&response).json}),
            Err(error) => OutputMode::failure_document(&error),
        };
        // The RPC path: the same request as a JSON line.
        let mut line = serde_json::to_value(&request)?;
        line["id"] = json!(argv.join(" "));
        let rpc = rpc::handle_line(&peer.ctx, &line.to_string()).await;
        assert_eq!(rpc["id"], json!(argv.join(" ")));
        assert_eq!(rpc["ok"], cli["ok"], "{argv:?}");
        let key = if ok(&cli) { "result" } else { "error" };
        assert_eq!(rpc[key], cli[key], "{argv:?}: CLI and RPC disagree");
    }
    Ok(())
}

/// Through RPC only: `a` invites `b` as a contact, then into a new home;
/// returns the home id once `b` holds the home channel.
async fn link_and_share_home(
    a: &mut RpcClient,
    b: &mut RpcClient,
    a_id: &str,
    b_id: &str,
) -> Result<String> {
    let created = a
        .ok("invite_create", json!({"invitee": b_id, "role": "contact"}))
        .await?;
    let imported = b
        .ok("invite_import", json!({"code": data(&created, "code")?}))
        .await?;
    b.ok(
        "invite_accept",
        json!({"invitation_id": data(&imported, "invitation_id")?}),
    )
    .await?;
    for (client, other) in [(&mut *a, b_id), (&mut *b, a_id)] {
        client
            .call_until("contact link", "contact_list", Value::Null, |r| {
                lists(r, "authority_id", other)
            })
            .await?;
    }

    let home = a.ok("home_create", json!({"name": "AHome"})).await?;
    let home_id = data(&home, "home_id")?;
    a.ok("home_invite", json!({"authority": b_id})).await?;
    b.call_until("home invitation accepted", "home_accept", Value::Null, ok)
        .await?;
    b.call_until(
        "invitee holds the home channel",
        "chat_show",
        json!({"channel": home_id}),
        ok,
    )
    .await?;
    Ok(home_id)
}

/// Alex and Barbara, each driven only through `aura rpc`, become contacts,
/// share a home and exchange messages. Barbara waits for Alex's message on
/// an event, without polling.
#[tokio::test(start_paused = true)]
async fn contact_home_and_messaging_flows_through_rpc() -> Result<()> {
    let net = SimNet::new();
    let alex = net.peer(31).await?;
    let barbara = net.peer(35).await?;
    let (alex_id, barbara_id) = (alex.id.to_string(), barbara.id.to_string());
    let (mut a, alex_server) = RpcClient::connect(&alex).await?;
    let (mut b, barbara_server) = RpcClient::connect(&barbara).await?;

    // The script owns the clients: when it ends, either way, the sessions see EOF.
    let script = async move {
        // Barbara watches messages, Alex invitations, from the start.
        let subscribed = b.ok("subscribe", json!({"topics": ["messages"]})).await?;
        assert_eq!(subscribed["type"], "subscribed");
        assert_eq!(b.hello["protocol"], "aura-rpc");
        a.ok("subscribe", json!({"topics": ["invitations"]}))
            .await?;

        // Barbara invites Alex as a contact, then into her home.
        let home_id = link_and_share_home(&mut b, &mut a, &barbara_id, &alex_id).await?;
        assert!(
            a.events.iter().any(|e| e["topic"] == "invitations"),
            "Alex saw no invitation event"
        );

        // Barbara receives Alex's message as an event.
        let sent = a
            .ok(
                "chat_send",
                json!({"channel": home_id, "message": "hello barbara"}),
            )
            .await?;
        assert_eq!(sent["type"], "message_sent");
        let event = b
            .wait_event("Barbara receives Alex's message", |e| {
                e["topic"] == "messages" && e["data"]["content"] == "hello barbara"
            })
            .await?;
        assert_eq!(event["data"]["sender_id"], json!(alex_id));

        // And back: Alex reads Barbara's reply in the channel history.
        b.ok(
            "chat_send",
            json!({"channel": home_id, "message": "hi alex"}),
        )
        .await?;
        a.call_until(
            "Alex receives Barbara's reply",
            "chat_history",
            json!({"channel": home_id}),
            |r| lists(r, "content", "hi alex"),
        )
        .await?;

        assert_eq!(a.shutdown().await?["ok"], true);
        b.shutdown().await?;
        Ok::<(), anyhow::Error>(())
    };
    let (outcome, alex_served, barbara_served) = tokio::join!(script, alex_server, barbara_server);
    alex_served?;
    barbara_served?;
    outcome
}

/// The TUI's message send (effect dispatch through `IoContext`).
async fn tui_send(peer: &Peer, channel: &str, content: &str) -> Result<()> {
    use aura_terminal::handlers::tui::TuiMode;
    use aura_terminal::tui::context::{InitializedAppCore, IoContext};
    use aura_terminal::tui::effects::EffectCommand;
    let base = tempfile::tempdir()?;
    let ctx = IoContext::builder()
        .with_app_core(InitializedAppCore::new(peer.app.clone()).await?)
        .with_existing_account(true)
        .with_base_path(base.path().to_path_buf())
        .with_device_id("tui-parity".to_string())
        .with_mode(TuiMode::Production)
        .build()?;
    ctx.dispatch(EffectCommand::SendMessage {
        channel: channel.to_string(),
        content: content.to_string(),
    })
    .await?;
    Ok(())
}

/// The same send through the CLI command model and through the TUI ends in
/// the same semantic operation outcome, and both messages arrive.
#[tokio::test(start_paused = true)]
async fn cli_send_reaches_the_same_outcome_as_tui_send() -> Result<()> {
    let net = SimNet::new();
    let alex = net.peer(51).await?;
    let barbara = net.peer(55).await?;
    let (alex_id, barbara_id) = (alex.id.to_string(), barbara.id.to_string());
    let (mut a, alex_server) = RpcClient::connect(&alex).await?;
    let (mut b, barbara_server) = RpcClient::connect(&barbara).await?;
    let alex_peer = &alex;

    // The script owns the clients: when it ends, either way, the sessions see EOF.
    let script = async move {
        let home_id = link_and_share_home(&mut b, &mut a, &barbara_id, &alex_id).await?;
        a.ok("subscribe", json!({"topics": ["operations"]})).await?;

        let sent = a
            .ok(
                "chat_send",
                json!({"channel": home_id, "message": "from the cli"}),
            )
            .await?;
        let cli_operation = sent["data"]["operation"].clone();
        assert_eq!(cli_operation["kind"], "send_chat_message");
        assert_eq!(cli_operation["phase"], "succeeded");

        // Let the CLI send's own lifecycle events drain, then watch the TUI's.
        a.ok("chat_list", Value::Null).await?;
        a.events.clear();
        tui_send(alex_peer, &home_id, "from the tui").await?;
        let tui_operation = a
            .wait_event("the TUI send settles", |e| {
                e["topic"] == "operations"
                    && e["data"]["kind"] == "send_chat_message"
                    && e["data"]["phase"] == "succeeded"
            })
            .await?;
        assert_eq!(tui_operation["data"]["kind"], cli_operation["kind"]);
        assert_eq!(tui_operation["data"]["phase"], cli_operation["phase"]);
        assert_eq!(tui_operation["data"]["error"], Value::Null);

        for text in ["from the cli", "from the tui"] {
            b.call_until(
                &format!("Barbara receives {text:?}"),
                "chat_history",
                json!({"channel": home_id}),
                |r| lists(r, "content", text),
            )
            .await?;
        }
        a.shutdown().await?;
        b.shutdown().await?;
        Ok::<(), anyhow::Error>(())
    };
    let (outcome, alex_served, barbara_served) = tokio::join!(script, alex_server, barbara_server);
    alex_served?;
    barbara_served?;
    outcome
}

/// Friend requests, contact nicknames, direct messages, members, settings,
/// notifications, budget and peers, all through `aura rpc` (Tasks 149-155).
#[tokio::test(start_paused = true)]
async fn social_and_account_commands_through_rpc() -> Result<()> {
    let net = SimNet::new();
    let alex = net.peer(81).await?;
    let barbara = net.peer(85).await?;
    let (alex_id, barbara_id) = (alex.id.to_string(), barbara.id.to_string());
    let (mut a, alex_server) = RpcClient::connect(&alex).await?;
    let (mut b, barbara_server) = RpcClient::connect(&barbara).await?;

    let script = async move {
        b.ok("subscribe", json!({"topics": ["messages"]})).await?;
        let home_id = link_and_share_home(&mut b, &mut a, &barbara_id, &alex_id).await?;

        // Friend request: Alex asks, Barbara sees it in her notifications and accepts.
        a.ok("friend_request", json!({"contact": barbara_id}))
            .await?;
        b.call_until(
            "Barbara's notifications list Alex's friend request",
            "notifications_list",
            Value::Null,
            |r| lists(r, "kind", "friend_request"),
        )
        .await?;
        b.ok("friend_accept", json!({"contact": alex_id})).await?;

        // A local nickname shows in the contact list.
        a.ok(
            "contact_rename",
            json!({"contact": barbara_id, "nickname": "Barb"}),
        )
        .await?;
        a.call_until("the nickname shows", "contact_list", Value::Null, |r| {
            lists(r, "nickname", "Barb")
        })
        .await?;
        let whois = a.ok("whois", json!({"target": barbara_id})).await?;
        assert_eq!(whois["type"], "contact");

        // Direct message reaches Barbara as an event.
        a.ok(
            "chat_dm",
            json!({"contact": barbara_id, "message": "dm hi"}),
        )
        .await?;
        b.wait_event("Barbara receives the DM", |e| {
            e["topic"] == "messages" && e["data"]["content"] == "dm hi"
        })
        .await?;

        // Read-side account commands answer with their typed responses.
        let members = b.ok("chat_members", json!({"channel": home_id})).await?;
        assert_eq!(members["type"], "members");
        assert_eq!(a.ok("budget", Value::Null).await?["type"], "budget");
        // Simulation runtimes run no rendezvous service: a typed failure.
        let peers = a.call("peer_list", Value::Null).await?;
        assert!(
            peers["result"]["type"] == "peer_list" || peers["error"]["code"].is_string(),
            "{peers}"
        );
        assert_eq!(
            a.ok("chat_mark_read", json!({"channel": home_id})).await?["type"],
            "done"
        );

        a.shutdown().await?;
        b.shutdown().await?;
        Ok::<(), anyhow::Error>(())
    };
    let (outcome, alex_served, barbara_served) = tokio::join!(script, alex_server, barbara_server);
    alex_served?;
    barbara_served?;
    outcome
}

/// A request line with a bad parameter fails with a typed error carrying
/// its id; the session continues.
#[tokio::test(start_paused = true)]
async fn malformed_requests_fail_typed_and_the_session_continues() -> Result<()> {
    let net = SimNet::new();
    let peer = net.peer(41).await?;
    let (mut client, server) = RpcClient::connect(&peer).await?;
    // The script owns the clients: when it ends, either way, the sessions see EOF.
    let script = async move {
        let bad = client.call("chat_send", json!({"channel": "x"})).await?;
        assert_eq!(bad["ok"], false);
        assert_eq!(bad["error"]["code"], "invalid_input");
        let missing = client
            .call("chat_history", json!({"channel": "no-such-channel"}))
            .await?;
        assert_eq!(missing["error"]["code"], "not_found");
        let listed = client.ok("chat_list", Value::Null).await?;
        assert_eq!(listed["type"], "channels");
        let topics = client
            .call("subscribe", json!({"topics": ["bogus"]}))
            .await?;
        assert_eq!(topics["error"]["code"], "invalid_input");
        client.shutdown().await?;
        Ok::<(), anyhow::Error>(())
    };
    let (outcome, served) = tokio::join!(script, server);
    served?;
    outcome
}
