use aura_macros::capability_family;

#[capability_family(namespace = "invitation")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum InvitationCapability {
    #[capability("send")]
    Send,
    #[capability("guardian:accept")]
    GuardianAccept,
}

fn main() {
    let send = InvitationCapability::Send.as_name();
    assert_eq!(send.as_str(), "invitation:send");
    assert_eq!(InvitationCapability::declared_names().len(), 2);
}
