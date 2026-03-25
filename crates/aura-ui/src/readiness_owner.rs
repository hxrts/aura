use aura_app::ui::contract::UiReadiness;

use crate::model::ScreenId;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ScreenProjectionReadiness {
    pub neighborhood_loaded: bool,
    pub neighborhood_home_bound: bool,
    pub chat_loaded: bool,
    pub contacts_loaded: bool,
    pub settings_loaded: bool,
    pub settings_profile_bound: bool,
    pub settings_devices_materialized: bool,
    pub settings_authorities_materialized: bool,
    pub notifications_loaded: bool,
}

pub(crate) fn screen_readiness(
    screen: ScreenId,
    projection: ScreenProjectionReadiness,
) -> UiReadiness {
    match screen {
        ScreenId::Onboarding => UiReadiness::Loading,
        ScreenId::Neighborhood => {
            if projection.neighborhood_loaded && projection.neighborhood_home_bound {
                UiReadiness::Ready
            } else {
                UiReadiness::Loading
            }
        }
        ScreenId::Chat => {
            if projection.chat_loaded {
                UiReadiness::Ready
            } else {
                UiReadiness::Loading
            }
        }
        ScreenId::Contacts => {
            if projection.contacts_loaded {
                UiReadiness::Ready
            } else {
                UiReadiness::Loading
            }
        }
        ScreenId::Notifications => {
            if projection.notifications_loaded {
                UiReadiness::Ready
            } else {
                UiReadiness::Loading
            }
        }
        ScreenId::Settings => {
            if projection.settings_loaded
                && projection.settings_profile_bound
                && projection.settings_devices_materialized
                && projection.settings_authorities_materialized
            {
                UiReadiness::Ready
            } else {
                UiReadiness::Loading
            }
        }
    }
}

pub(crate) fn account_gate_readiness(account_ready: bool) -> UiReadiness {
    if account_ready {
        UiReadiness::Ready
    } else {
        UiReadiness::Loading
    }
}
