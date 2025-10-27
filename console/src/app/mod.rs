use leptos::prelude::*;
use stylance::import_style;

mod components;
mod services;

use components::{BranchManager, NetworkView, Repl, StateInspector, Timeline};
use services::websocket_foundation::use_websocket_foundation;
use services::websocket_foundation::ConnectionStatus;
use wasm_core::ConnectionState;

// Import CSS modules with Stylance
import_style!(style, "../../styles/app.css");

// Console modes
#[derive(Debug, Clone, PartialEq)]
pub enum ConsoleMode {
    Simulation,
    Live,
    Analysis,
}

impl std::fmt::Display for ConsoleMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConsoleMode::Simulation => write!(f, "Simulation"),
            ConsoleMode::Live => write!(f, "Live"),
            ConsoleMode::Analysis => write!(f, "Analysis"),
        }
    }
}

/// Main application component
#[component]
pub fn App() -> impl IntoView {
    log::info!("Rendering App component");

    // Current console mode state
    let (current_mode, set_current_mode) = signal(ConsoleMode::Simulation);

    // WebSocket service for backend communication using wasm-core
    let websocket_url = "ws://localhost:9003/ws".to_string();
    let (connection_state, events, responses, _send_command, _connect, _disconnect) =
        use_websocket_foundation(websocket_url);

    // Provide global state context
    provide_context(current_mode);
    provide_context(set_current_mode);
    provide_context(connection_state);
    provide_context(events);
    provide_context(responses);

    view! {
        <div class=style::app_container>
            <Header />
            <MainContent />
        </div>
    }
}

/// Header component with navigation and mode switcher
#[component]
fn Header() -> impl IntoView {
    let current_mode = use_context::<ReadSignal<ConsoleMode>>().expect("Mode context");
    let set_current_mode = use_context::<WriteSignal<ConsoleMode>>().expect("Mode setter context");
    let connection_state =
        use_context::<ReadSignal<ConnectionState>>().expect("Connection context");

    view! {
        <header class=style::header>
            <div class=style::header_content>
                <div class=style::logo_section>
                    <h1 class=style::logo>
                        "Aura Dev Console"
                    </h1>
                    <ConnectionStatus connection_state=connection_state />
                </div>

                <nav class=style::mode_switcher>
                    <ModeButton
                        mode=ConsoleMode::Simulation
                        current_mode=current_mode
                        set_mode=set_current_mode
                    />
                    <ModeButton
                        mode=ConsoleMode::Live
                        current_mode=current_mode
                        set_mode=set_current_mode
                    />
                    <ModeButton
                        mode=ConsoleMode::Analysis
                        current_mode=current_mode
                        set_mode=set_current_mode
                    />
                </nav>
            </div>
        </header>
    }
}

/// Mode switcher button component
#[component]
fn ModeButton(
    mode: ConsoleMode,
    current_mode: ReadSignal<ConsoleMode>,
    set_mode: WriteSignal<ConsoleMode>,
) -> impl IntoView {
    let mode_clone = mode.clone();

    view! {
        <button
            class=move || {
                if current_mode.get() == mode {
                    format!("{} {}", style::mode_button, style::mode_button_active)
                } else {
                    style::mode_button.to_string()
                }
            }
            on:click=move |_| {
                log::info!("Switching to {} mode", mode_clone);
                set_mode.set(mode_clone.clone());
            }
        >
            {mode.to_string()}
        </button>
    }
}

/// Main content area with mode-specific views
#[component]
fn MainContent() -> impl IntoView {
    let current_mode = use_context::<ReadSignal<ConsoleMode>>().expect("Mode context");

    view! {
        <main class=style::main_content>
            {move || {
                match current_mode.get() {
                    ConsoleMode::Simulation => {
                        view! {
                            <div class=style::mode_content>
                                <div class=style::simulation_layout>
                                    <div class=style::simulation_sidebar>
                                        <BranchManager />
                                    </div>
                                    <div class=style::simulation_main>
                                        <h2>"Simulation Mode"</h2>
                                        <p>"Interactive simulation environment"</p>
                                        <Timeline />
                                    </div>
                                    <div class=style::simulation_repl>
                                        <Repl />
                                    </div>
                                </div>
                            </div>
                        }.into_any()
                    },
                    ConsoleMode::Live => {
                        view! {
                            <div class=style::mode_content>
                                <h2>"Live Network Mode"</h2>
                                <p>"Real-time monitoring of live Aura nodes"</p>
                                <NetworkView />
                            </div>
                        }.into_any()
                    },
                    ConsoleMode::Analysis => {
                        view! {
                            <div class=style::mode_content>
                                <h2>"Analysis Mode"</h2>
                                <p>"Post-hoc trace analysis and visualization"</p>
                                <StateInspector />
                            </div>
                        }.into_any()
                    },
                }
            }}
        </main>
    }
}
