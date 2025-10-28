use leptos::prelude::*;

mod components;
mod services;

use components::{BranchManager, Moon, NetworkView, Repl, StateInspector, Sun, Timeline};
use services::data_source::{DataSource, DataSourceManager};
// use services::websocket_foundation::use_websocket_foundation;
// use services::websocket_foundation::ConnectionStatus;
// use wasm_core::ConnectionState;

/// Main application component
#[component]
pub fn App() -> impl IntoView {
    log::info!("Rendering App component");

    // Data source manager
    let data_source_manager = DataSourceManager::new();

    // Dark mode state - default to light mode
    // TODO: Implement localStorage persistence once web_sys Storage API is available
    let initial_dark_mode = false;

    let (dark_mode, set_dark_mode) = signal(initial_dark_mode);

    // Update document class when dark mode changes
    {
        let dark_mode_val = dark_mode;
        Effect::new(move |_| {
            let is_dark = dark_mode_val.get();
            log::info!("Dark mode effect triggered. is_dark={}", is_dark);
            #[cfg(target_arch = "wasm32")]
            {
                if let Some(document) = web_sys::window().and_then(|w| Some(w.document()?)) {
                    if let Some(body) = document.body() {
                        log::info!("Got body element");
                        if is_dark {
                            log::info!("Adding dark class");
                            let _ = body.class_list().add_1("dark");
                        } else {
                            log::info!("Removing dark class");
                            let _ = body.class_list().remove_1("dark");
                        }
                        log::info!("Body classes: {:?}", body.class_name());
                    } else {
                        log::error!("Could not get body element");
                    }
                } else {
                    log::error!("Could not get document");
                }
            }
        });
    }

    // TODO: Re-enable WebSocket when wasm_core is re-integrated
    // let websocket_url = "ws://localhost:9003/ws".to_string();
    // let (connection_state, events, responses, _send_command, _connect, _disconnect) =
    //     use_websocket_foundation(websocket_url);

    // Provide global state context
    provide_context(data_source_manager);
    provide_context(dark_mode);
    provide_context(set_dark_mode);
    // provide_context(connection_state);
    // provide_context(events);
    // provide_context(responses);

    view! {
        <div class="main-container">
            <Header />
            <MainContent />
        </div>
    }
}

/// Header component with data source switcher
#[component]
fn Header() -> impl IntoView {
    let data_source_manager =
        use_context::<DataSourceManager>().expect("Data source manager context");
    let dark_mode = use_context::<ReadSignal<bool>>().expect("Dark mode context");
    let set_dark_mode = use_context::<WriteSignal<bool>>().expect("Dark mode setter context");
    // let connection_state =
    //     use_context::<ReadSignal<ConnectionState>>().expect("Connection context");

    view! {
        <header class="header-base">
            <div class="px-4 sm:px-6 lg:px-8 py-4">
                <div class="flex-between">
                    <div class="flex-row-center gap-3">
                        <h1 class="header-title">
                            <a href="/" class="text-zinc-900 dark:text-zinc-50 no-underline hover:text-zinc-900 dark:hover:text-zinc-50">
                                "Aura"
                            </a>
                        </h1>
                        // TODO: Re-enable when wasm_core is integrated
                        // <ConnectionStatus connection_state=connection_state />
                    </div>

                    <nav class="flex gap-2 items-center">
                        <DataSourceSwitcher data_source_manager=data_source_manager />

                        <button
                            class="btn-icon"
                            on:click=move |_| {
                                log::info!("Dark mode toggle button clicked");
                                set_dark_mode.update(|d| {
                                    *d = !*d;
                                    log::info!("Dark mode updated to: {}", *d);
                                });
                            }
                            title=move || if dark_mode.get() { "Switch to light mode" } else { "Switch to dark mode" }
                        >
                            {move || if dark_mode.get() {
                                view! { <Moon size=16 /> }.into_any()
                            } else {
                                view! { <Sun size=16 /> }.into_any()
                            }}
                        </button>
                    </nav>
                </div>
            </div>
        </header>
    }
}

/// Main content area with data-source-driven views
#[component]
fn MainContent() -> impl IntoView {
    let data_source_manager =
        use_context::<DataSourceManager>().expect("Data source manager context");
    let current_source = data_source_manager.current_source();

    view! {
        <main class="main-content">
            <div class="h-full grid grid-cols-1 md:grid-cols-3 gap-4 p-4">
                // Content area - 2 column width on desktop
                <section class="md:col-span-2 h-full overflow-y-auto">
                    <div class="grid grid-cols-1 gap-4">
                        // Network topology - always first, always visible
                        <div class="panel panel-padding">
                            <NetworkView />
                        </div>

                        // State inspector - always visible, directly below network
                        <div class="panel panel-padding">
                            <StateInspector />
                        </div>

                        {move || {
                            let source = current_source.get();
                            match source {
                                DataSource::Mock | DataSource::Simulator => {
                                    // Simulation sources: branches and timeline side by side
                                    view! {
                                        <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
                                            <div class="panel panel-padding">
                                                <BranchManager />
                                            </div>
                                            <div class="panel panel-padding min-h-96">
                                                <Timeline />
                                            </div>
                                        </div>
                                    }.into_any()
                                },
                                DataSource::Real => {
                                    // Live source: no additional components
                                    view! {
                                        <></>
                                    }.into_any()
                                },
                            }
                        }}
                    </div>
                </section>

                // REPL - always visible, rightmost, 1 column width
                <aside class="md:col-span-1 h-full overflow-y-auto">
                    <div class="panel panel-padding h-full flex flex-col">
                        <Repl />
                    </div>
                </aside>
            </div>
        </main>
    }
}

/// Data source switcher component
#[component]
fn DataSourceSwitcher(data_source_manager: DataSourceManager) -> impl IntoView {
    let current_source = data_source_manager.current_source();

    view! {
        <div class="flex items-center gap-1.5">
            <div class="relative">
                <select
                    class="input-base text-xs pl-3 pr-8 py-2 appearance-none cursor-pointer"
                    on:change=move |ev| {
                        let value = event_target_value(&ev);
                        let source = match value.as_str() {
                            "mock" => DataSource::Mock,
                            "simulator" => DataSource::Simulator,
                            "real" => DataSource::Real,
                            _ => DataSource::Mock,
                        };
                        data_source_manager.set_source(source);
                        log::info!("Data source switched to: {}", value);
                    }
                >
                    <option value="mock" selected=move || current_source.get() == DataSource::Mock>"Mock"</option>
                    <option value="simulator" selected=move || current_source.get() == DataSource::Simulator>"Simulated"</option>
                    <option value="real" selected=move || current_source.get() == DataSource::Real>"Live"</option>
                </select>
                <div class="absolute right-2 top-1/2 -translate-y-1/2 pointer-events-none text-zinc-400 dark:text-zinc-500">
                    <components::ChevronDown size=14 />
                </div>
            </div>
        </div>
    }
}
