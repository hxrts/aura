use crate::app::components::GitFork;
use crate::app::services::data_source::use_data_source;
use crate::app::services::mock_data::Branch;
use leptos::prelude::*;

#[component]
pub fn BranchManager() -> impl IntoView {
    let data_source_manager = use_data_source();
    let current_source = data_source_manager.current_source();

    let branches = RwSignal::new(Vec::<Branch>::new());
    let show_commit_dialog = RwSignal::new(false);
    let commit_name = RwSignal::new(String::new());
    let branch_to_commit = RwSignal::new(None::<String>);

    // Load branches from current data source
    Effect::new(move |_| {
        let source = current_source.get();
        log::info!("BranchManager updating for data source: {:?}", source);
        let service = data_source_manager.get_service();
        let branch_data = service.get_branches();
        branches.set(branch_data);
    });

    let current_branch = move || branches.get().into_iter().find(|b| b.is_current);

    let switch_branch = move |branch_id: String| {
        branches.update(|branches| {
            for branch in branches.iter_mut() {
                branch.is_current = branch.id == branch_id;
            }
        });
    };

    let delete_branch = move |branch_id: String| {
        if web_sys::window()
            .unwrap()
            .confirm_with_message(&format!(
                "Delete branch '{}'? This cannot be undone.",
                branch_id
            ))
            .unwrap_or(false)
        {
            branches.update(|branches| {
                branches.retain(|b| b.id != branch_id);
            });
        }
    };

    let fork_branch = move |_| {
        let current = current_branch();
        if let Some(current_branch) = current {
            let timestamp = js_sys::Date::now() as u64;
            let new_branch = Branch {
                id: format!("branch-{}", timestamp),
                name: format!("experiment-{}", timestamp),
                is_current: false,
                parent: Some(current_branch.id.clone()),
                fork_tick: Some(1042),
                scenario: None,
                last_modified: "Just now".to_string(),
                commit_count: 0,
            };

            branches.update(|branches| {
                branches.push(new_branch);
            });
        }
    };

    let start_commit = move |branch_id: String| {
        branch_to_commit.set(Some(branch_id.clone()));
        let timestamp = js_sys::Date::now() as u64;
        commit_name.set(format!("scenario-{}", timestamp));
        show_commit_dialog.set(true);
    };

    let confirm_commit = move |_| {
        if let Some(branch_id) = branch_to_commit.get() {
            let scenario_name = commit_name.get();
            if !scenario_name.trim().is_empty() {
                branches.update(|branches| {
                    if let Some(branch) = branches.iter_mut().find(|b| b.id == branch_id) {
                        branch.scenario = Some(scenario_name.clone());
                        branch.commit_count += 1;
                    }
                });

                web_sys::console::log_1(
                    &format!(
                        "Committed branch '{}' as scenario '{}'",
                        branch_id, scenario_name
                    )
                    .into(),
                );
            }

            show_commit_dialog.set(false);
            branch_to_commit.set(None);
            commit_name.set(String::new());
        }
    };

    let cancel_commit = move |_| {
        show_commit_dialog.set(false);
        branch_to_commit.set(None);
        commit_name.set(String::new());
    };

    view! {
        <div class="flex flex-col h-full w-full min-h-0">
            <div class="flex items-center justify-between gap-2 flex-shrink-0 pb-3 border-b border-zinc-200 dark:border-zinc-700">
                <h3 class="heading-1">"Branches"</h3>
                <button
                    class="btn-icon"
                    on:click=move |_| fork_branch(())
                    title="Fork current branch"
                >
                    <GitFork size=16 />
                </button>
            </div>

            <div class="flex-1 min-h-0 pt-3 relative">
                <div class="h-full overflow-y-auto space-y-2">
                {move || {
                    branches.get().iter().map(|branch| {
                        let branch_id = branch.id.clone();

                        let branch_item_class = if branch.is_current {
                            "p-3 bg-purple-50 dark:bg-zinc-800 border border-[#bc6de3] rounded-md cursor-pointer hover:bg-purple-100 dark:hover:bg-zinc-700 transition-colors"
                        } else {
                            "p-3 bg-zinc-100 dark:bg-zinc-800 border border-zinc-200 dark:border-zinc-700 rounded-md cursor-pointer hover:border-[#bc6de3] transition-colors"
                        };

                        view! {
                            <div class=branch_item_class>
                                <div on:click={
                                    let branch_id = branch_id.clone();
                                    move |_| switch_branch(branch_id.clone())
                                }>
                                    <div class="flex items-start gap-2 mb-2">
                                        <div class="flex-shrink-0 w-5">
                                            <div class={if branch.is_current {
                                                "w-2 h-2 rounded-full bg-[#bc6de3] mt-1.5"
                                            } else {
                                                "w-2 h-2 rounded-full border-2 border-zinc-400 dark:border-zinc-600 mt-1.5"
                                            }}>
                                            </div>
                                        </div>
                                        <div class="flex-1 min-w-0">
                                            <div class="text-sm font-semibold text-zinc-900 dark:text-zinc-50 truncate">
                                                {branch.name.clone()}
                                            </div>
                                        </div>
                                    </div>

                                    <div class="ml-7 space-y-1">
                                        {if let Some(ref parent) = branch.parent {
                                            let parent_text = if let Some(tick) = branch.fork_tick {
                                                format!("from {} at tick {}", parent, tick)
                                            } else {
                                                format!("from {}", parent)
                                            };
                                            view! {
                                                <div class="text-xs text-zinc-600 dark:text-zinc-400">
                                                    {parent_text}
                                                </div>
                                            }.into_any()
                                        } else {
                                            view! {
                                                <div class="text-xs text-zinc-600 dark:text-zinc-400">"Main branch"</div>
                                            }.into_any()
                                        }}

                                        {if let Some(ref scenario) = branch.scenario {
                                            view! {
                                                <div class="text-xs text-[#bc6de3] font-medium">
                                                    "Saved: "{scenario.clone()}
                                                </div>
                                            }.into_any()
                                        } else {
                                            ().into_any()
                                        }}

                                        <div class="text-xs text-zinc-500 dark:text-zinc-500">
                                            {format!("{} commits | {}", branch.commit_count, branch.last_modified)}
                                        </div>
                                    </div>
                                </div>

                                <div class="flex justify-end gap-1 mt-2">
                                    {if branch.scenario.is_none() {
                                        view! {
                                            <button
                                                class="btn-success"
                                                on:click={
                                                    let branch_id = branch_id.clone();
                                                    move |ev| {
                                                        ev.stop_propagation();
                                                        start_commit(branch_id.clone());
                                                    }
                                                }
                                                title="Save as scenario"
                                            >
                                                "Save"
                                            </button>
                                        }.into_any()
                                    } else {
                                        ().into_any()
                                    }}

                                    {if branch.id != "main" {
                                        view! {
                                            <button
                                                class="btn-danger"
                                                on:click={
                                                    let branch_id = branch_id.clone();
                                                    move |ev| {
                                                        ev.stop_propagation();
                                                        delete_branch(branch_id.clone());
                                                    }
                                                }
                                                title="Delete branch"
                                            >
                                                "Delete"
                                            </button>
                                        }.into_any()
                                    } else {
                                        ().into_any()
                                    }}
                                </div>
                            </div>
                        }
                    }).collect::<Vec<_>>()
                }}
                </div>
                <div class="pointer-events-none absolute bottom-0 left-0 right-0 h-8 bg-gradient-to-t from-white dark:from-zinc-900 to-transparent"></div>
            </div>

            {move || if show_commit_dialog.get() {
                view! {
                    <div class="modal-overlay">
                        <div class="modal">
                            <div class="modal-header">
                                <h4>"Save Branch as Scenario"</h4>
                            </div>
                            <div class="modal-body">
                                <p>"Choose a name for your scenario:"</p>
                                <input
                                    type="text"
                                    class="modal-input"
                                    placeholder="scenario-name"
                                    prop:value=move || commit_name.get()
                                    on:input=move |ev| {
                                        commit_name.set(event_target_value(&ev));
                                    }
                                />
                            </div>
                            <div class="modal-actions">
                                <button
                                    class="btn-secondary"
                                    on:click=move |_| cancel_commit(())
                                >
                                    "Cancel"
                                </button>
                                <button
                                    class="btn-primary"
                                    on:click=move |_| confirm_commit(())
                                    disabled=move || commit_name.get().trim().is_empty()
                                >
                                    "Save Scenario"
                                </button>
                            </div>
                        </div>
                    </div>
                }.into_any()
            } else {
                ().into_any()
            }}
        </div>
    }
}
