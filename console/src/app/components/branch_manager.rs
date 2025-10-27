use leptos::prelude::*;
use stylance::import_style;

import_style!(style, "../../../styles/branch-manager.css");

#[derive(Clone, Debug)]
pub struct Branch {
    pub id: String,
    pub name: String,
    pub is_current: bool,
    pub parent: Option<String>,
    pub fork_tick: Option<u64>,
    pub scenario: Option<String>,
    pub last_modified: String,
    pub commit_count: u32,
}

#[component]
pub fn BranchManager() -> impl IntoView {
    let (branches, set_branches) = signal(get_mock_branches());
    let (show_commit_dialog, set_show_commit_dialog) = signal(false);
    let (commit_name, set_commit_name) = signal(String::new());
    let (branch_to_commit, set_branch_to_commit) = signal(None::<String>);

    let current_branch = move || {
        branches
            .get()
            .iter()
            .find(|b| b.is_current)
            .map(|b| b.clone())
    };

    let switch_branch = {
        let set_branches = set_branches.clone();
        move |branch_id: String| {
            set_branches.update(|branches| {
                for branch in branches.iter_mut() {
                    branch.is_current = branch.id == branch_id;
                }
            });
        }
    };

    let delete_branch = {
        let set_branches = set_branches.clone();
        move |branch_id: String| {
            if web_sys::window()
                .unwrap()
                .confirm_with_message(&format!(
                    "Delete branch '{}'? This cannot be undone.",
                    branch_id
                ))
                .unwrap_or(false)
            {
                set_branches.update(|branches| {
                    branches.retain(|b| b.id != branch_id);
                });
            }
        }
    };

    let fork_branch = {
        let set_branches = set_branches.clone();
        move |_| {
            let current = current_branch();
            if let Some(current_branch) = current {
                let new_branch = Branch {
                    id: format!("branch-{}", chrono::Utc::now().timestamp()),
                    name: format!("experiment-{}", chrono::Utc::now().timestamp()),
                    is_current: false,
                    parent: Some(current_branch.id.clone()),
                    fork_tick: Some(1042),
                    scenario: None,
                    last_modified: "Just now".to_string(),
                    commit_count: 0,
                };

                set_branches.update(|branches| {
                    branches.push(new_branch);
                });
            }
        }
    };

    let start_commit = {
        let set_show_commit_dialog = set_show_commit_dialog.clone();
        let set_branch_to_commit = set_branch_to_commit.clone();
        let set_commit_name = set_commit_name.clone();
        move |branch_id: String| {
            set_branch_to_commit.set(Some(branch_id.clone()));
            set_commit_name.set(format!("scenario-{}", chrono::Utc::now().timestamp()));
            set_show_commit_dialog.set(true);
        }
    };

    let confirm_commit = {
        let set_branches = set_branches.clone();
        let set_show_commit_dialog = set_show_commit_dialog.clone();
        let set_branch_to_commit = set_branch_to_commit.clone();
        let set_commit_name = set_commit_name.clone();
        move |_| {
            if let Some(branch_id) = branch_to_commit.get() {
                let scenario_name = commit_name.get();
                if !scenario_name.trim().is_empty() {
                    set_branches.update(|branches| {
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
            }

            set_show_commit_dialog.set(false);
            set_branch_to_commit.set(None);
            set_commit_name.set(String::new());
        }
    };

    let cancel_commit = {
        let set_show_commit_dialog = set_show_commit_dialog.clone();
        let set_branch_to_commit = set_branch_to_commit.clone();
        let set_commit_name = set_commit_name.clone();
        move |_| {
            set_show_commit_dialog.set(false);
            set_branch_to_commit.set(None);
            set_commit_name.set(String::new());
        }
    };

    view! {
        <div class=style::branch_manager>
            <div class=style::branch_header>
                <h3>"Branches"</h3>
                <button
                    class=format!("{} {} {}", style::btn, style::btn_primary, style::btn_sm)
                    on:click=move |_| fork_branch(())
                    title="Fork current branch"
                >
                    "🔀 Fork"
                </button>
            </div>

            <div class=style::branch_list>
                {move || {
                    branches.get().iter().map(|branch| {
                        let branch_id = branch.id.clone();
                        let switch_branch = switch_branch.clone();
                        let delete_branch = delete_branch.clone();
                        let start_commit = start_commit.clone();

                        let branch_item_class = if branch.is_current {
                            format!("{} {}", style::branch_item, style::current)
                        } else {
                            style::branch_item.to_string()
                        };

                        view! {
                            <div class=branch_item_class>
                                <div class=style::branch_info on:click={
                                    let branch_id = branch_id.clone();
                                    move |_| switch_branch(branch_id.clone())
                                }>
                                    <div class=style::branch_name>
                                        {if branch.is_current { "● " } else { "○ " }}
                                        {branch.name.clone()}
                                    </div>

                                    <div class=style::branch_details>
                                        {if let Some(ref parent) = branch.parent {
                                            view! {
                                                <div class=style::branch_parent>
                                                    {format!("↳ from {}", parent)}
                                                    {if let Some(tick) = branch.fork_tick {
                                                        format!(" @ tick {}", tick)
                                                    } else {
                                                        String::new()
                                                    }}
                                                </div>
                                            }.into_any()
                                        } else {
                                            view! {
                                                <div class=style::branch_parent>"Main branch"</div>
                                            }.into_any()
                                        }}

                                        {if let Some(ref scenario) = branch.scenario {
                                            view! {
                                                <div class=style::branch_scenario>
                                                    "💾 Saved as: "
                                                    <span class=style::scenario_name>{scenario.clone()}</span>
                                                </div>
                                            }.into_any()
                                        } else {
                                            view! {}.into_any()
                                        }}

                                        <div class=style::branch_meta>
                                            {format!("{} commits • {}", branch.commit_count, branch.last_modified)}
                                        </div>
                                    </div>
                                </div>

                                <div class=style::branch_actions>
                                    {if !branch.is_current && branch.scenario.is_none() {
                                        view! {
                                            <button
                                                class=format!("{} {} {}", style::btn, style::btn_sm, style::btn_success)
                                                on:click={
                                                    let branch_id = branch_id.clone();
                                                    let start_commit = start_commit.clone();
                                                    move |ev| {
                                                        ev.stop_propagation();
                                                        start_commit(branch_id.clone());
                                                    }
                                                }
                                                title="Save as scenario"
                                            >
                                                "💾"
                                            </button>
                                        }.into_any()
                                    } else {
                                        view! {}.into_any()
                                    }}

                                    {if !branch.is_current && branch.id != "main" {
                                        view! {
                                            <button
                                                class=format!("{} {} {}", style::btn, style::btn_sm, style::btn_danger)
                                                on:click={
                                                    let branch_id = branch_id.clone();
                                                    let delete_branch = delete_branch.clone();
                                                    move |ev| {
                                                        ev.stop_propagation();
                                                        delete_branch(branch_id.clone());
                                                    }
                                                }
                                                title="Delete branch"
                                            >
                                                "🗑"
                                            </button>
                                        }.into_any()
                                    } else {
                                        view! {}.into_any()
                                    }}
                                </div>
                            </div>
                        }
                    }).collect::<Vec<_>>()
                }}
            </div>

            {move || if show_commit_dialog.get() {
                view! {
                    <div class=style::modal_overlay>
                        <div class=style::modal>
                            <div class=style::modal_header>
                                <h4>"Save Branch as Scenario"</h4>
                            </div>
                            <div class=style::modal_body>
                                <p>"Choose a name for your scenario:"</p>
                                <input
                                    type="text"
                                    class=style::scenario_name_input
                                    placeholder="scenario-name"
                                    prop:value=move || commit_name.get()
                                    on:input=move |ev| {
                                        set_commit_name.set(event_target_value(&ev));
                                    }
                                />
                            </div>
                            <div class=style::modal_actions>
                                <button
                                    class=format!("{} {}", style::btn, style::btn_secondary)
                                    on:click=move |_| cancel_commit(())
                                >
                                    "Cancel"
                                </button>
                                <button
                                    class=format!("{} {}", style::btn, style::btn_primary)
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
                view! {}.into_any()
            }}
        </div>
    }
}

fn get_mock_branches() -> Vec<Branch> {
    vec![
        Branch {
            id: "main".to_string(),
            name: "main".to_string(),
            is_current: true,
            parent: None,
            fork_tick: None,
            scenario: Some("dkd-basic.toml".to_string()),
            last_modified: "2 hours ago".to_string(),
            commit_count: 15,
        },
        Branch {
            id: "experiment-1".to_string(),
            name: "experiment-1".to_string(),
            is_current: false,
            parent: Some("main".to_string()),
            fork_tick: Some(100),
            scenario: None,
            last_modified: "30 minutes ago".to_string(),
            commit_count: 3,
        },
        Branch {
            id: "byzantine-test".to_string(),
            name: "byzantine-test".to_string(),
            is_current: false,
            parent: Some("main".to_string()),
            fork_tick: Some(50),
            scenario: Some("byzantine-scenario.toml".to_string()),
            last_modified: "1 hour ago".to_string(),
            commit_count: 7,
        },
    ]
}
