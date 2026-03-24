use super::*;

use crate::tui::screens::app::shell::props::RuntimeShellPropsSeed;
use crate::tui::updates::{harness_command_channel, ui_update_channel};

fn build_runtime_app(
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

///
/// This version uses the IoContext to fetch actual data from the reactive
/// views instead of mock data.
pub async fn run_app_with_context(ctx: IoContext) -> std::io::Result<ShellExitIntent> {
    // Create the UI update channel for reactive updates
    let (update_tx, update_rx) = ui_update_channel();
    let update_rx_holder = Arc::new(Mutex::new(Some(update_rx)));
    let (harness_command_tx, harness_command_rx) = harness_command_channel();
    let harness_command_rx_holder = Arc::new(Mutex::new(Some(harness_command_rx)));
    let (bootstrap_handoff_tx, bootstrap_handoff_rx) = tokio::sync::oneshot::channel();
    let bootstrap_handoff_tx_holder = Arc::new(Mutex::new(Some(bootstrap_handoff_tx)));
    ctx.clear_bootstrap_runtime_handoff_committed()
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    ensure_harness_command_listener().await?;
    register_harness_command_sender(harness_command_tx).await?;

    // Create effect dispatch callbacks using CallbackRegistry
    let ctx_arc = Arc::new(ctx);
    let app_core = ctx_arc.app_core_raw().clone();
    let callbacks = CallbackRegistry::new(ctx_arc.clone(), update_tx.clone(), app_core);

    // Create CallbackContext for providing callbacks to components via iocraft context
    let callback_context = CallbackContext::new(callbacks.clone());

    // Check if account already exists to determine if we show setup modal
    let show_account_setup = !ctx_arc.has_account();

    let nickname_suggestion = {
        let reactive = {
            let core = ctx_arc.app_core_raw().read().await;
            core.reactive().clone()
        };
        reactive
            .read(&*SETTINGS_SIGNAL)
            .await
            .unwrap_or_default()
            .nickname_suggestion
    };

    // Create AppCoreContext for components to access AppCore and signals
    // AppCore is always available (demo mode uses agent-less AppCore)
    let app_core_context = AppCoreContext::new(ctx_arc.app_core().clone(), ctx_arc.clone());
    // Wrap the app in nested ContextProviders
    // This enables components to use:
    // - `hooks.use_context::<AppCoreContext>()` for reactive signal subscription
    // - `hooks.use_context::<CallbackContext>()` for accessing domain callbacks
    {
        let app_context = app_core_context;
        let cb_context = callback_context;
        let io_app_props = IoAppProps::from_runtime_seed(RuntimeShellPropsSeed {
            nickname_suggestion,
            show_account_setup,
            pending_runtime_bootstrap: ctx_arc.pending_runtime_bootstrap(),
            update_rx: update_rx_holder,
            harness_command_rx: harness_command_rx_holder,
            bootstrap_handoff_tx: bootstrap_handoff_tx_holder.clone(),
            update_tx: update_tx.clone(),
            callbacks,
            #[cfg(feature = "development")]
            demo_mode: ctx_arc.is_demo_mode(),
            #[cfg(feature = "development")]
            demo_alice_code: ctx_arc.demo_alice_code(),
            #[cfg(feature = "development")]
            demo_carol_code: ctx_arc.demo_carol_code(),
            #[cfg(feature = "development")]
            demo_mobile_device_id: ctx_arc.demo_mobile_device_id(),
            #[cfg(feature = "development")]
            demo_mobile_authority_id: ctx_arc.demo_mobile_authority_id(),
        });
        let mut app = build_runtime_app(app_context, cb_context, io_app_props);
        let result = if show_account_setup {
            let app_future = app.fullscreen();
            tokio::pin!(app_future);
            tokio::select! {
                result = &mut app_future => result,
                result = async {
                    bootstrap_handoff_rx.await.map_err(|error| {
                        std::io::Error::other(format!(
                            "bootstrap runtime handoff notification dropped before shell exit: {error}"
                        ))
                    })
                } => {
                    result?;
                    if !ctx_arc.bootstrap_runtime_handoff_committed() {
                        return Err(std::io::Error::other(
                            "bootstrap runtime handoff notified without committed marker",
                        ));
                    }
                    match tokio::time::timeout(std::time::Duration::from_secs(5), &mut app_future).await {
                        Ok(result) => result,
                        Err(_) => Err(std::io::Error::other(
                            "bootstrap runtime handoff committed but fullscreen generation did not exit within 5s",
                        )),
                    }
                }
            }
        } else {
            app.fullscreen().await
        };
        let _ = clear_harness_command_sender().await;
        result?;
        ctx_arc.take_shell_exit_intent().ok_or_else(|| {
            std::io::Error::other(
                "fullscreen generation exited without explicit ShellExitIntent; see docs/122_ownership_model.md",
            )
        })
    }
}
