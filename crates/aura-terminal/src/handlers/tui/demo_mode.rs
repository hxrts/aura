#[cfg(feature = "development")]
use std::collections::HashMap;
use std::sync::Arc;

use async_lock::RwLock;
use aura_app::ui::prelude::*;

#[cfg(feature = "development")]
use crate::demo::DemoSimulator;

#[cfg(feature = "development")]
pub(super) async fn seed_realistic_demo_world(
    app_core: &Arc<RwLock<AppCore>>,
    bob_agent: &Arc<aura_agent::AuraAgent>,
    simulator: &DemoSimulator,
) -> crate::error::TerminalResult<()> {
    use aura_app::ui::signals::{
        HOMES_SIGNAL, HOMES_SIGNAL_NAME, NEIGHBORHOOD_SIGNAL, NEIGHBORHOOD_SIGNAL_NAME,
    };
    use aura_app::ui::types::{HomeMember, HomeRole, NeighborHome, OneHopLinkType};
    use aura_app::ui::workflows::context::{
        add_home_to_neighborhood, create_home, create_neighborhood,
    };
    use aura_app::ui::workflows::signals::{emit_signal, read_signal_or_default};
    use aura_core::effects::time::PhysicalTimeEffects;

    let prelinked_contacts = ["Dave", "Grace", "Judy", "Olivia", "Peggy", "Sybil"];
    let home_specs: Vec<(
        &'static str,
        &'static str,
        Vec<&'static str>,
        OneHopLinkType,
        u32,
    )> = vec![
        (
            "Northside",
            "Maple House",
            vec!["Dave", "Eve", "Frank"],
            OneHopLinkType::Direct,
            6,
        ),
        (
            "Northside",
            "Cedar House",
            vec!["Grace", "Heidi", "Ivan"],
            OneHopLinkType::Direct,
            5,
        ),
        (
            "Riverside",
            "Harbor House",
            vec!["Judy", "Mallory"],
            OneHopLinkType::TwoHop,
            4,
        ),
        (
            "Riverside",
            "Foundry House",
            vec!["Niaj", "Olivia"],
            OneHopLinkType::TwoHop,
            4,
        ),
        (
            "Hillside",
            "Orchard House",
            vec!["Peggy", "Rupert"],
            OneHopLinkType::Distant,
            3,
        ),
        (
            "Hillside",
            "Lantern House",
            vec!["Sybil", "Dave"],
            OneHopLinkType::Distant,
            3,
        ),
    ];

    let mut peer_authorities = HashMap::new();
    for profile in simulator.social_peer_profiles() {
        peer_authorities.insert(profile.name, profile.authority_id);
    }
    peer_authorities.insert("Alice".to_string(), simulator.alice_authority());
    peer_authorities.insert("Carol".to_string(), simulator.carol_authority());

    let now_ms = bob_agent
        .runtime()
        .effects()
        .physical_time()
        .await
        .map(|time| time.ts_ms)
        .unwrap_or(0);
    let contacts_to_add: Vec<(String, &str, u64)> = prelinked_contacts
        .iter()
        .enumerate()
        .filter_map(|(idx, name)| {
            peer_authorities
                .get(*name)
                .map(|peer_id| (peer_id.to_string(), *name, now_ms + idx as u64))
        })
        .collect();

    if !contacts_to_add.is_empty() {
        let contacts_refs: Vec<(&str, &str, u64)> = contacts_to_add
            .iter()
            .map(|(id, name, ts)| (id.as_str(), *name, *ts))
            .collect();
        aura_app::ui::workflows::contacts::add_contacts_batch(app_core, &contacts_refs).await?;
    }

    let _ = create_neighborhood(app_core, "Tri-Neighborhood Demo".to_string()).await?;

    let mut created_homes = Vec::with_capacity(home_specs.len());
    for (cluster, home_name, members, hop, shared_contacts) in home_specs {
        let display_name = format!("{cluster} · {home_name}");
        let home_id = create_home(
            app_core,
            Some(display_name.clone()),
            Some(format!("Demo home in the {cluster} cluster")),
        )
        .await?;
        add_home_to_neighborhood(app_core, &home_id.to_string()).await?;
        created_homes.push((home_id, display_name, members, hop, shared_contacts));
    }

    let mut homes_state = read_signal_or_default(app_core, &*HOMES_SIGNAL).await;
    let mut neighborhood_state = read_signal_or_default(app_core, &*NEIGHBORHOOD_SIGNAL).await;

    for (home_id, display_name, members, hop, shared_contacts) in &created_homes {
        if let Some(home) = homes_state.home_mut(home_id) {
            for (idx, member_name) in members.iter().enumerate() {
                let Some(member_id) = peer_authorities.get(*member_name) else {
                    continue;
                };
                if home.members.iter().any(|member| member.id == *member_id) {
                    continue;
                }

                let role = if idx == 0 {
                    HomeRole::Member
                } else {
                    HomeRole::Participant
                };
                home.add_member(HomeMember {
                    id: *member_id,
                    name: (*member_name).to_string(),
                    role,
                    is_online: true,
                    joined_at: now_ms + idx as u64 + 1,
                    last_seen: Some(now_ms + idx as u64 + 1),
                    storage_allocated: aura_app::ui::types::MEMBER_ALLOCATION,
                });
            }

            if *home_id != neighborhood_state.home_home_id {
                neighborhood_state.add_neighbor(NeighborHome {
                    id: *home_id,
                    name: display_name.clone(),
                    one_hop_link: *hop,
                    shared_contacts: *shared_contacts,
                    member_count: Some(home.member_count),
                    can_traverse: true,
                });
            }
        }
    }

    neighborhood_state.neighborhood_id = Some(format!("demo-topology-{}", simulator.seed()));
    neighborhood_state.neighborhood_name = Some("Tri-Neighborhood Demo".to_string());
    neighborhood_state.set_member_homes(created_homes.iter().map(|(id, ..)| *id));

    emit_signal(app_core, &*HOMES_SIGNAL, homes_state, HOMES_SIGNAL_NAME).await?;
    emit_signal(
        app_core,
        &*NEIGHBORHOOD_SIGNAL,
        neighborhood_state,
        NEIGHBORHOOD_SIGNAL_NAME,
    )
    .await?;

    aura_app::ui::workflows::system::refresh_account(app_core).await?;
    Ok(())
}
