use crate::app::services::data_source::use_data_source;
use crate::app::services::repl_commands::{ReplCommandHandler, ReplEntry};
use leptos::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;
use web_sys::KeyboardEvent;

#[component]
pub fn Repl() -> impl IntoView {
    let data_source_manager = use_data_source();
    let current_source = data_source_manager.current_source();

    let (history, set_history) = signal(std::collections::VecDeque::<ReplEntry>::new());
    let (history_index, set_history_index) = signal(None::<usize>);
    let (current_command, set_current_command) = signal(String::new());

    // Use Rc<RefCell> to share mutable command handler state
    let handler = Rc::new(RefCell::new(ReplCommandHandler::new()));

    // TODO: Re-enable when wasm_core is integrated
    // Connection status badge moved to Network Topology component

    let input_ref = NodeRef::<leptos::html::Input>::new();

    // Show welcome message and update when data source changes
    Effect::new(move |_| {
        let source = current_source.get();
        let welcome_entry = ReplEntry {
            command: String::new(),
            output: format!(
                "Welcome to the Aura REPL\nCurrent data source: {}\nType 'help' for available commands.",
                source
            ),
            is_error: false,
        };

        set_history.update(|h| {
            h.clear(); // Clear previous welcome messages
            h.push_back(welcome_entry);
        });
    });

    let execute_command = {
        let handler = handler.clone();
        let data_source_manager = data_source_manager.clone();

        move |cmd: String| {
            if cmd.trim().is_empty() {
                return;
            }

            // First, try to execute as a UI command via ReplCommandHandler
            let mut entry = handler.borrow_mut().execute(&cmd);

            // If the command was unrecognized, delegate to the current data source
            if entry.output.starts_with("Unknown command:") {
                let service = data_source_manager.get_service();
                entry.output = service.execute_command(&cmd);
            }

            set_history.update(|h| {
                h.push_back(entry);
                if h.len() > 100 {
                    h.pop_front();
                }
            });

            set_history_index.set(None);
            set_current_command.set(String::new());
        }
    };

    let handle_keydown = {
        let handler = handler.clone();
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
                    let cmd_hist = handler.borrow().get_history();
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
                    let cmd_hist = handler.borrow().get_history();
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
                    if let Some(suggestion) = handler.borrow().autocomplete(&partial) {
                        set_current_command.set(suggestion);
                    }
                }
                _ => {}
            }
        }
    };

    view! {
        <div class="flex flex-col h-full gap-3">
            <div class="flex-between gap-2">
                <h3 class="heading-1">"REPL"</h3>
            </div>

            <div class="flex-1 overflow-y-auto space-y-2 p-3 code-block">
                {move || {
                    history.get().iter().map(|entry| {
                        view! {
                            <div class="space-y-1">
                                {if !entry.command.is_empty() {
                                    view! {
                                        <div class="text-secondary">
                                            <span class="code-prompt">"> "</span>
                                            <span class="code-text">{entry.command.clone()}</span>
                                        </div>
                                    }.into_any()
                                } else {
                                    ().into_any()
                                }}
                                <div class=if entry.is_error {
                                    "code-error whitespace-pre-wrap"
                                } else {
                                    "code-output whitespace-pre-wrap"
                                }>
                                    {entry.output.clone()}
                                </div>
                            </div>
                        }
                    }).collect::<Vec<_>>()
                }}
            </div>

            <div class="flex-shrink-0">
                <div class="flex items-center gap-2 p-2 code-block">
                    <span class="code-prompt">"> "</span>
                    <input
                        type="text"
                        class="flex-1 bg-transparent text-zinc-900 dark:text-zinc-100 font-mono text-xs focus:outline-none"
                        placeholder="Enter command (help for options)..."
                        prop:value=move || current_command.get()
                        on:input=move |ev| {
                            set_current_command.set(event_target_value(&ev));
                        }
                        on:keydown=handle_keydown
                        node_ref=input_ref
                    />
                </div>
            </div>
        </div>
    }
}
