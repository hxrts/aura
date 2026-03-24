use super::*;

#[test]
fn test_settings_section_navigation() {
    let mut tui = TestTui::new();
    tui.go_to_screen(Screen::Settings);

    assert_eq!(tui.state.settings.section, SettingsSection::Profile);

    tui.send_char('j');
    assert_eq!(tui.state.settings.section, SettingsSection::Threshold);
    tui.send_char('j');
    assert_eq!(tui.state.settings.section, SettingsSection::Recovery);
    tui.send_char('j');
    assert_eq!(tui.state.settings.section, SettingsSection::Devices);
    tui.send_char('j');
    assert_eq!(tui.state.settings.section, SettingsSection::Authority);
    tui.send_char('j');
    assert_eq!(tui.state.settings.section, SettingsSection::Observability);
    tui.send_char('j');
    assert_eq!(tui.state.settings.section, SettingsSection::Profile);
}

#[test]
fn test_settings_section_navigation_up() {
    let mut tui = TestTui::new();
    tui.go_to_screen(Screen::Settings);

    tui.send_char('k');
    assert_eq!(tui.state.settings.section, SettingsSection::Observability);
    tui.send_char('k');
    assert_eq!(tui.state.settings.section, SettingsSection::Authority);
    tui.send_char('k');
    assert_eq!(tui.state.settings.section, SettingsSection::Devices);
}

#[test]
fn test_settings_profile_edit() {
    let mut tui = TestTui::new();
    tui.go_to_screen(Screen::Settings);
    tui.send_enter();
    tui.assert_has_modal();

    tui.type_text("NewNickname");
    tui.clear_commands();
    tui.send_enter();

    tui.assert_dispatch(|d| {
        matches!(
            d,
            DispatchCommand::UpdateNicknameSuggestion { nickname_suggestion }
                if nickname_suggestion == "NewNickname"
        )
    });
}

#[test]
fn test_settings_threshold_modal() {
    let mut tui = TestTui::new();
    tui.go_to_screen(Screen::Settings);
    tui.send_char('j');
    assert_eq!(tui.state.settings.section, SettingsSection::Threshold);
    tui.send_enter();

    if tui.has_modal() {
        tui.send_escape();
        assert!(!tui.has_modal());
    }
}

#[test]
fn test_settings_authority_mfa_hotkey() {
    let mut tui = TestTui::new();
    tui.go_to_screen(Screen::Settings);

    for _ in 0..4 {
        tui.send_char('j');
    }
    assert_eq!(tui.state.settings.section, SettingsSection::Authority);

    tui.clear_commands();
    tui.send_char('m');
    tui.assert_dispatch(|d| matches!(d, DispatchCommand::OpenMfaSetup));
}

#[test]
fn test_settings_device_management() {
    let mut tui = TestTui::new();
    let invitee_authority_id = AuthorityId::new_from_entropy([7u8; 32]);
    tui.go_to_screen(Screen::Settings);

    tui.send_char('j');
    tui.send_char('j');
    tui.send_char('j');
    assert_eq!(tui.state.settings.section, SettingsSection::Devices);

    tui.send_char('a');
    tui.assert_has_modal();
    tui.type_text("My Phone");
    tui.send_tab();
    tui.type_text(&invitee_authority_id.to_string());

    tui.clear_commands();
    tui.send_enter();

    tui.assert_dispatch(|d| {
        matches!(
            d,
            DispatchCommand::AddDevice {
                name,
                invitee_authority_id: actual_invitee_authority_id,
            } if name == "My Phone" && *actual_invitee_authority_id == invitee_authority_id
        )
    });
}

#[test]
fn test_settings_panel_focus() {
    let mut tui = TestTui::new();
    tui.go_to_screen(Screen::Settings);
    tui.send_char('l');
    tui.send_char('h');
}
