use crate::app::services::websocket_foundation::ConsoleCommand;
use leptos::prelude::*;
use std::collections::VecDeque;
use stylance::import_style;
use web_sys::KeyboardEvent;

import_style!(style, "../../../styles/repl.css");

#[derive(Clone, Debug)]
pub struct ReplEntry {
    pub command: String,
    pub output: String,
    pub is_error: bool,
}

#[component]
pub fn Repl() -> impl IntoView {
    let (history, set_history) = signal(VecDeque::<ReplEntry>::new());
    let (command_history, set_command_history) = signal(VecDeque::<String>::new());
    let (history_index, set_history_index) = signal(None::<usize>);
    let (current_command, set_current_command) = signal(String::new());

    let is_connected = use_context::<ReadSignal<wasm_core::ConnectionState>>()
        .map(|state| {
            Signal::derive(move || matches!(state.get(), wasm_core::ConnectionState::Connected))
        })
        .unwrap_or_else(|| signal(false).0.into());

    let input_ref = NodeRef::<leptos::html::Input>::new();

    Effect::new(move |_| {
        let welcome_entry = ReplEntry {
            command: String::new(),
            output: "Welcome to Aura Dev Console REPL\nType 'help' for available commands."
                .to_string(),
            is_error: false,
        };
        set_history.update(|h| h.push_back(welcome_entry));
    });

    let execute_command = {
        let _history = history.clone();
        let set_history = set_history.clone();
        let _command_history = command_history.clone();
        let set_command_history = set_command_history.clone();
        let set_history_index = set_history_index.clone();
        let set_current_command = set_current_command.clone();

        move |cmd: String| {
            if cmd.trim().is_empty() {
                return;
            }

            let output = handle_command(&cmd);
            let entry = ReplEntry {
                command: cmd.clone(),
                output,
                is_error: false,
            };

            set_history.update(|h| {
                h.push_back(entry);
                if h.len() > 100 {
                    h.pop_front();
                }
            });

            set_command_history.update(|ch| {
                ch.push_back(cmd);
                if ch.len() > 50 {
                    ch.pop_front();
                }
            });

            set_history_index.set(None);
            set_current_command.set(String::new());
        }
    };

    let handle_keydown = {
        let execute_command = execute_command.clone();
        move |ev: KeyboardEvent| {
            let key = ev.key();

            match key.as_str() {
                "Enter" => {
                    ev.prevent_default();
                    let cmd = current_command.get();
                    execute_command(cmd);
                }
                "ArrowUp" => {
                    ev.prevent_default();
                    let cmd_hist = command_history.get();
                    if !cmd_hist.is_empty() {
                        let new_index = match history_index.get() {
                            None => Some(cmd_hist.len() - 1),
                            Some(idx) => {
                                if idx > 0 {
                                    Some(idx - 1)
                                } else {
                                    Some(0)
                                }
                            }
                        };
                        if let Some(idx) = new_index {
                            if let Some(cmd) = cmd_hist.get(idx) {
                                set_current_command.set(cmd.clone());
                                set_history_index.set(new_index);
                            }
                        }
                    }
                }
                "ArrowDown" => {
                    ev.prevent_default();
                    let cmd_hist = command_history.get();
                    if !cmd_hist.is_empty() {
                        let new_index = match history_index.get() {
                            None => None,
                            Some(idx) => {
                                if idx < cmd_hist.len() - 1 {
                                    Some(idx + 1)
                                } else {
                                    None
                                }
                            }
                        };

                        if let Some(idx) = new_index {
                            if let Some(cmd) = cmd_hist.get(idx) {
                                set_current_command.set(cmd.clone());
                            }
                        } else {
                            set_current_command.set(String::new());
                        }
                        set_history_index.set(new_index);
                    }
                }
                "Tab" => {
                    ev.prevent_default();
                    let partial = current_command.get();
                    let suggestion = autocomplete_command(&partial);
                    if let Some(completed) = suggestion {
                        set_current_command.set(completed);
                    }
                }
                _ => {}
            }
        }
    };

    view! {
        <div class=style::repl_container>
            <div class=style::repl_header>
                <h3>"REPL Console"</h3>
                <div class=style::connection_indicator>
                    {move || {
                        if is_connected.get() {
                            view! {
                                <span class=format!("{} {}", style::status_indicator, style::connected)>
                                    "Connected"
                                </span>
                            }
                        } else {
                            view! {
                                <span class=format!("{} {}", style::status_indicator, style::disconnected)>
                                    "Disconnected"
                                </span>
                            }
                        }
                    }}
                </div>
            </div>

            <div class=style::repl_history>
                {move || {
                    history.get().iter().map(|entry| {
                        view! {
                            <div class=style::repl_entry>
                                {if !entry.command.is_empty() {
                                    view! {
                                        <div class=style::command_line>
                                            <span class=style::prompt>"aura> "</span>
                                            <span class=style::command>{entry.command.clone()}</span>
                                        </div>
                                    }.into_any()
                                } else {
                                    view! {}.into_any()
                                }}
                                <div class=if entry.is_error {
                                    format!("{} {}", style::output, style::error)
                                } else {
                                    style::output.to_string()
                                }>
                                    <pre>{entry.output.clone()}</pre>
                                </div>
                            </div>
                        }
                    }).collect::<Vec<_>>()
                }}
            </div>

            <div class=style::repl_input_area>
                <div class=style::input_line>
                    <span class=style::prompt>"aura> "</span>
                    <input
                        type="text"
                        class=style::repl_input
                        placeholder="Enter command..."
                        prop:value=move || current_command.get()
                        on:input=move |ev| {
                            set_current_command.set(event_target_value(&ev));
                        }
                        on:keydown=handle_keydown
                        node_ref=input_ref
                    />
                </div>
                <div class=style::help_text>
                    "Press Tab for autocomplete, ↑↓ for history, Enter to execute"
                </div>
            </div>
        </div>
    }
}

fn handle_command(cmd: &str) -> String {
    let trimmed = cmd.trim();
    let parts: Vec<&str> = trimmed.split_whitespace().collect();

    if parts.is_empty() {
        return String::new();
    }

    let command = parts[0];
    let args = &parts[1..];

    match command {
        "help" => {
            r#"Available Commands:

Simulation Control:
  step [n]              - Advance simulation by n ticks (default: 1)
  run                   - Run until idle
  reset                 - Reset simulation to beginning
  seek <tick>           - Jump to specific tick

State Inspection:
  devices               - List all devices
  state <device>        - Show device state
  ledger [device]       - Show ledger state
  network               - Show network topology
  events [device]       - Show recent events

Simulation Manipulation:
  inject <to> <msg>     - Inject message to device
  partition <devices>   - Create network partition
  byzantine <device>    - Make device Byzantine
  crash <device>        - Crash device
  recover <device>      - Recover crashed device

Branch Management:
  branches              - List all branches
  fork [name]           - Fork current branch
  checkout <branch>     - Switch to branch
  commit <name>         - Save branch as scenario
  export <file>         - Export current branch

Utilities:
  clear                 - Clear console
  status                - Show simulation status
  help                  - Show this help"#.to_string()
        }
        "devices" => {
            "Device List:\n- alice (online, honest)\n- bob (online, honest)\n- charlie (offline)\n- dave (online, byzantine)".to_string()
        }
        "status" => {
            format!("Simulation Status:\nTick: 1042\nBranch: main\nScenario: dkd-basic.toml\nDevices: 4 total, 3 online\nEvents: 156")
        }
        "state" => {
            if args.is_empty() {
                "Error: state command requires device name\nUsage: state <device>".to_string()
            } else {
                let device = args[0];
                format!("State for device '{}':\n{{\n  \"device_id\": \"{}\",\n  \"epoch\": 42,\n  \"status\": \"online\"\n}}", device, device)
            }
        }
        "network" => {
            "Network Topology:\nalice ←→ bob\nalice ←→ charlie (partitioned)\nbob ←→ dave\ncharlie ←→ dave".to_string()
        }
        "branches" => {
            "* main (scenario: dkd-basic.toml)\n  experiment-1 (forked at tick 100)\n  byzantine-test (forked at tick 50)".to_string()
        }
        "clear" => {
            "Console cleared.".to_string()
        }
        "step" => {
            let count = if args.is_empty() { 1 } else {
                args[0].parse().unwrap_or(1)
            };
            format!("Stepped {} tick(s). Current tick: {}", count, 1042 + count)
        }
        "run" => {
            "Running simulation until idle...\nSimulation complete at tick 1200.".to_string()
        }
        "reset" => {
            "Simulation reset to tick 0.".to_string()
        }
        "fork" => {
            let name = if args.is_empty() {
                format!("experiment-{}", chrono::Utc::now().timestamp())
            } else {
                args.join("-")
            };
            format!("Created new branch '{}' at current tick", name)
        }
        _ => {
            format!("Unknown command: {}\nType 'help' for available commands.", command)
        }
    }
}

#[allow(dead_code)]
fn parse_command(cmd: &str) -> Option<ConsoleCommand> {
    let trimmed = cmd.trim();
    let parts: Vec<&str> = trimmed.split_whitespace().collect();

    if parts.is_empty() {
        return None;
    }

    let command = parts[0];
    let args = &parts[1..];

    match command {
        "state" => {
            if let Some(device_id) = args.get(0) {
                Some(ConsoleCommand::QueryState {
                    device_id: device_id.to_string(),
                })
            } else {
                None
            }
        }
        "events" => {
            let since_event_id = args.get(0).and_then(|s| s.parse().ok());
            let limit = args.get(1).and_then(|s| s.parse().ok());
            Some(ConsoleCommand::GetEvents {
                since_event_id,
                limit,
            })
        }
        "network" => Some(ConsoleCommand::GetNetworkTopology),
        "devices" => Some(ConsoleCommand::GetDevices),
        "stats" => Some(ConsoleCommand::GetStats),
        "record" => {
            if let Some(enabled_str) = args.get(0) {
                let enabled = *enabled_str == "true" || *enabled_str == "on";
                Some(ConsoleCommand::SetRecording { enabled })
            } else {
                None
            }
        }
        "clear" => Some(ConsoleCommand::ClearEvents),
        "subscribe" => {
            let event_types = args.iter().map(|s| s.to_string()).collect();
            Some(ConsoleCommand::Subscribe { event_types })
        }
        "unsubscribe" => Some(ConsoleCommand::Unsubscribe),
        _ => None,
    }
}

fn autocomplete_command(partial: &str) -> Option<String> {
    let commands = [
        "help",
        "devices",
        "state",
        "ledger",
        "network",
        "events",
        "step",
        "run",
        "reset",
        "seek",
        "inject",
        "partition",
        "byzantine",
        "crash",
        "recover",
        "branches",
        "fork",
        "checkout",
        "commit",
        "export",
        "clear",
        "status",
    ];

    let matches: Vec<&str> = commands
        .iter()
        .filter(|cmd| cmd.starts_with(partial))
        .copied()
        .collect();

    if matches.len() == 1 {
        Some(matches[0].to_string())
    } else {
        None
    }
}
