//! `aura chat`: messaging through the shared messaging workflows.
//!
//! Channels are named by their name (`general`, `#general`) or canonical id.

use crate::command::{ExportFormat, Request};
use bpaf::{construct, long, positional, pure, short, Parser};

fn channel() -> impl Parser<String> {
    positional::<String>("CHANNEL").help("Channel name or id")
}

fn sender() -> impl Parser<Option<String>> {
    long("sender")
        .help("Only messages from this authority")
        .argument::<String>("AUTHORITY")
        .optional()
}

fn list() -> impl Parser<Request> {
    pure(Request::ChatList)
        .to_options()
        .command("list")
        .help("List joined channels")
}

fn show() -> impl Parser<Request> {
    let channel = channel();
    construct!(Request::ChatShow { channel })
        .to_options()
        .command("show")
        .help("Show a channel's details and members")
}

fn history() -> impl Parser<Request> {
    let limit = short('l')
        .long("limit")
        .help("Only the last LIMIT messages")
        .argument::<usize>("LIMIT")
        .optional();
    let sender = sender();
    let channel = channel();
    construct!(Request::ChatHistory {
        limit,
        sender,
        channel
    })
    .to_options()
    .command("history")
    .help("Show a channel's messages")
}

fn send() -> impl Parser<Request> {
    let channel = channel();
    let message = positional::<String>("MESSAGE").help("Message text");
    construct!(Request::ChatSend { channel, message })
        .to_options()
        .command("send")
        .help("Send a message")
}

fn create() -> impl Parser<Request> {
    let topic = long("topic")
        .help("Channel topic")
        .argument::<String>("TOPIC")
        .optional();
    let members = short('m')
        .long("member")
        .help("Initial member authority (repeatable)")
        .argument::<String>("AUTHORITY")
        .many();
    let name = positional::<String>("NAME").help("Channel name");
    construct!(Request::ChatCreate {
        topic,
        members,
        name
    })
    .to_options()
    .command("create")
    .help("Create a channel")
}

fn invite() -> impl Parser<Request> {
    let channel = channel();
    let authority = positional::<String>("AUTHORITY").help("Authority to invite");
    construct!(Request::ChatInvite { channel, authority })
        .to_options()
        .command("invite")
        .help("Invite an authority into a channel")
}

fn leave() -> impl Parser<Request> {
    let channel = channel();
    construct!(Request::ChatLeave { channel })
        .to_options()
        .command("leave")
        .help("Leave a channel (confirm with --yes)")
}

fn update() -> impl Parser<Request> {
    let name = long("name")
        .help("New channel name")
        .argument::<String>("NAME")
        .optional();
    let topic = long("topic")
        .help("New channel topic")
        .argument::<String>("TOPIC")
        .optional();
    let channel = channel();
    construct!(Request::ChatUpdate {
        name,
        topic,
        channel
    })
    .to_options()
    .command("update")
    .help("Rename a channel or set its topic")
}

fn search() -> impl Parser<Request> {
    let channel = long("channel")
        .help("Only this channel")
        .argument::<String>("CHANNEL")
        .optional();
    let sender = sender();
    let limit = short('l')
        .long("limit")
        .help("Maximum number of results (default: 20)")
        .argument::<usize>("LIMIT")
        .fallback(20);
    let query = positional::<String>("QUERY").help("Text to find");
    construct!(Request::ChatSearch {
        channel,
        sender,
        limit,
        query
    })
    .to_options()
    .command("search")
    .help("Find messages containing a text")
}

fn export() -> impl Parser<Request> {
    let format = short('f')
        .long("format")
        .help("json, csv or text (default: json)")
        .argument::<ExportFormat>("FORMAT")
        .fallback(ExportFormat::Json);
    let channel = channel();
    construct!(Request::ChatExport { format, channel })
        .to_options()
        .command("export")
        .help("Print a channel's history in a portable format")
}

fn dm() -> impl Parser<Request> {
    let contact = positional::<String>("CONTACT").help("Contact name or authority");
    let message = positional::<String>("MESSAGE").help("Message text");
    construct!(Request::ChatDm { contact, message })
        .to_options()
        .command("dm")
        .help("Send a direct message to a contact")
}

fn join() -> impl Parser<Request> {
    let channel = positional::<String>("NAME").help("Channel name");
    construct!(Request::ChatJoin { channel })
        .to_options()
        .command("join")
        .help("Join a channel by name")
}

fn close() -> impl Parser<Request> {
    let channel = channel();
    construct!(Request::ChatClose { channel })
        .to_options()
        .command("close")
        .help("Close a channel you own (confirm with --yes)")
}

fn members() -> impl Parser<Request> {
    let channel = channel();
    construct!(Request::ChatMembers { channel })
        .to_options()
        .command("members")
        .help("List a channel's participants")
}

fn retry() -> impl Parser<Request> {
    let channel = channel();
    let message_id = positional::<String>("MESSAGE_ID").help("Message to resend");
    construct!(Request::ChatRetry {
        channel,
        message_id
    })
    .to_options()
    .command("retry")
    .help("Resend a message that failed to deliver")
}

fn mark_read() -> impl Parser<Request> {
    let channel = channel();
    construct!(Request::ChatMarkRead { channel })
        .to_options()
        .command("mark-read")
        .help("Mark a channel's messages read")
}

#[must_use]
pub fn chat_parser() -> impl Parser<Request> {
    construct!([
        list(),
        show(),
        history(),
        send(),
        create(),
        invite(),
        leave(),
        update(),
        search(),
        export(),
        dm(),
        join(),
        close(),
        members(),
        retry(),
        mark_read()
    ])
}
