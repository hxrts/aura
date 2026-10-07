//! Human-readable rendering of a [`Response`].

use super::response::{ChannelView, MessageView, OperationView, Response};
use crate::handlers::CliOutput;

fn channel_rows(channels: &[ChannelView]) -> Vec<Vec<String>> {
    channels
        .iter()
        .map(|c| {
            vec![
                c.name.clone(),
                c.channel_id.clone(),
                c.member_count.to_string(),
                c.unread.to_string(),
            ]
        })
        .collect()
}

fn message_rows(messages: &[MessageView]) -> Vec<Vec<String>> {
    messages
        .iter()
        .map(|m| {
            let sender = if m.sender_name.is_empty() {
                m.sender_id.clone()
            } else {
                m.sender_name.clone()
            };
            vec![m.timestamp_ms.to_string(), sender, m.content.clone()]
        })
        .collect()
}

fn operation(out: &mut CliOutput, operation: Option<&OperationView>) {
    if let Some(op) = operation {
        out.kv("Operation", format!("{:?} {:?}", op.kind, op.phase));
    }
}

/// Text output for a response.
#[must_use]
pub fn render(response: &Response) -> CliOutput {
    let mut out = CliOutput::new();
    match response {
        Response::Account(a) => {
            out.section("Account Status");
            out.kv("Authority", &a.authority_id);
            out.kv("Nickname", &a.nickname);
            out.kv(
                "Threshold",
                format!("{} of {}", a.threshold_k, a.threshold_n),
            );
            out.kv("Devices", a.devices.to_string());
            out.kv("Contacts", a.contacts.to_string());
        }
        Response::Authorities(list) => {
            out.section(format!("Authorities ({})", list.len()));
            for a in list {
                let current = if a.current { " (current account)" } else { "" };
                out.println(format!("  - {} {}{current}", a.authority_id, a.nickname));
            }
        }
        Response::Contacts(contacts) => {
            out.section(format!("Contacts ({})", contacts.len()));
            let rows: Vec<Vec<String>> = contacts
                .iter()
                .map(|c| {
                    let mut roles = Vec::new();
                    if c.is_guardian {
                        roles.push("guardian");
                    }
                    if c.is_member {
                        roles.push("member");
                    }
                    vec![c.nickname.clone(), c.authority_id.clone(), roles.join(",")]
                })
                .collect();
            out.table(&["Name", "Authority", "Roles"], &rows);
        }
        Response::Contact(c) => {
            out.section(&c.nickname);
            out.kv("Authority", &c.authority_id);
            out.kv("Guardian", c.is_guardian.to_string());
            out.kv("Home member", c.is_member.to_string());
        }
        Response::Members(members) => {
            out.section(format!("Members ({})", members.len()));
            for member in members {
                out.println(format!("  - {member}"));
            }
        }
        Response::Settings(s) => {
            out.section("Settings");
            out.kv("Nickname", &s.nickname);
            out.kv(
                "Threshold",
                format!("{} of {}", s.threshold_k, s.threshold_n),
            );
            out.kv("MFA policy", &s.mfa_policy);
            out.kv("Contacts", s.contacts.to_string());
            out.kv("Devices", s.devices.len().to_string());
            for device in &s.devices {
                out.println(format!("  - {device}"));
            }
        }
        Response::PeerList(peers) => {
            out.section(format!("Peers ({})", peers.len()));
            for peer in peers {
                out.println(format!("  - {peer}"));
            }
        }
        Response::CeremonyStarted { ceremony_id } => {
            out.kv("Ceremony started", ceremony_id);
        }
        Response::NeighborhoodCreated { neighborhood_id } => {
            out.kv("Created neighborhood", neighborhood_id);
        }
        Response::Budget { summary } => {
            out.section("Home Storage Budget");
            out.println(summary);
        }
        Response::Notifications(items) => {
            out.section(format!("Notifications ({})", items.len()));
            let rows: Vec<Vec<String>> = items
                .iter()
                .map(|n| vec![n.kind.clone(), n.title.clone(), n.id.clone()])
                .collect();
            out.table(&["Kind", "Title", "ID"], &rows);
        }
        Response::Channels(channels) => {
            out.section(format!("Channels ({})", channels.len()));
            out.table(
                &["Name", "ID", "Members", "Unread"],
                &channel_rows(channels),
            );
        }
        Response::Channel(c) => {
            out.section(&c.name);
            out.kv("Channel", &c.channel_id);
            if let Some(context) = &c.context_id {
                out.kv("Context", context);
            }
            if let Some(topic) = &c.topic {
                out.kv("Topic", topic);
            }
            out.kv("Members", c.member_count.to_string());
            for member in &c.members {
                out.println(format!("  - {member}"));
            }
        }
        Response::Messages(messages) => {
            out.section(format!("Messages ({})", messages.len()));
            out.table(&["Time", "Sender", "Message"], &message_rows(messages));
        }
        Response::MessageSent {
            channel_id,
            message_id,
            operation: op,
        } => {
            out.kv("Message sent", message_id);
            out.kv("Channel", channel_id);
            operation(&mut out, op.as_ref());
        }
        Response::ChannelCreated {
            channel_id,
            context_id,
        } => {
            out.kv("Created channel", channel_id);
            out.kv("Context", context_id);
        }
        Response::InvitationCreated {
            invitation_id,
            code,
        } => {
            out.kv("Invitation", invitation_id);
            out.kv("Code", code);
            out.println("Share the code; the recipient runs `aura invite import --code <code>`.");
        }
        Response::Invitations(list) => {
            out.section(format!("Pending invitations ({})", list.len()));
            let rows: Vec<Vec<String>> = list
                .iter()
                .map(|i| {
                    vec![
                        i.invitation_id.clone(),
                        i.kind.clone(),
                        i.sender_id.clone(),
                        i.status.clone(),
                    ]
                })
                .collect();
            out.table(&["ID", "Kind", "From", "Status"], &rows);
        }
        Response::Invitation(i) => {
            out.section("Invitation");
            out.kv("Invitation ID", &i.invitation_id);
            out.kv("Kind", &i.kind);
            out.kv("From", &i.sender_id);
            out.kv("Status", &i.status);
            if let Some(message) = &i.message {
                out.kv("Message", message);
            }
            out.println(format!(
                "Accept with: aura invite accept --invitation-id {}",
                i.invitation_id
            ));
        }
        Response::InvitationCode {
            invitation_id,
            code,
        } => {
            out.kv("Invitation", invitation_id);
            out.kv("Code", code);
        }
        Response::Export { body, .. } => {
            out.println(body);
        }
        Response::HomeCreated { home_id } => {
            out.kv("Created home", home_id);
        }
        Response::RecoveryStarted { ceremony_id } => {
            out.kv("Recovery ceremony", ceremony_id);
        }
        Response::Recovery(r) => {
            out.section("Recovery");
            out.kv("Threshold", r.threshold.to_string());
            out.kv("Guardians", r.guardians.len().to_string());
            for guardian in &r.guardians {
                out.println(format!("  - {guardian}"));
            }
            match (&r.active_ceremony, &r.approvals) {
                (Some(ceremony), approvals) => {
                    out.kv("Active ceremony", ceremony);
                    if let Some(approvals) = approvals {
                        out.kv("Approvals", approvals);
                    }
                }
                (None, _) => {
                    out.kv("Active ceremony", "none");
                }
            }
        }
        Response::Sync { status, detail } => {
            out.kv("Sync", status);
            if let Some(detail) = detail {
                out.kv("Detail", detail);
            }
        }
        Response::Peers { connected } => {
            out.kv("Connected peers", connected.to_string());
        }
        Response::AmpChannel(a) => {
            out.section(format!(
                "Channel State for {}:{}",
                a.context_id, a.channel_id
            ));
            out.kv("Current Epoch", a.epoch.to_string());
            out.kv("Current Generation", a.generation.to_string());
            out.kv(
                "Last Checkpoint Gen",
                a.last_checkpoint_generation.to_string(),
            );
            out.kv("Skip Window", a.skip_window.to_string());
            out.kv(
                "Pending Bump",
                a.pending_bump.clone().unwrap_or_else(|| "none".to_string()),
            );
        }
        Response::AmpBumpProposed {
            parent_epoch,
            new_epoch,
            bump_id,
        } => {
            out.kv(
                "Proposed epoch bump",
                format!("{parent_epoch} -> {new_epoch}"),
            );
            out.kv("Bump ID", bump_id);
        }
        Response::AmpCheckpoint {
            epoch,
            base_generation,
        } => {
            out.kv("Checkpoint epoch", epoch.to_string());
            out.kv("Base generation", base_generation.to_string());
        }
        Response::SnapshotProposed { proposal_id } => {
            out.kv("Snapshot proposal recorded with key", proposal_id);
        }
        Response::Done {
            summary,
            operation: op,
        } => {
            out.println(summary);
            operation(&mut out, op.as_ref());
        }
    }
    out
}
