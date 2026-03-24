use super::*;

#[test]
fn test_neighborhood_grid_navigation() {
    let mut tui = TestTui::new();
    tui.go_to_screen(Screen::Neighborhood);
    tui.state.neighborhood.grid.set_cols(4);
    tui.state.neighborhood.grid.set_count(12);

    let (initial_col, initial_row) = (
        tui.state.neighborhood.grid.col(),
        tui.state.neighborhood.grid.row(),
    );

    tui.send_char('l');
    assert_eq!(tui.state.neighborhood.grid.col(), initial_col + 1);
    tui.send_char('h');
    assert_eq!(tui.state.neighborhood.grid.col(), initial_col);
    tui.send_char('j');
    assert_eq!(tui.state.neighborhood.grid.row(), initial_row + 1);
    tui.send_char('k');
    assert_eq!(tui.state.neighborhood.grid.row(), initial_row);
}

#[test]
fn test_neighborhood_enter_home() {
    let mut tui = TestTui::new();
    tui.go_to_screen(Screen::Neighborhood);
    tui.state.neighborhood.home_count = 1;
    tui.clear_commands();
    tui.send_enter();
    tui.assert_dispatch(|d| matches!(d, DispatchCommand::EnterHome { .. }));
}

#[test]
fn test_neighborhood_go_home() {
    let mut tui = TestTui::new();
    tui.go_to_screen(Screen::Neighborhood);
    tui.clear_commands();
    tui.send_char('g');
    tui.assert_dispatch(|d| matches!(d, DispatchCommand::GoHome));
}

#[test]
fn test_neighborhood_back_to_limited() {
    let mut tui = TestTui::new();
    tui.go_to_screen(Screen::Neighborhood);
    tui.clear_commands();
    tui.send_char('b');
    tui.assert_dispatch(|d| matches!(d, DispatchCommand::BackToLimited));
}

#[test]
fn test_neighborhood_grid_wraps_around() {
    let mut tui = TestTui::new();
    tui.go_to_screen(Screen::Neighborhood);
    tui.state.neighborhood.grid.set_cols(4);
    tui.state.neighborhood.grid.set_count(12);

    assert_eq!(tui.state.neighborhood.grid.col(), 0);
    assert_eq!(tui.state.neighborhood.grid.row(), 0);

    tui.send_char('h');
    assert_eq!(tui.state.neighborhood.grid.current(), 11);
    tui.send_char('l');
    assert_eq!(tui.state.neighborhood.grid.current(), 0);
    tui.send_char('k');
    assert_eq!(tui.state.neighborhood.grid.row(), 2);
    tui.send_char('j');
    assert_eq!(tui.state.neighborhood.grid.row(), 0);
}
