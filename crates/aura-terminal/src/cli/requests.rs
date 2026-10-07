//! Parsers for the workflow-backed commands other than `chat`: each builds
//! a [`Request`] and nothing else.

use crate::command::{InviteRole, Request};
use bpaf::{construct, long, positional, pure, Parser};

fn comma_list(raw: String) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

#[must_use]
pub fn status_parser() -> impl Parser<Request> {
    pure(Request::Status)
}

#[must_use]
pub fn authority_parser() -> impl Parser<Request> {
    pure(Request::AuthorityList)
        .to_options()
        .command("list")
        .help("List authorities known to this runtime")
}

#[must_use]
pub fn contact_parser() -> impl Parser<Request> {
    pure(Request::ContactList)
        .to_options()
        .command("list")
        .help("List contacts")
}

#[must_use]
pub fn context_parser() -> impl Parser<Request> {
    let context = long("context")
        .help("Context identifier")
        .argument::<String>("CONTEXT");
    construct!(Request::ContextInspect { context })
        .to_options()
        .command("inspect")
        .help("List the channels bound to a relational context")
}

fn context_channel() -> impl Parser<(String, String)> {
    let context = long("context")
        .help("Context identifier")
        .argument::<String>("CONTEXT");
    let channel = long("channel")
        .help("Channel identifier")
        .argument::<String>("CHANNEL");
    construct!(context, channel)
}

#[must_use]
pub fn amp_parser() -> impl Parser<Request> {
    let inspect = context_channel()
        .map(|(context, channel)| Request::AmpInspect { context, channel })
        .to_options()
        .command("inspect")
        .help("Show channel epoch/windows for a context/channel");
    let bump = context_channel()
        .map(|(context, channel)| Request::AmpBump { context, channel })
        .to_options()
        .command("bump")
        .help("Propose a routine epoch bump");
    let checkpoint = context_channel()
        .map(|(context, channel)| Request::AmpCheckpoint { context, channel })
        .to_options()
        .command("checkpoint")
        .help("Emit a checkpoint at the current generation");
    construct!([inspect, bump, checkpoint])
}

fn invitation_id() -> impl Parser<String> {
    long("invitation-id")
        .help("Invitation identifier")
        .argument::<String>("INVITATION_ID")
}

#[must_use]
pub fn invite_parser() -> impl Parser<Request> {
    let create = {
        let invitee = long("invitee")
            .help("Authority to invite")
            .argument::<String>("AUTHORITY");
        let role = long("role")
            .help("contact, guardian or channel (default: contact)")
            .argument::<InviteRole>("ROLE")
            .fallback(InviteRole::Contact);
        let channel = long("channel")
            .help("Channel for a channel invitation")
            .argument::<String>("CHANNEL")
            .optional();
        let ttl_secs = long("ttl")
            .help("Expire after SECONDS")
            .argument::<u64>("SECONDS")
            .optional();
        construct!(Request::InviteCreate {
            invitee,
            role,
            channel,
            ttl_secs
        })
        .to_options()
        .command("create")
        .help("Invite an authority and print the shareable code")
    };
    let accept = {
        let invitation_id = invitation_id();
        construct!(Request::InviteAccept { invitation_id })
            .to_options()
            .command("accept")
            .help("Accept a received invitation")
    };
    let decline = {
        let invitation_id = invitation_id();
        construct!(Request::InviteDecline { invitation_id })
            .to_options()
            .command("decline")
            .help("Decline a received invitation")
    };
    let cancel = {
        let invitation_id = invitation_id();
        construct!(Request::InviteCancel { invitation_id })
            .to_options()
            .command("cancel")
            .help("Cancel an invitation you sent (confirm with --yes)")
    };
    let list = pure(Request::InviteList)
        .to_options()
        .command("list")
        .help("List pending invitations");
    let export = {
        let invitation_id = invitation_id();
        construct!(Request::InviteExport { invitation_id })
            .to_options()
            .command("export")
            .help("Print an invitation's shareable code")
    };
    let import = {
        let code = long("code")
            .help("Shareable invite code")
            .argument::<String>("CODE");
        construct!(Request::InviteImport { code })
            .to_options()
            .command("import")
            .help("Import a shareable code so it can be accepted")
    };
    construct!([create, accept, decline, cancel, list, export, import])
}

#[must_use]
pub fn home_parser() -> impl Parser<Request> {
    let create = {
        let name = long("name")
            .help("Home name")
            .argument::<String>("NAME")
            .optional();
        construct!(Request::HomeCreate { name })
            .to_options()
            .command("create")
            .help("Create a home")
    };
    let invite = {
        let authority = positional::<String>("AUTHORITY").help("Authority to invite");
        construct!(Request::HomeInvite { authority })
            .to_options()
            .command("invite")
            .help("Invite an authority into the current home")
    };
    let accept = pure(Request::HomeAccept)
        .to_options()
        .command("accept")
        .help("Accept the pending home or channel invitation");
    construct!([create, invite, accept])
}

#[must_use]
pub fn slash_parser() -> impl Parser<Request> {
    let channel = long("channel")
        .help("Channel the command runs in")
        .argument::<String>("CHANNEL")
        .optional();
    let command = positional::<String>("COMMAND").help("Slash command, e.g. \"/kick bob\"");
    construct!(Request::Slash { channel, command })
}

fn ceremony_id() -> impl Parser<String> {
    long("ceremony")
        .help("Recovery ceremony identifier")
        .argument::<String>("CEREMONY")
}

#[must_use]
pub fn recovery_parser() -> impl Parser<Request> {
    let start = {
        let guardians = long("guardians")
            .help("Comma-separated guardian authorities")
            .argument::<String>("GUARDIANS")
            .map(comma_list);
        let threshold = long("threshold")
            .help("Guardians required (default: 2)")
            .argument::<u16>("K")
            .fallback(2);
        construct!(Request::RecoveryStart {
            guardians,
            threshold
        })
        .to_options()
        .command("start")
        .help("Start guardian recovery")
    };
    let approve = {
        let ceremony_id = ceremony_id();
        construct!(Request::RecoveryApprove { ceremony_id })
            .to_options()
            .command("approve")
            .help("Approve a recovery ceremony as a guardian")
    };
    let dispute = {
        let ceremony_id = ceremony_id();
        let reason = long("reason")
            .help("Why this recovery is disputed")
            .argument::<String>("REASON");
        construct!(Request::RecoveryDispute {
            ceremony_id,
            reason
        })
        .to_options()
        .command("dispute")
        .help("Dispute a recovery ceremony as a guardian")
    };
    let status = pure(Request::RecoveryStatus)
        .to_options()
        .command("status")
        .help("Show guardians and recovery progress");
    construct!([start, approve, dispute, status])
}

/// `aura sync` subcommands other than `daemon`.
#[must_use]
pub fn sync_request_parser() -> impl Parser<Request> {
    let status = pure(Request::SyncStatus)
        .to_options()
        .command("status")
        .help("Show journal sync status");
    let once = {
        let peers = long("peers")
            .help("Comma-separated peer authorities (default: all peers)")
            .argument::<String>("PEERS")
            .map(comma_list)
            .fallback(Vec::new());
        construct!(Request::SyncOnce { peers })
            .to_options()
            .command("once")
            .help("Sync now")
    };
    let peer = || {
        long("peer")
            .help("Peer authority")
            .argument::<String>("PEER")
    };
    let add_peer = {
        let peer = peer();
        construct!(Request::PeerAdd { peer })
            .to_options()
            .command("add-peer")
            .help("Track a connected peer")
    };
    let remove_peer = {
        let peer = peer();
        construct!(Request::PeerRemove { peer })
            .to_options()
            .command("remove-peer")
            .help("Stop tracking a peer")
    };
    construct!([status, once, add_peer, remove_peer])
}

#[must_use]
pub fn admin_parser() -> impl Parser<Request> {
    let account = long("account")
        .help("Account identifier (UUID)")
        .argument::<String>("ACCOUNT");
    let new_admin = long("new-admin")
        .help("Authority of the new admin")
        .argument::<String>("AUTHORITY");
    let activation_epoch = long("activation-epoch")
        .help("Epoch when the new admin becomes authoritative")
        .argument::<u64>("EPOCH");
    construct!(Request::AdminReplace {
        account,
        new_admin,
        activation_epoch
    })
}
