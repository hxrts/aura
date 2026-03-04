use crate::model::UiController;
use dioxus::prelude::*;
use std::sync::Arc;

#[component]
pub fn AuraUiRoot(controller: Arc<UiController>) -> Element {
    let snapshot = controller.snapshot();

    rsx! {
        main {
            h1 { "Aura Web" }
            p { "Shared UI core: aura-ui" }
            pre { "{snapshot.screen}" }
        }
    }
}
