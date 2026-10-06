//! Task 86: after a home invitation is accepted, the inviter's readiness
//! refresh hooks must stay alive; a failed participant lookup for one update
//! may not end the long-lived attachment.

#![allow(missing_docs)]

mod support;

use anyhow::Result;
use aura_app::ui::workflows::context;
use support::{home_view, join_home, link_contacts, wait_until, SimNet};

#[tokio::test(start_paused = true)]
async fn inviter_readiness_hooks_survive_home_invitation_acceptance() -> Result<()> {
    let net = SimNet::new();
    let barbara = net.peer(101).await?;
    let alex = net.peer(105).await?;
    link_contacts(&barbara, &alex).await?;

    let home = context::create_home(&barbara.app, Some("BarbHome".to_string()), None).await?;
    join_home(&barbara, &alex, home).await?;

    // The acceptance reaches Barbara and drives her readiness hooks; every
    // refresh it triggers has run once both runtimes are quiescent.
    wait_until("Barbara lists Alex as a home member", || async {
        home_view(&barbara.app, home)
            .await
            .is_some_and(|h| h.members.iter().any(|m| m.id == alex.id))
    })
    .await?;
    // Task 88: shared-transport simulation runs the production sync service,
    // so no refresh hook may die and no refresh update may fail (e.g. a
    // missing sync status).
    net.finish().await
}

/// Task 115: a wait aborts as soon as a runtime records a dead supervised
/// task, carrying its cause, instead of running out its virtual-time bound.
#[tokio::test(start_paused = true)]
async fn wait_fails_fast_on_a_dead_supervised_task() -> Result<()> {
    let net = SimNet::new();
    let peer = net.testing_peer(111).await?;
    let _dead = peer
        .agent
        .runtime()
        .tasks()
        .spawn_try_named("injected", async {
            Err(aura_core::AuraError::internal(
                "injected schema decode failure",
            ))
        });
    let start = tokio::time::Instant::now();
    let error = wait_until("a condition that never holds", || async { false })
        .await
        .expect_err("the wait must fail on the dead task");
    let message = format!("{error:#}");
    assert!(message.contains("injected"), "{message}");
    assert!(
        message.contains("injected schema decode failure"),
        "{message}"
    );
    assert!(
        start.elapsed() < std::time::Duration::from_secs(1),
        "the wait must abort at once, not after its bound: {:?}",
        start.elapsed()
    );
    assert!(net.finish().await.is_err());
    Ok(())
}
