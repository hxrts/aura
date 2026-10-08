//! Home membership beyond the first invitation.
//!
//! - Late joiner (Task 164): Barbara creates a home, invites Alex, and once
//!   the channel has traffic she invites Carol. Carol reads only what is
//!   sent after she joins (the channel rekeys among its members); a kicked
//!   member reads nothing sent after the kick.
//! - Participant to member (Task 62): `/admit` turns a participant into a
//!   member, who can then be designated moderator.

#![allow(missing_docs)]

use crate::support;

use anyhow::Result;
use aura_app::ui::signals::CHAT_SIGNAL;
use aura_app::ui::workflows::{context, messaging, strong_command as sc};
use aura_app::views::home::HomeRole;
use aura_core::effects::reactive::ReactiveEffects;
use support::{home_view, join_home, link_contacts, strong, wait_until, Peer, SimNet};

async fn received(
    to: &Peer,
    home: aura_core::types::identifiers::ChannelId,
    from: &Peer,
    text: &str,
) -> bool {
    let chat = to.app.read().await.read(&*CHAT_SIGNAL).await;
    chat.is_ok_and(|c| {
        c.messages_for_channel(&home)
            .iter()
            .any(|m| m.sender_id == from.id && m.content == text)
    })
}

/// Wait until every one of `viewers` lists `member` in `home`.
async fn wait_lists_member(
    viewers: &[&Peer],
    home: aura_core::types::identifiers::ChannelId,
    member: &Peer,
) -> Result<()> {
    for viewer in viewers {
        wait_until(
            &format!("{} lists {} as a home member", viewer.id, member.id),
            || async {
                home_view(&viewer.app, home)
                    .await
                    .is_some_and(|h| h.member(&member.id).is_some())
            },
        )
        .await?;
    }
    Ok(())
}

/// The home channel's reduced AMP epoch on `peer`.
async fn channel_epoch(peer: &Peer, home: aura_core::types::identifiers::ChannelId) -> Option<u64> {
    let context = home_view(&peer.app, home).await?.context_id?;
    aura_protocol::amp::get_channel_state(&peer.agent.runtime().effects(), context, home)
        .await
        .ok()
        .map(|state| state.chan_epoch)
}

/// Wait until every one of `peers` has moved the home channel to `epoch`.
async fn wait_epoch(
    peers: &[&Peer],
    home: aura_core::types::identifiers::ChannelId,
    epoch: u64,
) -> Result<()> {
    for peer in peers {
        wait_until(
            &format!("{} reaches channel epoch {epoch}", peer.id),
            || async { channel_epoch(peer, home).await.is_some_and(|e| e >= epoch) },
        )
        .await?;
    }
    Ok(())
}

/// Send `text` from `from` and wait until each of `to` reads it.
async fn exchange(
    home: aura_core::types::identifiers::ChannelId,
    from: &Peer,
    text: &str,
    to: &[&Peer],
) -> Result<()> {
    messaging::send_message_now_with_instance(&from.app, home, text, None).await?;
    for peer in to {
        wait_until(&format!("{text:?} reaches {}", peer.id), || {
            received(peer, home, from, text)
        })
        .await?;
    }
    Ok(())
}

// Task 164: a late joiner reads only what is sent after it joins. Carol's
// join moves the home channel to a new epoch keyed by a ceremony among the
// members (Carol never receives the epoch-0 key); she reads post-join
// messages but not the earlier one, while Barbara and Alex keep their
// history. A kicked member is left out of the next epoch's key and reads
// nothing sent after the kick.
#[tokio::test(start_paused = true)]
async fn late_joiner_reads_only_post_join_messages_and_kick_rekeys() -> Result<()> {
    let net = SimNet::new();
    let barbara = net.peer(111).await?;
    let alex = net.peer(115).await?;
    let carol = net.peer(119).await?;
    link_contacts(&barbara, &alex).await?;
    link_contacts(&barbara, &carol).await?;

    let home = context::create_home(&barbara.app, Some("BarbHome".to_string()), None).await?;
    join_home(&barbara, &alex, home).await?;
    exchange(home, &alex, "before carol", &[&barbara]).await?;

    // Carol joins late: the members rekey the channel to epoch 1.
    join_home(&barbara, &carol, home).await?;
    wait_lists_member(&[&barbara, &alex, &carol], home, &carol).await?;
    wait_epoch(&[&barbara, &alex, &carol], home, 1).await?;

    exchange(home, &carol, "hello from carol", &[&barbara, &alex]).await?;
    exchange(home, &barbara, "welcome carol", &[&alex, &carol]).await?;
    exchange(home, &alex, "hi carol", &[&barbara, &carol]).await?;
    assert!(
        !received(&carol, home, &alex, "before carol").await,
        "a late joiner cannot read messages from before it joined"
    );
    for member in [&barbara, &alex] {
        assert!(
            received(member, home, &alex, "before carol").await || member.id == alex.id,
            "existing members keep their history"
        );
    }

    // Barbara kicks Alex: Barbara and Carol rekey to epoch 2 without him.
    aura_app::ui::workflows::moderation::kick_user_resolved(
        &barbara.app,
        home,
        alex.id,
        None,
        10_000,
    )
    .await?;
    wait_epoch(&[&barbara, &carol], home, 2).await?;
    exchange(home, &barbara, "after the kick", &[&carol]).await?;
    for _ in 0..20 {
        support::quiesce().await;
    }
    assert!(
        !received(&alex, home, &barbara, "after the kick").await,
        "a kicked member cannot read messages sent after the kick"
    );
    net.finish().await
}

// Task 11: moderator commands typed in the home channel succeed for its
// moderator and are refused for a participant: /deop, /pin, /unpin, /mode.
#[tokio::test(start_paused = true)]
async fn moderator_commands_are_allowed_for_moderators_and_refused_for_participants() -> Result<()>
{
    let net = SimNet::new();
    let barbara = net.peer(131).await?;
    let alex = net.peer(135).await?;
    link_contacts(&barbara, &alex).await?;
    let home = context::create_home(&barbara.app, Some("ModHome".to_string()), None).await?;
    join_home(&barbara, &alex, home).await?;
    wait_lists_member(&[&barbara, &alex], home, &alex).await?;

    messaging::send_message_now_with_instance(&barbara.app, home, "pin me", None).await?;
    wait_until("Alex receives the message to pin", || {
        received(&alex, home, &barbara, "pin me")
    })
    .await?;
    let message_id = barbara
        .app
        .read()
        .await
        .read(&*CHAT_SIGNAL)
        .await?
        .messages_for_channel(&home)
        .iter()
        .find(|m| m.content == "pin me")
        .map(|m| m.id.clone())
        .ok_or_else(|| anyhow::anyhow!("pinned message missing"))?;

    let commands = |target: &Peer| {
        vec![
            sc::ParsedCommand::Pin {
                message_id: message_id.clone(),
            },
            sc::ParsedCommand::Unpin {
                message_id: message_id.clone(),
            },
            sc::ParsedCommand::Mode {
                channel: "ModHome".to_string(),
                flags: "+i".to_string(),
            },
            sc::ParsedCommand::Deop {
                target: target.id.to_string(),
            },
        ]
    };
    for command in commands(&barbara) {
        let label = format!("{command:?}");
        assert!(
            strong(&alex.app, alex.id, home, command).await.is_err(),
            "a participant's {label} must be refused"
        );
    }

    strong(
        &barbara.app,
        barbara.id,
        home,
        sc::ParsedCommand::Admit {
            target: alex.id.to_string(),
        },
    )
    .await?;
    strong(
        &barbara.app,
        barbara.id,
        home,
        sc::ParsedCommand::Op {
            target: alex.id.to_string(),
        },
    )
    .await?;
    for command in commands(&alex) {
        let label = format!("{command:?}");
        strong(&barbara.app, barbara.id, home, command)
            .await
            .map_err(|error| anyhow::anyhow!("moderator {label} failed: {error}"))?;
    }
    wait_until("Alex is a member again after /deop", || async {
        home_view(&alex.app, home)
            .await
            .is_some_and(|h| role_of(&h, alex.id) == Some(HomeRole::Member))
    })
    .await?;
    net.finish().await
}

fn role_of(
    home: &aura_app::views::home::HomeState,
    who: aura_core::types::identifiers::AuthorityId,
) -> Option<HomeRole> {
    home.member(&who).map(|member| member.role)
}

// Task 62: a joined participant cannot be designated moderator until a
// moderator admits them as a member; after `/admit` every client lists them
// as a member, `/op` then designates them, and a participant cannot admit.
#[tokio::test(start_paused = true)]
async fn admitted_participant_becomes_member_then_moderator() -> Result<()> {
    let net = SimNet::new();
    let barbara = net.peer(121).await?;
    let alex = net.peer(125).await?;
    let carol = net.peer(129).await?;
    link_contacts(&barbara, &alex).await?;
    link_contacts(&barbara, &carol).await?;
    link_contacts(&alex, &carol).await?;
    let home = context::create_home(&barbara.app, Some("BarbHome".to_string()), None).await?;
    join_home(&barbara, &alex, home).await?;
    join_home(&barbara, &carol, home).await?;
    wait_lists_member(&[&barbara, &alex, &carol], home, &alex).await?;
    wait_lists_member(&[&barbara, &alex, &carol], home, &carol).await?;

    let op = |target: &Peer| sc::ParsedCommand::Op {
        target: target.id.to_string(),
    };
    let admit = |target: &Peer| sc::ParsedCommand::Admit {
        target: target.id.to_string(),
    };
    assert!(
        strong(&barbara.app, barbara.id, home, op(&alex))
            .await
            .is_err(),
        "a participant cannot be designated moderator"
    );
    assert!(
        strong(&alex.app, alex.id, home, admit(&carol))
            .await
            .is_err(),
        "a participant cannot admit members"
    );

    strong(&barbara.app, barbara.id, home, admit(&alex)).await?;
    for (who, peer) in [("Barbara", &barbara), ("Alex", &alex), ("Carol", &carol)] {
        wait_until(&format!("{who} lists Alex as a member"), || async {
            home_view(&peer.app, home)
                .await
                .is_some_and(|h| role_of(&h, alex.id) == Some(HomeRole::Member))
        })
        .await?;
    }
    assert_eq!(
        home_view(&alex.app, home).await.map(|h| h.my_role),
        Some(HomeRole::Member)
    );

    strong(&barbara.app, barbara.id, home, op(&alex)).await?;
    for (who, peer) in [("Barbara", &barbara), ("Alex", &alex)] {
        wait_until(&format!("{who} lists Alex as a moderator"), || async {
            home_view(&peer.app, home)
                .await
                .is_some_and(|h| role_of(&h, alex.id) == Some(HomeRole::Moderator))
        })
        .await?;
    }
    net.finish().await
}

// Task 47: `/invite` into a channel that already has members reaches a
// successful terminal outcome, and the invitee joins and reads the channel.
#[tokio::test(start_paused = true)]
async fn invite_command_into_existing_channel_succeeds() -> Result<()> {
    let net = SimNet::new();
    let barbara = net.peer(141).await?;
    let alex = net.peer(145).await?;
    let carol = net.peer(149).await?;
    link_contacts(&barbara, &alex).await?;
    link_contacts(&barbara, &carol).await?;
    let home = context::create_home(&barbara.app, Some("InvHome".to_string()), None).await?;
    join_home(&barbara, &alex, home).await?;

    strong(
        &barbara.app,
        barbara.id,
        home,
        sc::ParsedCommand::Invite {
            target: carol.id.to_string(),
        },
    )
    .await
    .map_err(|error| anyhow::anyhow!("/invite failed: {error}"))?;
    wait_until("Carol accepts the channel invitation", || async {
        aura_app::ui::workflows::invitation::accept_pending_channel_invitation(&carol.app)
            .await
            .is_ok()
    })
    .await?;
    // Carol reads only what is sent after her join's key epoch (Task 164).
    wait_epoch(&[&barbara, &carol], home, 1).await?;
    messaging::send_message_now_with_instance(&barbara.app, home, "hi invitee", None).await?;
    wait_until("Carol reads the channel", || {
        received(&carol, home, &barbara, "hi invitee")
    })
    .await?;
    net.finish().await
}
