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
    let list = pure(Request::ContactList)
        .to_options()
        .command("list")
        .help("List contacts");
    let rename = {
        let contact = contact();
        let nickname = positional::<String>("NICKNAME").help("Local nickname");
        construct!(Request::ContactRename { contact, nickname })
            .to_options()
            .command("rename")
            .help("Give a contact a local nickname")
    };
    let remove = {
        let contact = contact();
        construct!(Request::ContactRemove { contact })
            .to_options()
            .command("remove")
            .help("Remove a contact (confirm with --yes)")
    };
    let whois = {
        let target = positional::<String>("TARGET").help("Contact name or authority");
        construct!(Request::Whois { target })
            .to_options()
            .command("whois")
            .help("Show a contact's details")
    };
    let receipts = {
        let contact = contact();
        let enabled = on_off("STATE");
        construct!(Request::ReadReceipts { contact, enabled })
            .to_options()
            .command("read-receipts")
            .help("Send read receipts to a contact: on or off")
    };
    construct!([list, rename, remove, whois, receipts])
}

fn contact() -> impl Parser<String> {
    positional::<String>("CONTACT").help("Contact name or authority")
}

fn target() -> impl Parser<String> {
    positional::<String>("TARGET").help("Member name or authority")
}

/// `on` / `off` as a bool.
fn on_off(meta: &'static str) -> impl Parser<bool> {
    positional::<String>(meta)
        .help("on or off")
        .parse(|raw: String| match raw.as_str() {
            "on" | "true" | "yes" => Ok(true),
            "off" | "false" | "no" => Ok(false),
            other => Err(format!("expected on or off, got {other}")),
        })
}

#[must_use]
pub fn friend_parser() -> impl Parser<Request> {
    let send = {
        let contact = contact();
        construct!(Request::FriendRequest { contact })
            .to_options()
            .command("send")
            .help("Ask a contact to become a friend")
    };
    let accept = {
        let contact = contact();
        construct!(Request::FriendAccept { contact })
            .to_options()
            .command("accept")
            .help("Accept a friend request")
    };
    let decline = {
        let contact = contact();
        construct!(Request::FriendDecline { contact })
            .to_options()
            .command("decline")
            .help("Decline a friend request")
    };
    let revoke = {
        let contact = contact();
        construct!(Request::FriendRevoke { contact })
            .to_options()
            .command("revoke")
            .help("End a friendship (confirm with --yes)")
    };
    construct!([send, accept, decline, revoke])
}

#[must_use]
pub fn profile_parser() -> impl Parser<Request> {
    let nickname = positional::<String>("NICKNAME").help("New nickname");
    construct!(Request::ProfileNick { nickname })
        .to_options()
        .command("nick")
        .help("Set this account's nickname")
}

#[must_use]
pub fn settings_parser() -> impl Parser<Request> {
    let show = pure(Request::SettingsShow)
        .to_options()
        .command("show")
        .help("Show account settings");
    let mfa = {
        let require = on_off("STATE");
        construct!(Request::SettingsMfa { require })
            .to_options()
            .command("mfa")
            .help("Require multifactor approval: on or off")
    };
    construct!([show, mfa])
}

#[must_use]
pub fn neighborhood_parser() -> impl Parser<Request> {
    let create = {
        let name = positional::<String>("NAME").help("Neighborhood name");
        construct!(Request::NeighborhoodCreate { name })
            .to_options()
            .command("create")
            .help("Create a neighborhood")
    };
    let home = || positional::<String>("HOME").help("Home name or id");
    let add = {
        let home = home();
        construct!(Request::NeighborhoodAdd { home })
            .to_options()
            .command("add")
            .help("Add a home to the neighborhood")
    };
    let link = {
        let home = home();
        construct!(Request::NeighborhoodLink { home })
            .to_options()
            .command("link")
            .help("Link a home one hop away")
    };
    construct!([create, add, link])
}

#[must_use]
pub fn moderation_parser() -> impl Parser<Request> {
    let reason = || {
        long("reason")
            .help("Reason")
            .argument::<String>("REASON")
            .optional()
    };
    let kick = {
        let channel = long("channel")
            .help("Home channel (default: the current home)")
            .argument::<String>("CHANNEL")
            .optional();
        let reason = reason();
        let target = target();
        construct!(Request::ModKick {
            channel,
            reason,
            target
        })
        .to_options()
        .command("kick")
        .help("Remove a member (confirm with --yes)")
    };
    let ban = {
        let reason = reason();
        let target = target();
        construct!(Request::ModBan { reason, target })
            .to_options()
            .command("ban")
            .help("Ban an authority (confirm with --yes)")
    };
    let unban = {
        let target = target();
        construct!(Request::ModUnban { target })
            .to_options()
            .command("unban")
            .help("Lift a ban")
    };
    let mute = {
        let duration_secs = long("duration")
            .help("Mute for SECONDS (default: until unmuted)")
            .argument::<u64>("SECONDS")
            .optional();
        let target = target();
        construct!(Request::ModMute {
            duration_secs,
            target
        })
        .to_options()
        .command("mute")
        .help("Mute an authority")
    };
    let unmute = {
        let target = target();
        construct!(Request::ModUnmute { target })
            .to_options()
            .command("unmute")
            .help("Lift a mute")
    };
    let message = || positional::<String>("MESSAGE_ID").help("Message id");
    let pin = {
        let message_id = message();
        construct!(Request::ModPin { message_id })
            .to_options()
            .command("pin")
            .help("Pin a message")
    };
    let unpin = {
        let message_id = message();
        construct!(Request::ModUnpin { message_id })
            .to_options()
            .command("unpin")
            .help("Unpin a message")
    };
    let op = {
        let target = target();
        construct!(Request::ModOp { target })
            .to_options()
            .command("op")
            .help("Make a member a moderator")
    };
    let deop = {
        let target = target();
        construct!(Request::ModDeop { target })
            .to_options()
            .command("deop")
            .help("Revoke moderator")
    };
    let admit = {
        let target = target();
        construct!(Request::ModAdmit { target })
            .to_options()
            .command("admit")
            .help("Admit a participant as a member")
    };
    construct!([kick, ban, unban, mute, unmute, pin, unpin, op, deop, admit])
}

#[must_use]
pub fn access_parser() -> impl Parser<Request> {
    let home = long("home")
        .help("Home (default: the current home)")
        .argument::<String>("HOME")
        .optional();
    let target = positional::<String>("AUTHORITY").help("Authority");
    let level = positional::<String>("LEVEL").help("limited, partial or full");
    construct!(Request::AccessSet {
        home,
        target,
        level
    })
    .to_options()
    .command("set")
    .help("Override an authority's access level")
}

#[must_use]
pub fn peer_parser() -> impl Parser<Request> {
    let list = pure(Request::PeerList)
        .to_options()
        .command("list")
        .help("List known peers");
    let discover = pure(Request::PeerDiscover)
        .to_options()
        .command("discover")
        .help("Discover peers now");
    construct!([list, discover])
}

#[must_use]
pub fn device_parser() -> impl Parser<Request> {
    let threshold = {
        let k = positional::<u8>("K").help("Signatures required");
        let n = positional::<u8>("N").help("Devices");
        construct!(Request::ThresholdSet { k, n })
            .to_options()
            .command("threshold")
            .help("Change the device signing threshold")
    };
    let remove = {
        let device = positional::<String>("DEVICE").help("Device id");
        construct!(Request::DeviceRemove { device })
            .to_options()
            .command("remove")
            .help("Remove a device (confirm with --yes)")
    };
    construct!([threshold, remove])
}

#[must_use]
pub fn guardians_parser() -> impl Parser<Request> {
    let guardians = long("guardians")
        .help("Comma-separated guardian authorities")
        .argument::<String>("GUARDIANS")
        .map(comma_list);
    let threshold = long("threshold")
        .help("Guardians required")
        .argument::<u16>("K");
    construct!(Request::GuardiansSet {
        guardians,
        threshold
    })
    .to_options()
    .command("set")
    .help("Set this account's guardians")
}

#[must_use]
pub fn rotation_parser() -> impl Parser<Request> {
    let ceremony_id = positional::<String>("CEREMONY").help("Ceremony id");
    let status = construct!(Request::RotationStatus { ceremony_id })
        .to_options()
        .command("status")
        .help("Show a key-rotation ceremony's progress");
    let ceremony_id = positional::<String>("CEREMONY").help("Ceremony id");
    let cancel = construct!(Request::RotationCancel { ceremony_id })
        .to_options()
        .command("cancel")
        .help("Cancel a key-rotation ceremony");
    construct!([status, cancel])
}

/// `aura ota` subcommands that are plain requests (`publish` reads files and
/// is assembled by the binary).
#[must_use]
pub fn ota_parser() -> impl Parser<Request> {
    let list = pure(Request::OtaList)
        .to_options()
        .command("list")
        .help("Declared releases");
    let status = pure(Request::OtaStatus)
        .to_options()
        .command("status")
        .help("This account's staged and activated upgrades");
    let release = positional::<String>("RELEASE").help("Release id (hex)");
    let recommend = construct!(Request::OtaRecommend { release })
        .to_options()
        .command("recommend")
        .help("Recommend a declared release to this account");
    let release = positional::<String>("RELEASE").help("Release id (hex)");
    let from = long("from")
        .help("Release this account runs, when no completed cutover records it")
        .argument::<String>("RELEASE")
        .optional();
    let stage = construct!(Request::OtaStage { from, release })
        .to_options()
        .command("stage")
        .help("Stage a declared release for this account");
    construct!([list, status, recommend, stage])
}

#[must_use]
pub fn account_parser() -> impl Parser<Request> {
    pure(Request::AccountRefresh)
        .to_options()
        .command("refresh")
        .help("Refresh account state from the runtime")
}

#[must_use]
pub fn notifications_parser() -> impl Parser<Request> {
    pure(Request::NotificationsList)
        .to_options()
        .command("list")
        .help("List pending friend requests, invitations and recovery requests")
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
        let accept = long("accept").help("Accept it right away").switch();
        construct!(Request::InviteImport { code, accept })
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
    let enter = {
        let depth = long("depth")
            .help("limited, partial or full (default: full)")
            .argument::<String>("DEPTH")
            .optional();
        let home = positional::<String>("HOME").help("Home name or id");
        construct!(Request::HomeEnter { depth, home })
            .to_options()
            .command("enter")
            .help("Move to a home")
    };
    construct!([create, invite, accept, enter])
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
