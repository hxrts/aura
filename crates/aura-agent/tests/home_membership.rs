//! Home membership beyond the first invitation.
//!
//! - Late joiner (work/8.md Tasks 31/55): Barbara creates a home, invites
//!   Alex, and once the channel has traffic she invites Carol. The home's AMP
//!   channel already has its epoch-0 bootstrap; Carol must still join, read
//!   and send, and all three must see each other's messages.

#![allow(missing_docs)]

use crate::support;

use anyhow::Result;
use aura_app::ui::signals::CHAT_SIGNAL;
use aura_app::ui::workflows::{context, messaging};
use aura_core::effects::reactive::ReactiveEffects;
use support::{home_view, join_home, link_contacts, wait_until, Peer, SimNet};

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

#[tokio::test(start_paused = true)]
async fn late_joiner_joins_home_and_exchanges_messages() -> Result<()> {
    let net = SimNet::new();
    let barbara = net.peer(111).await?;
    let alex = net.peer(115).await?;
    let carol = net.peer(119).await?;
    link_contacts(&barbara, &alex).await?;
    link_contacts(&barbara, &carol).await?;

    let home = context::create_home(&barbara.app, Some("BarbHome".to_string()), None).await?;
    join_home(&barbara, &alex, home).await?;
    messaging::send_message_now_with_instance(&alex.app, home, "before carol", None).await?;
    wait_until("Barbara receives Alex's first message", || {
        received(&barbara, home, &alex, "before carol")
    })
    .await?;

    // The channel's bootstrap already exists: Carol joins late.
    join_home(&barbara, &carol, home).await?;
    wait_lists_member(&[&barbara, &alex, &carol], home, &carol).await?;

    for (from, text) in [
        (&carol, "hello from carol"),
        (&barbara, "welcome carol"),
        (&alex, "hi carol"),
    ] {
        messaging::send_message_now_with_instance(&from.app, home, text, None).await?;
        for to in [&barbara, &alex, &carol] {
            if to.id == from.id {
                continue;
            }
            wait_until(&format!("{text:?} reaches {}", to.id), || {
                received(to, home, from, text)
            })
            .await?;
        }
    }
    net.finish().await
}
