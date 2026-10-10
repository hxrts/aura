//! Nav bar component with fixed 2-row layout.
//!
//! The nav bar occupies the top 2 rows of the TUI and contains:
//! - Row 1: Screen tabs (evenly spaced across the width)
//! - Row 2: Bottom border/separator

use crate::tui::layout::dim;
use crate::tui::screens::Screen;
use crate::tui::theme::Theme;
use iocraft::prelude::*;

/// Props for NavBar
#[derive(Default, Props)]
pub struct NavBarProps {
    /// Currently active screen
    pub active_screen: Screen,
}

/// Fixed 2-row nav bar for the TUI.
///
/// Layout:
/// ```text
/// │  Neighborhood    Chat    Contacts    Notifications    Settings  │ Row 1: Tabs
/// ├─────────────────────────────────────────────────────────────────┤ Row 2: Border
/// ```
#[component]
pub fn NavBar(props: &NavBarProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let control_layout_0 = crate::tui::rendered_controls::observe_context_draw(
        &mut hooks,
        crate::tui::rendered_controls::DrawObservationRole::Control {
            id: aura_app::ui::contract::ControlId::NavRoot,
            rendered: true,
        },
    );
    let control_layout_1 = crate::tui::rendered_controls::observe_context_draw(
        &mut hooks,
        crate::tui::rendered_controls::DrawObservationRole::Control {
            id: aura_app::ui::contract::ControlId::List(aura_app::ui::contract::ListId::Navigation),
            rendered: true,
        },
    );
    let active = props.active_screen;

    crate::tui::rendered_controls::register_native_view(
        element! {
            View(
                width: dim::TOTAL_WIDTH,
                height: dim::NAV_HEIGHT,
                flex_direction: FlexDirection::Column,
                overflow: Overflow::Hidden,
            ) {
                // Row 1: Screen tabs evenly spaced
                View(
                    width: 100pct,
                    height: 1,
                    flex_direction: FlexDirection::Row,
                    justify_content: JustifyContent::SpaceEvenly,
                ) {
                    #(Screen::all().iter().map(|&screen| {
                        let is_active = screen == active;
                        let color = if is_active { Theme::PRIMARY } else { Theme::TEXT_MUTED };
                        let weight = if is_active { Weight::Bold } else { Weight::Normal };
                        let title = screen.name().to_string();
                        element! {
                            ObservedNavTab(screen: screen, title: title, color: color, weight: weight)
                        }
                    }))
                }

                // Row 2: Bottom border (1 row)
                View(
                    width: 100pct,
                    height: 1,
                    border_style: BorderStyle::Single,
                    border_edges: Edges::Top,
                    border_color: Theme::BORDER,
                )
            }
        },
        vec![control_layout_0, control_layout_1],
    )
}

#[derive(Props)]
struct ObservedNavTabProps {
    screen: Screen,
    title: String,
    color: Color,
    weight: Weight,
}

impl Default for ObservedNavTabProps {
    fn default() -> Self {
        Self {
            screen: Screen::default(),
            title: String::new(),
            color: Theme::TEXT,
            weight: Weight::Normal,
        }
    }
}

#[component]
fn ObservedNavTab(props: &ObservedNavTabProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let control_layout_0 = crate::tui::rendered_controls::observe_context_draw(
        &mut hooks,
        crate::tui::rendered_controls::DrawObservationRole::Control {
            id: aura_app::ui::contract::nav_control_id_for_screen(props.screen.into()),
            rendered: true,
        },
    );
    crate::tui::rendered_controls::register_native_text(
        element! { Text(content: props.title.clone(), color: props.color, weight: props.weight) },
        vec![control_layout_0],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nav_bar_dimensions() {
        // Nav bar should be exactly 2 rows (tabs + border)
        assert_eq!(dim::NAV_HEIGHT, 2);
    }
}
