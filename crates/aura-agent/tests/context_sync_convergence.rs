//! Relational-context sync convergence over a shared in-memory transport.
//!
//! Commit-time sends are single best-effort sends; a send lost to a link
//! partition (`LinkFault::Drop`, unobservable by the sender) must still reach
//! every member once the link heals, through the runtimes' periodic
//! relational-context sync on virtual time. No test issues a manual sync.

#![allow(missing_docs)]

use crate::support;

use anyhow::{anyhow, Result};
use async_lock::RwLock;
use aura_agent::LinkFault;
use aura_app::core::AppCore;
use aura_app::ui::signals::CHAT_SIGNAL;
use aura_app::ui::workflows::{context, messaging};
use aura_app::views::chat::MessageDeliveryStatus;
use aura_core::effects::reactive::ReactiveEffects;
use aura_core::types::identifiers::ChannelId;
use std::sync::Arc;
use support::{
    home_view, join_home, link_contacts, wait_converged, wait_membership_converged, SimNet,
};

async fn message_status(
    app: &Arc<RwLock<AppCore>>,
    home: ChannelId,
    id: &str,
) -> Option<MessageDeliveryStatus> {
    let chat = app.read().await.read(&*CHAT_SIGNAL).await.ok()?;
    chat.messages_for_channel(&home)
        .iter()
        .find(|m| m.id == id)
        .map(|m| m.delivery_status)
}

/// A home-channel message sent while the link to the only other member is
/// partitioned reaches that member after the link heals, and the sender's
/// status advances to Delivered from the recipient's receipt.
#[tokio::test(start_paused = true)]
async fn chat_message_converges_after_dropped_link_heals() -> Result<()> {
    let net = SimNet::new();
    let barbara = net.peer(61).await?;
    let alex = net.peer(65).await?;
    link_contacts(&barbara, &alex).await?;
    let home = context::create_home(&barbara.app, Some("SyncHome".to_string()), None).await?;
    join_home(&barbara, &alex, home).await?;

    net.transport
        .fault_link(alex.id, barbara.id, LinkFault::Drop);
    let id = messaging::send_message(
        &alex.app,
        home,
        "sent across a partition",
        1_700_000_000_500,
    )
    .await?;
    support::quiesce().await;
    let barbara_saw = message_status(&barbara.app, home, &id).await;
    assert!(
        barbara_saw.is_none(),
        "partition must drop the send: {barbara_saw:?}"
    );
    net.transport.heal_links();

    let id = id.as_str();
    wait_converged(
        "both members hold Alex's message",
        &[&alex, &barbara],
        |_, app| async move { message_status(&app, home, id).await.is_some() },
    )
    .await?;
    wait_converged(
        "Alex's message is Delivered",
        &[&alex],
        |_, app| async move {
            message_status(&app, home, id).await == Some(MessageDeliveryStatus::Delivered)
        },
    )
    .await?;
    net.finish().await
}

/// Run 172 (Task 199): Barbara creates a home and invites Alex, then Carol.
/// Barbara's joins written for Alex and Carol reach each member through
/// context sync, possibly in a page before Barbara's own join (pages are
/// digest buckets). A join whose author's standing is not observed yet is
/// deferred to a later round, never logged as a drop, and membership
/// converges on all three clients. Without both the self-joins-first page
/// order and the deferral, Alex logs Barbara's join for him as a drop.
#[tokio::test(start_paused = true)]
async fn joins_written_by_the_inviter_converge_without_standing_drops() -> Result<()> {
    let net = SimNet::new();
    let barbara = net.peer(81).await?;
    let alex = net.peer(85).await?;
    let carol = net.peer(89).await?;
    link_contacts(&barbara, &alex).await?;
    link_contacts(&barbara, &carol).await?;
    let home = context::create_home(&barbara.app, Some("BarbHome".to_string()), None).await?;
    join_home(&barbara, &alex, home).await?;
    join_home(&barbara, &carol, home).await?;
    let context_id = home_view(&barbara.app, home)
        .await
        .and_then(|h| h.context_id)
        .ok_or_else(|| anyhow!("home context missing"))?;
    let all = [&barbara, &alex, &carol];
    let members = [barbara.id, alex.id, carol.id];
    wait_converged(
        "every client lists all three members",
        &all,
        |agent, _| async move {
            aura_protocol::amp::list_channel_participants(
                agent.runtime().effects().as_ref(),
                context_id,
                home,
            )
            .await
            .is_ok_and(|participants| members.iter().all(|m| participants.contains(m)))
        },
    )
    .await?;
    wait_membership_converged(&all, context_id).await?;

    for peer in all {
        let (drops, _) = peer.agent.runtime().effects().message_drops();
        let standing: Vec<_> = drops
            .iter()
            .filter(|drop| {
                drop.reason
                    .to_string()
                    .starts_with("membership_author_without_standing")
            })
            .collect();
        assert!(
            standing.is_empty(),
            "{} logged membership facts as author-without-standing: {standing:?}",
            peer.id
        );
    }
    net.finish().await
}

/// Alex leaves the home channel while the Alex<->Barbara link is
/// partitioned. Leaving commits only Alex's own membership fact; Barbara
/// learns it through context sync once the link heals, so the reduced AMP
/// membership converges on both clients.
#[tokio::test(start_paused = true)]
async fn channel_membership_converges_after_dropped_link_heals() -> Result<()> {
    let net = SimNet::new();
    let barbara = net.peer(71).await?;
    let alex = net.peer(75).await?;
    link_contacts(&barbara, &alex).await?;
    let home = context::create_home(&barbara.app, Some("SyncHome".to_string()), None).await?;
    join_home(&barbara, &alex, home).await?;
    let context_id = home_view(&barbara.app, home)
        .await
        .and_then(|h| h.context_id)
        .ok_or_else(|| anyhow!("home context missing"))?;
    let alex_id = alex.id;
    let alex_listed = move |agent: Arc<aura_agent::AuraAgent>| async move {
        aura_protocol::amp::list_channel_participants(
            agent.runtime().effects().as_ref(),
            context_id,
            home,
        )
        .await
        .is_ok_and(|participants| participants.contains(&alex_id))
    };
    wait_converged("both clients list Alex", &[&barbara, &alex], |agent, _| {
        alex_listed(agent)
    })
    .await?;
    // Alex's departure must observe Barbara's join episode, which only
    // context sync delivers to him.
    wait_membership_converged(&[&barbara, &alex], context_id).await?;

    net.transport
        .fault_link(alex.id, barbara.id, LinkFault::Drop);
    messaging::leave_channel(&alex.app, home).await?;
    support::quiesce().await;
    net.transport.heal_links();

    wait_converged(
        "neither client lists Alex after he left",
        &[&barbara, &alex],
        |agent, _| async move { !alex_listed(agent).await },
    )
    .await?;
    wait_membership_converged(&[&barbara, &alex], context_id).await?;
    net.finish().await
}
