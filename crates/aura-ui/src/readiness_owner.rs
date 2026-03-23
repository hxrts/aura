use aura_app::ui::contract::UiReadiness;

use crate::model::ScreenId;

pub(crate) fn screen_readiness(screen: ScreenId) -> UiReadiness {
    match screen {
        ScreenId::Onboarding => UiReadiness::Loading,
        ScreenId::Neighborhood
        | ScreenId::Chat
        | ScreenId::Contacts
        | ScreenId::Notifications
        | ScreenId::Settings => UiReadiness::Ready,
    }
}

pub(crate) fn account_gate_readiness(account_ready: bool) -> UiReadiness {
    if account_ready {
        UiReadiness::Ready
    } else {
        UiReadiness::Loading
    }
}
