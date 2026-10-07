//! Two-member home moderation over a shared in-memory transport.
//!
//! Barbara creates a home and invites her contact Alex, who joins. Barbara
//! then limits Alex's access and bans him. Both governance actions must take
//! effect on Alex's own client (his home view and his sender gate), and the
//! `/ban` strong command must observe the ban in Barbara's home ban list.

#![allow(missing_docs)]

mod support;

use anyhow::{anyhow, Result};
use async_lock::RwLock;
use aura_agent::{LinkFault, SharedTransport};
use aura_app::core::AppCore;
use aura_app::ui::signals::{CHAT_SIGNAL, CONTACTS_SIGNAL, HOMES_SIGNAL};
use aura_app::ui::workflows::{access, contacts, context, messaging, strong_command as sc};
use aura_core::effects::reactive::ReactiveEffects;
use aura_core::types::identifiers::{AuthorityId, ChannelId};
use std::sync::Arc;
use support::{home_view, join_home, link_contacts, strong, wait_until, SimNet};

#[tokio::test(start_paused = true)]
async fn limited_then_banned_member_is_refused_on_own_client() -> Result<()> {
    moderation_reaches_member(None).await
}

/// The override and ban fanout is delayed past the moderation workflow's
/// completion (held on the Alex<->Barbara link, then released).
#[tokio::test(start_paused = true)]
async fn moderation_reaches_member_after_delayed_fanout() -> Result<()> {
    moderation_reaches_member(Some(LinkFault::Hold)).await
}

/// The override and ban fanout is lost on the Alex<->Barbara link (a
/// partition the sender cannot observe); after the link heals, Alex's
/// home-context journal sync pulls the missed facts.
#[tokio::test(start_paused = true)]
async fn moderation_reaches_member_after_lost_fanout() -> Result<()> {
    moderation_reaches_member(Some(LinkFault::Drop)).await
}

/// Run `action` with the Alex<->Barbara link faulted, then heal it.
async fn faulted<T, Fut: std::future::Future<Output = T>>(
    transport: &SharedTransport,
    alex: AuthorityId,
    barbara: AuthorityId,
    fault: Option<LinkFault>,
    action: Fut,
) -> T {
    if let Some(fault) = fault {
        transport.fault_link(alex, barbara, fault);
    }
    let out = action.await;
    transport.heal_links();
    out
}

async fn moderation_reaches_member(fault: Option<LinkFault>) -> Result<()> {
    let net = SimNet::new();
    let barbara = net.peer(91).await?;
    let alex = net.peer(95).await?;
    link_contacts(&barbara, &alex).await?;

    // Home + home invitation (the `/homeinvite` path) + acceptance. A lost
    // fanout reaches Alex through the runtime-owned periodic sync, driven
    // by virtual time; the test issues no manual sync.
    let home = context::create_home(&barbara.app, Some("BarbHome".to_string()), None).await?;
    join_home(&barbara, &alex, home).await?;
    let alex_home = home_view(&alex.app, home)
        .await
        .ok_or_else(|| anyhow!("Alex home view missing"))?;
    eprintln!(
        "[joined] alex home members={} moderators={:?}",
        alex_home.members.len(),
        alex_home
            .members
            .iter()
            .filter(|m| m.is_moderator())
            .map(|m| m.id)
            .collect::<Vec<_>>()
    );
    messaging::send_message(&alex.app, home, "hello from alex", 1_700_000_000_100).await?;
    messaging::send_message_now_with_instance(&alex.app, home, "second from alex", None).await?;
    // Task 96: Alex reads his own sent home-channel messages as plaintext,
    // authored by himself, and every send appears.
    wait_until("Alex sees both own messages", || async {
        let chat = alex.app.read().await.read(&*CHAT_SIGNAL).await;
        chat.is_ok_and(|c| {
            let own: Vec<_> = c
                .messages_for_channel(&home)
                .iter()
                .filter(|m| m.sender_id == alex.id)
                .collect();
            eprintln!(
                "[own] {:?}",
                own.iter()
                    .map(|m| (m.content.clone(), m.sender_name.clone(), m.is_own))
                    .collect::<Vec<_>>()
            );
            own.len() == 2
                && own.iter().all(|m| {
                    m.is_own
                        && (m.content == "hello from alex" || m.content == "second from alex")
                        && !m.sender_name.starts_with("authority")
                })
        })
    })
    .await?;

    // Limited override (BarbHome is still the selected home): it must reach
    // Alex's view and his own client must refuse his send.
    faulted(
        &net.transport,
        alex.id,
        barbara.id,
        fault,
        access::set_access_override(
            &barbara.app,
            None,
            alex.id,
            aura_social::AccessLevel::Limited,
        ),
    )
    .await?;
    wait_until("Alex sees his Limited override", || async {
        home_view(&alex.app, home)
            .await
            .is_some_and(|h| h.access_override(&alex.id) == Some(aura_social::AccessLevel::Limited))
    })
    .await?;
    let limited_send =
        messaging::send_message(&alex.app, home, "limited send", 1_700_000_000_200).await;
    assert!(
        limited_send
            .as_ref()
            .is_err_and(|e| e.to_string().contains("access level")),
        "Alex's send at Limited must be refused: {limited_send:?}"
    );
    assert_tui_sends_refused(&alex.app, home, "access level").await;
    // The live TUI refusal goes through the typed handoff (Task 120).
    let limited_tui = tui_send(&alex.app, home, "limited tui send").await;
    assert!(
        limited_tui
            .as_ref()
            .is_err_and(|e| e.contains("access level")),
        "Alex's TUI send at Limited must be refused: {limited_tui:?}"
    );

    // Task 120: the same moderator raises Alex to Partial. The later write
    // supersedes the Limited one on both clients, so Alex's send is accepted
    // by his own client and by Barbara's receiver gate.
    faulted(
        &net.transport,
        alex.id,
        barbara.id,
        fault,
        access::set_access_override(
            &barbara.app,
            None,
            alex.id,
            aura_social::AccessLevel::Partial,
        ),
    )
    .await?;
    for (who, app) in [("Alex", &alex.app), ("Barbara", &barbara.app)] {
        wait_until(&format!("{who} sees Alex's Partial override"), || async {
            home_view(app, home).await.is_some_and(|h| {
                h.access_override(&alex.id) == Some(aura_social::AccessLevel::Partial)
            })
        })
        .await?;
    }
    // Send through the TUI submission path (the typed handoff with an id
    // target), which must succeed and actually reach Barbara.
    tui_send(&alex.app, home, "partial send")
        .await
        .map_err(|error| anyhow!("Alex's TUI send at Partial failed: {error}"))?;
    wait_until("Barbara receives Alex's Partial send", || async {
        let chat = barbara.app.read().await.read(&*CHAT_SIGNAL).await;
        chat.is_ok_and(|c| {
            c.messages_for_channel(&home)
                .iter()
                .any(|m| m.sender_id == alex.id && m.content == "partial send")
        })
    })
    .await?;

    // Barbara owns a second home (as in the live run) that is now the
    // selected home, while she types `/ban` in BarbHome's channel.
    let other = context::create_home(&barbara.app, Some("OtherHome".to_string()), None).await?;
    let selected = barbara
        .app
        .read()
        .await
        .read(&*HOMES_SIGNAL)
        .await?
        .current_home_id()
        .copied();
    assert_eq!(
        selected,
        Some(other),
        "the new home becomes the selected home"
    );

    let ban = faulted(
        &net.transport,
        alex.id,
        barbara.id,
        fault,
        strong(
            &barbara.app,
            barbara.id,
            home,
            sc::ParsedCommand::Ban {
                target: alex.id.to_string(),
                reason: Some("spam".to_string()),
            },
        ),
    )
    .await?;
    assert!(
        matches!(
            ban.completion_outcome,
            sc::CommandCompletionOutcome::Satisfied(_)
        ),
        "ban barrier must be satisfied: {:?}",
        ban.completion_outcome
    );
    let barbara_other = home_view(&barbara.app, other)
        .await
        .ok_or_else(|| anyhow!("Barbara other home missing"))?;
    assert!(
        !barbara_other.ban_list.contains_key(&alex.id),
        "the ban targets the planned home, not the selected one"
    );
    wait_until("Alex sees his ban", || async {
        home_view(&alex.app, home)
            .await
            .is_some_and(|h| h.ban_list.contains_key(&alex.id))
    })
    .await?;
    let banned_send =
        messaging::send_message(&alex.app, home, "banned send", 1_700_000_000_300).await;
    assert!(
        banned_send
            .as_ref()
            .is_err_and(|e| e.to_string().contains("banned")),
        "Alex's send after the ban must be refused: {banned_send:?}"
    );
    assert_tui_sends_refused(&alex.app, home, "banned").await;
    let banned_tui = tui_send(&alex.app, home, "banned tui send").await;
    assert!(
        banned_tui.as_ref().is_err_and(|e| e.contains("banned")),
        "Alex's TUI send after the ban must be refused: {banned_tui:?}"
    );
    // Task 126: no refusal reason inserts a message into the sender's chat.
    let chat = alex.app.read().await.read(&*CHAT_SIGNAL).await?;
    let own: Vec<_> = chat
        .messages_for_channel(&home)
        .iter()
        .filter(|m| m.sender_id == alex.id)
        .map(|m| m.content.clone())
        .collect();
    for refused in [
        "limited send",
        "limited tui send",
        "banned send",
        "banned tui send",
        "tui send by id",
        "tui send by name",
    ] {
        assert!(
            !own.iter().any(|content| content == refused),
            "refused send {refused:?} must not appear in Alex's chat: {own:?}"
        );
    }
    net.finish().await
}

/// Send exactly as the TUI does: the typed handoff with an id target.
async fn tui_send(
    app: &Arc<RwLock<AppCore>>,
    home: ChannelId,
    content: &str,
) -> std::result::Result<String, String> {
    messaging::handoff::send_chat_message(
        app,
        messaging::handoff::SendChatMessageRequest {
            target: messaging::handoff::SendChatTarget::ChannelId(home),
            content: content.to_string(),
            operation_instance_id: None,
        },
    )
    .await
    .result
    .map_err(|error| error.to_string())
}

/// The TUI submits through the `*_now_with_instance` APIs (by channel id, or
/// by name when the input does not parse as an id). Both must hit the same
/// sender gate as `send_message`.
async fn assert_tui_sends_refused(app: &Arc<RwLock<AppCore>>, home: ChannelId, reason: &str) {
    let by_id = messaging::send_message_now_with_instance(app, home, "tui send by id", None).await;
    assert!(
        by_id
            .as_ref()
            .is_err_and(|e| e.to_string().contains(reason)),
        "TUI send by id must be refused ({reason}): {by_id:?}"
    );
    // By name, using the name Alex's own chat view shows for the channel.
    let name = app
        .read()
        .await
        .read(&*CHAT_SIGNAL)
        .await
        .ok()
        .and_then(|c| c.channel(&home).map(|ch| ch.name.clone()))
        .unwrap_or_default();
    eprintln!("[by-name] alex channel name for home: {name:?}");
    let by_name =
        messaging::send_message_by_name_now_with_instance(app, &name, "tui send by name", None)
            .await;
    assert!(
        by_name
            .as_ref()
            .is_err_and(|e| e.to_string().contains(reason)),
        "TUI send by name must be refused ({reason}): {by_name:?}"
    );
}

/// Task 118: contacts and home governance reduce from fact sets, so a
/// restarted runtime must seed both views from its whole committed fact set
/// before later facts (a rename, an unban) reduce against them.
#[tokio::test(start_paused = true)]
async fn contact_and_governance_views_survive_restart() -> Result<()> {
    let net = SimNet::new();
    let barbara = net.peer(91).await?;
    let alex = net.peer(95).await?;
    link_contacts(&barbara, &alex).await?;
    let home = context::create_home(&barbara.app, Some("BarbHome".to_string()), None).await?;
    join_home(&barbara, &alex, home).await?;
    access::set_access_override(
        &barbara.app,
        None,
        alex.id,
        aura_social::AccessLevel::Limited,
    )
    .await?;
    strong(
        &barbara.app,
        barbara.id,
        home,
        sc::ParsedCommand::Ban {
            target: alex.id.to_string(),
            reason: Some("spam".to_string()),
        },
    )
    .await?;
    let limited_and_banned = |h: &aura_app::views::home::HomeState| {
        h.ban_list.contains_key(&alex.id)
            && h.access_override(&alex.id) == Some(aura_social::AccessLevel::Limited)
    };
    wait_until("Barbara sees the override and the ban", || async {
        home_view(&barbara.app, home)
            .await
            .is_some_and(|h| limited_and_banned(&h))
    })
    .await?;

    let barbara = barbara.restart(&net).await?;
    wait_until(
        "restarted Barbara still has Alex and the governance",
        || async {
            support::is_contact(&barbara.app, alex.id).await
                && home_view(&barbara.app, home)
                    .await
                    .is_some_and(|h| limited_and_banned(&h))
        },
    )
    .await?;

    contacts::update_contact_nickname(
        &barbara.app,
        &alex.id.to_string(),
        "Alexander",
        1_700_000_001_000,
    )
    .await?;
    strong(
        &barbara.app,
        barbara.id,
        home,
        sc::ParsedCommand::Unban {
            target: alex.id.to_string(),
        },
    )
    .await?;
    wait_until(
        "rename and unban reduce against the seeded views",
        || async {
            let renamed = barbara
                .app
                .read()
                .await
                .read(&*CONTACTS_SIGNAL)
                .await
                .is_ok_and(|c| {
                    c.all_contacts()
                        .any(|c| c.id == alex.id && c.nickname == "Alexander")
                });
            renamed
                && home_view(&barbara.app, home).await.is_some_and(|h| {
                    !h.ban_list.contains_key(&alex.id)
                        && h.access_override(&alex.id) == Some(aura_social::AccessLevel::Limited)
                })
        },
    )
    .await?;
    net.finish().await
}

/// Task 93: the inviter and the invitee each commit Alex's join under the
/// accepted invitation, so both copies are one membership episode. Barbara's
/// kick revokes that episode, which removes Alex on both clients. (A rejoin
/// cannot be driven here: the kick's AMP channel departure makes the inviter
/// refuse a later acceptance; the rejoin orders are covered by the
/// `views::home::governance` permutation tests.)
#[tokio::test(start_paused = true)]
async fn kick_ends_the_shared_membership_episode_on_both_clients() -> Result<()> {
    let net = SimNet::new();
    let barbara = net.peer(91).await?;
    let alex = net.peer(95).await?;
    link_contacts(&barbara, &alex).await?;
    let home = context::create_home(&barbara.app, Some("BarbHome".to_string()), None).await?;
    join_home(&barbara, &alex, home).await?;
    for (who, app) in [("Barbara", &barbara.app), ("Alex", &alex.app)] {
        wait_until(&format!("{who} lists Alex as a member"), || async {
            home_view(app, home)
                .await
                .is_some_and(|h| h.member(&alex.id).is_some())
        })
        .await?;
    }

    let kick = strong(
        &barbara.app,
        barbara.id,
        home,
        sc::ParsedCommand::Kick {
            target: alex.id.to_string(),
            reason: Some("cool off".to_string()),
        },
    )
    .await?;
    assert!(
        matches!(
            kick.completion_outcome,
            sc::CommandCompletionOutcome::Satisfied(_)
        ),
        "kick must observe the member's removal: {:?}",
        kick.completion_outcome
    );
    for (who, app) in [("Barbara", &barbara.app), ("Alex", &alex.app)] {
        wait_until(&format!("{who} drops kicked Alex"), || async {
            home_view(app, home)
                .await
                .is_some_and(|h| h.member(&alex.id).is_none() && h.kick_log.len() == 1)
        })
        .await?;
    }

    // Task 128: a fresh home invitation starts a new membership episode, so
    // the kicked member rejoins the home and its channel.
    join_home(&barbara, &alex, home).await?;
    for (who, app) in [("Barbara", &barbara.app), ("Alex", &alex.app)] {
        wait_until(&format!("{who} lists rejoined Alex"), || async {
            home_view(app, home)
                .await
                .is_some_and(|h| h.member(&alex.id).is_some())
        })
        .await?;
    }
    for (from, to, text) in [
        (&alex, &barbara, "back from alex"),
        (&barbara, &alex, "welcome back"),
    ] {
        messaging::send_message_now_with_instance(&from.app, home, text, None).await?;
        wait_until(&format!("{text:?} is received"), || async {
            let chat = to.app.read().await.read(&*CHAT_SIGNAL).await;
            chat.is_ok_and(|c| {
                c.messages_for_channel(&home)
                    .iter()
                    .any(|m| m.sender_id == from.id && m.content == text)
            })
        })
        .await?;
    }
    net.finish().await
}
