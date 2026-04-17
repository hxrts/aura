use super::notification_actions::NotificationActionBar;
use super::*;

#[allow(non_snake_case)]
pub(super) fn NotificationsScreen(
    model: &UiModel,
    runtime: &NotificationsRuntimeView,
    controller: Arc<UiController>,
    mut render_tick: Signal<u64>,
) -> Element {
    let selected = runtime
        .items
        .get(model.selected_notification_index().unwrap_or_default())
        .cloned();
    rsx! {
        div {
            class: "grid w-full gap-3 lg:grid-cols-12 lg:h-full lg:min-h-0 lg:[grid-template-rows:minmax(0,1fr)]",
            UiCard {
                title: "Notifications".to_string(),
                subtitle: Some("Runtime events".to_string()),
                extra_class: Some("lg:col-span-4".to_string()),
                if runtime.items.is_empty() {
                    Empty {
                        class: Some("h-full border-0 bg-background".to_string()),
                        EmptyHeader {
                            EmptyTitle { "No notifications" }
                            EmptyDescription { "Activity from the peer network will appear here." }
                        }
                    }
                } else {
                    ScrollArea {
                        class: Some("flex-1 lg:min-h-0 pr-1".to_string()),
                        ScrollAreaViewport {
                            class: Some("aura-list space-y-2".to_string()),
                            for (idx, entry) in runtime.items.iter().enumerate() {
                                button {
                                    r#type: "button",
                                    id: list_item_dom_id(ListId::Notifications, &entry.id),
                                    class: "block w-full text-left",
                                    onclick: {
                                        let controller = controller.clone();
                                        let item_count = runtime.items.len();
                                        move |_| {
                                            controller.set_selected_notification_index(idx, item_count);
                                            render_tick.set(render_tick() + 1);
                                        }
                                    },
                                    UiListItem {
                                        label: entry.title.clone(),
                                        secondary: Some(entry.kind_label.clone()),
                                        active: model.selected_notification_index() == Some(idx),
                                    }
                                }
                            }
                        }
                    }
                }
            }
            UiCard {
                title: "Details".to_string(),
                subtitle: Some("Selected notification".to_string()),
                extra_class: Some("lg:col-span-8".to_string()),
                if let Some(item) = selected {
                    UiCardBody {
                        extra_class: Some("gap-2".to_string()),
                        UiListItem {
                            label: item.kind_label.clone(),
                            secondary: Some(item.title.clone()),
                            active: false,
                        }
                        UiListItem {
                            label: item.subtitle.clone(),
                            secondary: Some(item.detail.clone()),
                            active: false,
                        }
                        UiCardFooter {
                            extra_class: None,
                            div {
                                class: "flex h-full w-full items-end justify-end gap-2 overflow-x-auto",
                                NotificationActionBar {
                                    action: item.action.clone(),
                                    item_id: item.id,
                                    controller: controller.clone(),
                                    render_tick,
                                }
                                UiButton {
                                    label: "Dismiss".to_string(),
                                    variant: ButtonVariant::Secondary,
                                    onclick: move |_| {
                                        controller.dismiss_selected_notification();
                                        render_tick.set(render_tick() + 1);
                                    },
                                }
                            }
                        }
                    }
                } else {
                    Empty {
                        class: Some("h-full border-0 bg-background".to_string()),
                        EmptyHeader {
                            EmptyTitle { "No notification selected" }
                            EmptyDescription { "Select an item from the list to view details." }
                        }
                    }
                }
            }
        }
    }
}
