fn main() {
    let _ = aura_app::ui::workflows::messaging::refresh_authoritative_channel_membership_readiness;
    let _ =
        aura_app::ui::workflows::messaging::refresh_authoritative_recipient_resolution_readiness;
}
