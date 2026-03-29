use super::*;

pub(super) fn build_runtime_app(
    app_context: AppCoreContext,
    cb_context: CallbackContext,
    props: IoAppProps,
) -> AnyElement<'static> {
    #[cfg(feature = "development")]
    let IoAppProps {
        channels,
        messages,
        invitations,
        guardians,
        devices,
        nickname_suggestion,
        threshold_k,
        threshold_n,
        mfa_policy,
        contacts,
        discovered_peers,
        neighborhood_name,
        homes,
        access_level,
        show_account_setup,
        pending_runtime_bootstrap,
        network_status,
        transport_peers,
        known_online,
        update_rx,
        harness_command_rx,
        bootstrap_handoff_tx,
        update_tx,
        callbacks,
        #[cfg(feature = "development")]
        demo_mode,
        #[cfg(feature = "development")]
        demo_alice_code,
        #[cfg(feature = "development")]
        demo_carol_code,
        #[cfg(feature = "development")]
        demo_mobile_device_id,
        #[cfg(feature = "development")]
        demo_mobile_authority_id,
    } = props;

    #[cfg(feature = "development")]
    return element! {
        ContextProvider(value: Context::owned(app_context)) {
            ContextProvider(value: Context::owned(cb_context)) {
                IoApp(
                    channels: channels,
                    messages: messages,
                    invitations: invitations,
                    guardians: guardians,
                    devices: devices,
                    nickname_suggestion: nickname_suggestion,
                    threshold_k: threshold_k,
                    threshold_n: threshold_n,
                    mfa_policy: mfa_policy,
                    contacts: contacts,
                    discovered_peers: discovered_peers,
                    neighborhood_name: neighborhood_name,
                    homes: homes,
                    access_level: access_level,
                    show_account_setup: show_account_setup,
                    pending_runtime_bootstrap: pending_runtime_bootstrap,
                    network_status: network_status,
                    transport_peers: transport_peers,
                    known_online: known_online,
                    demo_mode: demo_mode,
                    demo_alice_code: demo_alice_code,
                    demo_carol_code: demo_carol_code,
                    demo_mobile_device_id: demo_mobile_device_id,
                    demo_mobile_authority_id: demo_mobile_authority_id,
                    update_rx: update_rx,
                    harness_command_rx: harness_command_rx,
                    bootstrap_handoff_tx: bootstrap_handoff_tx,
                    update_tx: update_tx,
                    callbacks: callbacks,
                )
            }
        }
    }
    .into();

    #[cfg(not(feature = "development"))]
    {
        let IoAppProps {
            channels,
            messages,
            invitations,
            guardians,
            devices,
            nickname_suggestion,
            threshold_k,
            threshold_n,
            mfa_policy,
            contacts,
            discovered_peers,
            neighborhood_name,
            homes,
            access_level,
            show_account_setup,
            pending_runtime_bootstrap,
            network_status,
            transport_peers,
            known_online,
            update_rx,
            harness_command_rx,
            bootstrap_handoff_tx,
            update_tx,
            callbacks,
        } = props;

        element! {
            ContextProvider(value: Context::owned(app_context)) {
                ContextProvider(value: Context::owned(cb_context)) {
                    IoApp(
                        channels: channels,
                        messages: messages,
                        invitations: invitations,
                        guardians: guardians,
                        devices: devices,
                        nickname_suggestion: nickname_suggestion,
                        threshold_k: threshold_k,
                        threshold_n: threshold_n,
                        mfa_policy: mfa_policy,
                        contacts: contacts,
                        discovered_peers: discovered_peers,
                        neighborhood_name: neighborhood_name,
                        homes: homes,
                        access_level: access_level,
                        show_account_setup: show_account_setup,
                        pending_runtime_bootstrap: pending_runtime_bootstrap,
                        network_status: network_status,
                        transport_peers: transport_peers,
                        known_online: known_online,
                        update_rx: update_rx,
                        harness_command_rx: harness_command_rx,
                        bootstrap_handoff_tx: bootstrap_handoff_tx,
                        update_tx: update_tx,
                        callbacks: callbacks,
                    )
                }
            }
        }
        .into()
    }
}
