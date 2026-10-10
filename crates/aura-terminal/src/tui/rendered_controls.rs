//! Renderer-local observation only. Root and clipping hooks bracket actual
//! iocraft draw traversal; semantic publication happens after the root draw.
use aura_app::ui::contract::{
    ControlId, ControlRenderCapture, ControlRenderSource, RenderedControlLayout,
    RenderedControlObservation, UiSnapshot,
};
use iocraft::{ComponentDrawer, Hook};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy)]
struct Bounds {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}
impl Bounds {
    fn from_drawer(drawer: &ComponentDrawer<'_>) -> Self {
        let p = drawer.canvas_position();
        let s = drawer.size();
        Self {
            left: i32::from(p.x),
            top: i32::from(p.y),
            right: i32::from(p.x) + i32::from(s.width),
            bottom: i32::from(p.y) + i32::from(s.height),
        }
    }
    fn intersect(self, other: Self) -> Self {
        Self {
            left: self.left.max(other.left),
            top: self.top.max(other.top),
            right: self.right.min(other.right),
            bottom: self.bottom.min(other.bottom),
        }
    }
}
struct DrawFrame {
    snapshot: UiSnapshot,
    capture_revision: u64,
    clips: Vec<Bounds>,
    controls: Vec<(ControlRenderCapture, RenderedControlLayout)>,
}
/// One synchronous canvas traversal. This is an observed frame, not an async
/// task owner, business capability, or a cache of authoritative application state.
#[derive(Clone)]
pub(super) struct RenderedControlFrame(Arc<Mutex<DrawFrame>>);
impl RenderedControlFrame {
    pub(super) fn new(snapshot: UiSnapshot) -> Self {
        Self(Arc::new(Mutex::new(DrawFrame {
            snapshot,
            capture_revision: 0,
            clips: Vec::new(),
            controls: Vec::new(),
        })))
    }
}
pub(super) enum DrawObservationRole {
    Root {
        viewport: Option<(u16, u16)>,
        publish: Arc<dyn Fn(UiSnapshot, Vec<RenderedControlObservation>) + Send + Sync>,
    },
    /// Attach only at an actual overflow-hidden renderer boundary. All actual
    /// clipping ancestors on the observed control's path must participate.
    Clip,
    PassThrough,
    Control {
        id: ControlId,
        rendered: bool,
    },
}
pub(super) struct DrawObservationHook {
    pub frame: RenderedControlFrame,
    pub role: DrawObservationRole,
    capture_revision: u64,
    layout: Arc<Mutex<Option<Bounds>>>,
}
impl Hook for DrawObservationHook {
    fn pre_component_draw(&mut self, drawer: &mut ComponentDrawer<'_>) {
        let Ok(mut frame) = self.frame.0.lock() else {
            return;
        };
        match &self.role {
            DrawObservationRole::Root { viewport, .. } => {
                let Some(next_capture) = self.capture_revision.checked_add(1) else {
                    frame.capture_revision = 0;
                    frame.clips.clear();
                    frame.controls.clear();
                    return;
                };
                self.capture_revision = next_capture;
                frame.capture_revision = self.capture_revision;
                frame.clips.clear();
                frame.controls.clear();
                let mut bounds = Bounds::from_drawer(drawer);
                if let Some((width, height)) = viewport {
                    bounds = bounds.intersect(Bounds {
                        left: 0,
                        top: 0,
                        right: i32::from(*width),
                        bottom: i32::from(*height),
                    });
                }
                frame.clips.push(bounds);
            }
            DrawObservationRole::Clip => {
                let Some(parent) = frame.clips.last().copied() else {
                    return;
                };
                // Mirror the selected native renderer's child clip exactly:
                // effective overflow is per axis, and borders exclude children.
                let overflow = drawer.style().overflow;
                let layout = drawer.layout();
                let position = drawer.canvas_position();
                let size = drawer.size();
                let mut clip = parent;
                if overflow.x != iocraft::Overflow::Visible {
                    clip.left = clip
                        .left
                        .max(i32::from((position.x + layout.border.left as i16).max(0)));
                    clip.right = clip.right.min(i32::from(
                        (position.x + size.width as i16 - layout.border.right as i16).max(0),
                    ));
                }
                if overflow.y != iocraft::Overflow::Visible {
                    clip.top = clip
                        .top
                        .max(i32::from((position.y + layout.border.top as i16).max(0)));
                    clip.bottom = clip.bottom.min(i32::from(
                        (position.y + size.height as i16 - layout.border.bottom as i16).max(0),
                    ));
                }
                frame.clips.push(clip);
            }
            DrawObservationRole::Control { .. } => {
                if let Ok(mut layout) = self.layout.lock() {
                    *layout = None;
                }
            }
            DrawObservationRole::PassThrough => {}
        }
    }
    fn post_component_draw(&mut self, _drawer: &mut ComponentDrawer<'_>) {
        let Ok(mut frame) = self.frame.0.lock() else {
            return;
        };
        match &self.role {
            DrawObservationRole::Control { id, rendered } => {
                // A transparent wrapper's immediate child can be an unattached
                // zero-size node. Only its explicitly registered native root
                // may supply layout; painted descendants are not substitutes.
                let bounds = self
                    .layout
                    .lock()
                    .ok()
                    .and_then(|mut layout| layout.take())
                    .unwrap_or(Bounds {
                        left: 0,
                        top: 0,
                        right: 0,
                        bottom: 0,
                    });
                let capture = ControlRenderCapture::new(
                    *id,
                    ControlRenderSource::from_snapshot(&frame.snapshot),
                    frame.capture_revision,
                );
                frame.controls.push((
                    capture,
                    RenderedControlLayout {
                        connected: true,
                        hidden: !rendered,
                        width: f64::from((bounds.right - bounds.left).max(0)),
                        height: f64::from((bounds.bottom - bounds.top).max(0)),
                    },
                ));
            }
            DrawObservationRole::Clip => {
                frame.clips.pop();
            }
            DrawObservationRole::PassThrough => {}
            DrawObservationRole::Root { publish, .. } => {
                // Actual root post-draw follows all descendants' canvas writes.
                let mut snapshot = frame.snapshot.clone();
                if frame.capture_revision == 0 {
                    snapshot.rendered_control_frame =
                        aura_app::ui::contract::RenderedControlFrame::Unavailable;
                    frame.controls.clear();
                    frame.clips.clear();
                    let publish = publish.clone();
                    drop(frame);
                    publish(snapshot, Vec::new());
                    return;
                }
                let source = ControlRenderSource::from_snapshot(&snapshot);
                snapshot.rendered_control_frame =
                    aura_app::ui::contract::RenderedControlFrame::Committed {
                        capture_revision: frame.capture_revision,
                        layout_revision: frame.capture_revision,
                        source: source.clone(),
                    };
                let observations = frame
                    .controls
                    .drain(..)
                    .map(|(mut capture, layout)| {
                        capture.acknowledge_renderer_commit(&source);
                        capture.observe(&snapshot, layout)
                    })
                    .collect();
                frame.clips.clear();
                let publish = publish.clone();
                drop(frame);
                publish(snapshot, observations);
            }
        }
    }
}

#[derive(Clone)]
pub(super) struct RenderedFrameContext(pub Option<RenderedControlFrame>);

struct OptionalDrawObservationHook(Option<DrawObservationHook>);
impl Hook for OptionalDrawObservationHook {
    fn pre_component_draw(&mut self, drawer: &mut ComponentDrawer<'_>) {
        if let Some(hook) = self.0.as_mut() {
            hook.pre_component_draw(drawer);
        }
    }
    fn post_component_draw(&mut self, drawer: &mut ComponentDrawer<'_>) {
        if let Some(hook) = self.0.as_mut() {
            hook.post_component_draw(drawer);
        }
    }
}

pub(super) fn observe_draw(
    hooks: &mut iocraft::Hooks<'_, '_>,
    frame: Option<RenderedControlFrame>,
    role: DrawObservationRole,
) -> ControlLayoutRegistration {
    let hook = hooks.use_hook(|| OptionalDrawObservationHook(None));
    match (hook.0.as_mut(), frame) {
        (Some(existing), Some(frame)) => {
            existing.frame = frame;
            existing.role = role;
        }
        (_, Some(frame)) => {
            hook.0 = Some(DrawObservationHook {
                frame,
                role,
                capture_revision: 0,
                layout: Arc::new(Mutex::new(None)),
            });
        }
        (_, None) => hook.0 = None,
    }
    ControlLayoutRegistration(hook.0.as_ref().and_then(|hook| {
        matches!(hook.role, DrawObservationRole::Control { .. }).then(|| hook.layout.clone())
    }))
}

pub(super) fn observe_context_draw(
    hooks: &mut iocraft::Hooks<'_, '_>,
    role: DrawObservationRole,
) -> ControlLayoutRegistration {
    use iocraft::hooks::UseContext;
    let frame = hooks
        .try_use_context::<RenderedFrameContext>()
        .and_then(|context| context.0.clone());
    observe_draw(hooks, frame, role)
}

/// Traversal-local observation ticket, issued by the original component hook.
/// It carries no operation authority and cannot publish a snapshot itself.
#[derive(Clone, Default)]
pub(crate) struct ControlLayoutRegistration(Option<Arc<Mutex<Option<Bounds>>>>);

struct NativeRootMeasurement {
    frame: Option<RenderedControlFrame>,
    registrations: Vec<ControlLayoutRegistration>,
}
impl Hook for NativeRootMeasurement {
    fn pre_component_draw(&mut self, drawer: &mut ComponentDrawer<'_>) {
        let Some(frame) = self.frame.as_ref() else {
            return;
        };
        let Some(clip) = frame
            .0
            .lock()
            .ok()
            .and_then(|frame| frame.clips.last().copied())
        else {
            return;
        };
        let bounds = clip.intersect(Bounds::from_drawer(drawer));
        for registration in &self.registrations {
            if let Some(layout) = registration.0.as_ref() {
                if let Ok(mut layout) = layout.lock() {
                    *layout = Some(bounds);
                }
            }
        }
    }
}
fn measure_native_root(
    hooks: &mut iocraft::Hooks<'_, '_>,
    registrations: &[ControlLayoutRegistration],
    frame: Option<RenderedControlFrame>,
) {
    let hook = hooks.use_hook(|| NativeRootMeasurement {
        frame: None,
        registrations: Vec::new(),
    });
    hook.frame = frame;
    hook.registrations = registrations.to_vec();
}

#[derive(Default, iocraft::Props)]
pub(super) struct RegisteredViewProps<'a> {
    view: iocraft::components::ViewProps<'a>,
    registrations: Vec<ControlLayoutRegistration>,
}
pub(super) struct RegisteredView(ObservedClipView);
impl iocraft::Component for RegisteredView {
    type Props<'a> = RegisteredViewProps<'a>;
    fn new(props: &Self::Props<'_>) -> Self {
        Self(<ObservedClipView as iocraft::Component>::new(&props.view))
    }
    fn update(
        &mut self,
        props: &mut Self::Props<'_>,
        mut hooks: iocraft::Hooks,
        updater: &mut iocraft::ComponentUpdater,
    ) {
        let frame = updater
            .component_context_stack()
            .get_context::<RenderedFrameContext>()
            .and_then(|context| context.0.clone());
        measure_native_root(&mut hooks, &props.registrations, frame);
        <ObservedClipView as iocraft::Component>::update(
            &mut self.0,
            &mut props.view,
            hooks,
            updater,
        );
    }
    fn draw(&mut self, drawer: &mut ComponentDrawer<'_>) {
        <ObservedClipView as iocraft::Component>::draw(&mut self.0, drawer);
    }
}
/// Replace the component implementation on the exact selected View element;
/// preserve its original key, props, children, layout and native draw operation.
pub(super) fn register_native_view<'a>(
    view: iocraft::Element<'a, iocraft::components::View>,
    registrations: Vec<ControlLayoutRegistration>,
) -> iocraft::Element<'a, RegisteredView> {
    iocraft::Element {
        key: view.key,
        props: RegisteredViewProps {
            view: view.props,
            registrations,
        },
    }
}

#[derive(Default, iocraft::Props)]
pub(super) struct RegisteredTextProps {
    text: iocraft::components::TextProps,
    registrations: Vec<ControlLayoutRegistration>,
}
pub(super) struct RegisteredText(iocraft::components::Text);
impl iocraft::Component for RegisteredText {
    type Props<'a> = RegisteredTextProps;
    fn new(props: &Self::Props<'_>) -> Self {
        Self(<iocraft::components::Text as iocraft::Component>::new(
            &props.text,
        ))
    }
    fn update(
        &mut self,
        props: &mut Self::Props<'_>,
        mut hooks: iocraft::Hooks,
        updater: &mut iocraft::ComponentUpdater,
    ) {
        let frame = updater
            .component_context_stack()
            .get_context::<RenderedFrameContext>()
            .and_then(|context| context.0.clone());
        measure_native_root(&mut hooks, &props.registrations, frame);
        <iocraft::components::Text as iocraft::Component>::update(
            &mut self.0,
            &mut props.text,
            hooks,
            updater,
        );
    }
    fn draw(&mut self, drawer: &mut ComponentDrawer<'_>) {
        <iocraft::components::Text as iocraft::Component>::draw(&mut self.0, drawer);
    }
}
pub(super) fn register_native_text<'a>(
    text: iocraft::Element<'a, iocraft::components::Text>,
    registrations: Vec<ControlLayoutRegistration>,
) -> iocraft::Element<'a, RegisteredText> {
    iocraft::Element {
        key: text.key,
        props: RegisteredTextProps {
            text: text.props,
            registrations,
        },
    }
}

#[derive(Default, iocraft::Props)]
pub(super) struct RenderedTerminalRootProps<'a> {
    pub frame: Option<RenderedControlFrame>,
    pub children: Vec<iocraft::AnyElement<'a>>,
}

#[iocraft::component]
pub(super) fn RenderedTerminalRoot<'a>(
    props: &mut RenderedTerminalRootProps<'a>,
    mut hooks: iocraft::Hooks,
) -> impl Into<iocraft::AnyElement<'a>> {
    use crate::tui::layout::dim;
    use iocraft::prelude::*;
    let viewport = hooks.use_terminal_size();
    observe_draw(
        &mut hooks,
        props.frame.clone(),
        DrawObservationRole::Root {
            viewport: Some(viewport),
            publish: Arc::new(|mut snapshot, observations| {
                snapshot.rendered_controls = observations;
                if let Err(error) =
                    crate::tui::harness_state::publish_rendered_ui_snapshot(&snapshot)
                {
                    tracing::warn!(error = %error, "failed to publish rendered TUI snapshot");
                }
            }),
        },
    );
    element! {
        View(flex_direction: FlexDirection::Column, width: dim::TOTAL_WIDTH, height: dim::TOTAL_HEIGHT, overflow: Overflow::Hidden) {
            ContextProvider(value: Context::owned(RenderedFrameContext(props.frame.clone()))) {
                #(&mut props.children)
            }
        }
    }
}

/// Actual iocraft View with identical props/layout/drawing and an observed
/// clipping boundary. No extra renderer node or independent layout is inserted.
pub(super) struct ObservedClipView(iocraft::components::View);
impl iocraft::Component for ObservedClipView {
    type Props<'a> = iocraft::components::ViewProps<'a>;
    fn new(props: &Self::Props<'_>) -> Self {
        Self(<iocraft::components::View as iocraft::Component>::new(
            props,
        ))
    }
    fn update(
        &mut self,
        props: &mut Self::Props<'_>,
        mut hooks: iocraft::Hooks,
        updater: &mut iocraft::ComponentUpdater,
    ) {
        // Handwritten native components receive raw Hooks without a context
        // stack; clone the actual inherited frame from the updater explicitly.
        let frame = updater
            .component_context_stack()
            .get_context::<RenderedFrameContext>()
            .and_then(|context| context.0.clone());
        observe_draw(&mut hooks, frame, DrawObservationRole::Clip);
        <iocraft::components::View as iocraft::Component>::update(
            &mut self.0,
            props,
            hooks,
            updater,
        );
    }
    fn draw(&mut self, drawer: &mut iocraft::ComponentDrawer<'_>) {
        <iocraft::components::View as iocraft::Component>::draw(&mut self.0, drawer);
    }
}

#[cfg(test)]
pub(crate) mod rendered_tests {
    use super::*;
    use aura_app::ui::contract::{ControlLayoutState, ScreenId};
    use iocraft::prelude::*;
    #[derive(Default, Props)]
    struct FixtureProps<'a> {
        frame: Option<RenderedControlFrame>,
        output: Arc<Mutex<Vec<RenderedControlObservation>>>,
        height: u16,
        terminal_viewport: bool,
        children: Vec<AnyElement<'a>>,
    }
    #[component]
    fn Fixture<'a>(props: &mut FixtureProps<'a>, mut hooks: Hooks) -> impl Into<AnyElement<'a>> {
        let terminal_viewport = hooks.use_terminal_size();
        if props.terminal_viewport && terminal_viewport == (1, 1) {
            hooks.use_context_mut::<SystemContext>().exit();
        }
        let output = props.output.clone();
        observe_draw(
            &mut hooks,
            props.frame.clone(),
            DrawObservationRole::Root {
                viewport: props.terminal_viewport.then_some(terminal_viewport),
                publish: Arc::new(move |_, observations| {
                    *output.lock().expect("capture") = observations;
                }),
            },
        );
        element! { View(width: 80, height: props.height, overflow: Overflow::Hidden) {
            ContextProvider(value: Context::owned(RenderedFrameContext(props.frame.clone()))) { #(&mut props.children) }
        }}
    }
    pub(crate) fn observe_test_layout(
        children: Vec<AnyElement<'static>>,
    ) -> (String, Vec<RenderedControlObservation>) {
        let output = Arc::new(Mutex::new(Vec::new()));
        let frame = RenderedControlFrame::new(UiSnapshot::loading(ScreenId::Settings));
        let mut element = element! { Fixture(frame: Some(frame), output: output.clone(), height: 31u16) { #(children) } };
        let canvas = element.render(None).to_string();
        let observations = output.lock().expect("capture").clone();
        (canvas, observations)
    }
    fn account_layout(visible: bool, height: u16) -> Vec<RenderedControlObservation> {
        let output = Arc::new(Mutex::new(Vec::new()));
        let frame = RenderedControlFrame::new(UiSnapshot::loading(ScreenId::Onboarding));
        let mut element = element! { Fixture(frame: Some(frame), output: output.clone(), height) {
            crate::tui::components::ModalFrame {
                crate::tui::components::AccountSetupModal(visible)
            }
        }};
        let _canvas = element.render(None);
        let observations = output
            .lock()
            .expect("capture")
            .iter()
            .filter(|observation| observation.control == ControlId::OnboardingRoot)
            .cloned()
            .collect();
        observations
    }
    #[test]
    fn actual_account_component_visible_false_cannot_publish_visible() {
        let observations = account_layout(false, 31);
        assert_eq!(observations.len(), 1);
        assert_eq!(observations[0].state, ControlLayoutState::Hidden);
    }
    #[test]
    fn actual_modal_clipped_outside_root_has_zero_extent() {
        let observations = account_layout(true, 1);
        assert_eq!(observations.len(), 1);
        assert_eq!(observations[0].state, ControlLayoutState::ZeroExtent);
    }
    #[test]
    fn removed_account_component_cannot_reuse_prior_canvas_observation() {
        let output = Arc::new(Mutex::new(Vec::new()));
        let frame = RenderedControlFrame::new(UiSnapshot::loading(ScreenId::Onboarding));
        let mut element = element! { Fixture(frame: Some(frame), output: output.clone(), height: 31u16) {
            crate::tui::components::ModalFrame { crate::tui::components::AccountSetupModal(visible: true) }
        }};
        let _ = element.render(None);
        assert!(output
            .lock()
            .expect("capture")
            .iter()
            .any(|observation| observation.control == ControlId::OnboardingRoot));
        element.props.children.clear();
        let _ = element.render(None);
        assert!(output.lock().expect("capture").is_empty());
    }
    #[test]
    fn actual_account_component_positive_follows_complete_canvas_draw() {
        let observations = account_layout(true, 31);
        assert_eq!(observations.len(), 1);
        assert_eq!(observations[0].state, ControlLayoutState::Visible);
        assert!(observations[0].capture_revision > 0);
    }

    #[component]
    fn UninstrumentedControl(mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
        observe_context_draw(
            &mut hooks,
            DrawObservationRole::Control {
                id: ControlId::OnboardingRoot,
                rendered: true,
            },
        );
        // A painted descendant must not substitute for this unregistered root.
        element! { View { ObservedClipView { Text(content: "painted descendant") } } }
    }
    #[test]
    fn unregistered_root_cannot_borrow_painted_descendant_layout() {
        let (canvas, observations) =
            observe_test_layout(vec![element! { UninstrumentedControl }.into_any()]);
        assert!(canvas.contains("painted descendant"));
        let control = observations
            .iter()
            .find(|o| o.control == ControlId::OnboardingRoot)
            .expect("registration");
        assert_eq!(control.state, ControlLayoutState::ZeroExtent);
    }
    #[test]
    fn actual_message_input_root_supplies_visible_field_layout() {
        let (canvas, observations) = observe_test_layout(vec![element! {
            crate::tui::components::MessageInput(value: "actual field".to_string())
        }
        .into_any()]);
        assert!(canvas.contains("actual field"));
        let control = observations
            .iter()
            .find(|o| o.control == ControlId::Field(aura_app::ui::contract::FieldId::ChatInput))
            .expect("actual field");
        assert_eq!(control.state, ControlLayoutState::Visible);
    }
    #[test]
    fn native_root_adapter_preserves_original_key_props_and_canvas() {
        let mut original = element! { View(key: "original-native-root", width: 20, height: 3,
            overflow: Overflow::Hidden, border_style: BorderStyle::Single) {
            Text(content: "same child")
        }};
        let key = original.key.clone();
        let expected = original.render(None).to_string();
        let mut registered = register_native_view(original, Vec::new());
        assert_eq!(registered.key, key);
        assert_eq!(registered.render(None).to_string(), expected);
    }

    #[derive(Default, Props)]
    struct PositionedRegisteredControlProps {
        outside: bool,
        left: i32,
        top: i32,
    }
    #[component]
    fn PositionedRegisteredControl(
        props: &PositionedRegisteredControlProps,
        mut hooks: Hooks,
    ) -> impl Into<AnyElement<'static>> {
        let id = if props.outside {
            ControlId::OnboardingRoot
        } else {
            ControlId::Field(aura_app::ui::contract::FieldId::ChatInput)
        };
        let registration = observe_context_draw(
            &mut hooks,
            DrawObservationRole::Control { id, rendered: true },
        );
        register_native_view(
            element! { View(position: Position::Absolute, top: if props.outside { 5 } else { props.top }, left: props.left, width: 8, height: 1) {
                Text(content: if props.outside { "outside" } else { "inside" })
            }},
            vec![registration],
        )
    }
    #[test]
    fn native_adapter_inherits_frame_and_actual_nonzero_ancestor_clip() {
        let (canvas, observations) = observe_test_layout(vec![element! {
            ObservedClipView(width: 12, height: 2, overflow: Overflow::Hidden) {
                PositionedRegisteredControl(outside: false)
                PositionedRegisteredControl(outside: true)
            }
        }
        .into_any()]);
        assert!(canvas.contains("inside"));
        assert!(!canvas.contains("outside"));
        let inside = observations
            .iter()
            .find(|o| o.control == ControlId::Field(aura_app::ui::contract::FieldId::ChatInput))
            .expect("inside original ticket");
        let outside = observations
            .iter()
            .find(|o| o.control == ControlId::OnboardingRoot)
            .expect("outside original ticket");
        assert_eq!(inside.state, ControlLayoutState::Visible);
        assert_eq!(outside.state, ControlLayoutState::ZeroExtent);
    }

    #[test]
    fn native_clip_matches_effective_overflow_x_without_clipping_visible_y() {
        let (canvas, observations) = observe_test_layout(vec![element! {
            ObservedClipView(width: 12, height: 2, overflow_x: Overflow::Hidden, overflow_y: Overflow::Visible) {
                PositionedRegisteredControl(outside: true)
                PositionedRegisteredControl(left: 20)
            }
        }.into_any()]);
        assert!(canvas.contains("outside"));
        assert!(!canvas.contains("inside"));
        assert_eq!(
            observations
                .iter()
                .find(|o| o.control == ControlId::OnboardingRoot)
                .expect("vertical overflow ticket")
                .state,
            ControlLayoutState::Visible
        );
        assert_eq!(
            observations
                .iter()
                .find(|o| o.control == ControlId::Field(aura_app::ui::contract::FieldId::ChatInput))
                .expect("horizontal clipped ticket")
                .state,
            ControlLayoutState::ZeroExtent
        );
    }
    #[test]
    fn native_clip_excludes_actual_border_from_child_extent() {
        let (canvas, observations) = observe_test_layout(vec![element! {
            ObservedClipView(width: 12, height: 4, overflow: Overflow::Hidden, border_style: BorderStyle::Single) {
                PositionedRegisteredControl(top: -1, left: -1)
            }
        }.into_any()]);
        assert!(!canvas.contains("inside"));
        assert_eq!(
            observations
                .iter()
                .find(|o| o.control == ControlId::Field(aura_app::ui::contract::FieldId::ChatInput))
                .expect("border clipped original ticket")
                .state,
            ControlLayoutState::ZeroExtent
        );
    }

    #[test]
    fn actual_nested_view_clips_registered_field_without_extra_layout_node() {
        let output = Arc::new(Mutex::new(Vec::new()));
        let frame = RenderedControlFrame::new(UiSnapshot::loading(ScreenId::Chat));
        let mut element = element! { Fixture(frame: Some(frame), output: output.clone(), height: 31u16) {
            ObservedClipView(width: 8, height: 0, overflow: Overflow::Hidden) {
                crate::tui::components::MessageInput(value: "content".to_string())
            }
        }};
        let _ = element.render(None);
        let observations = output.lock().expect("capture");
        let input = observations
            .iter()
            .find(|observation| {
                observation.control == ControlId::Field(aura_app::ui::contract::FieldId::ChatInput)
            })
            .expect("actual input observation");
        assert_eq!(input.state, ControlLayoutState::ZeroExtent);
    }

    #[test]
    fn actual_shared_authority_selection_publishes_typed_list_and_modal_layout() {
        let output = Arc::new(Mutex::new(Vec::new()));
        let frame = RenderedControlFrame::new(UiSnapshot::loading(ScreenId::Settings));
        let mut element = element! { Fixture(frame: Some(frame), output: output.clone(), height: 31u16) {
            crate::tui::components::SelectionModal(visible: true, title: "Select Authority".to_string(),
                rows: vec![crate::tui::components::SelectionModalRow { id: "retained-authority".into(), label: "Retained authority".into() }],
                list_id: Some(aura_app::ui::contract::ListId::Authorities),
                modal_id: Some(aura_app::ui::contract::ModalId::SwitchAuthority),
            )
        }};
        let canvas = element.render(None).to_string();
        assert!(canvas.contains("Retained authority"));
        let observations = output.lock().expect("capture");
        for expected in [
            ControlId::List(aura_app::ui::contract::ListId::Authorities),
            ControlId::Modal(aura_app::ui::contract::ModalId::SwitchAuthority),
        ] {
            assert!(observations
                .iter()
                .any(|observation| observation.control == expected
                    && observation.state == ControlLayoutState::Visible));
        }
    }

    #[test]
    fn actual_terminal_resize_clips_fixed_layout_to_reported_viewport() {
        use futures::StreamExt;
        let output = Arc::new(Mutex::new(Vec::new()));
        let frame = RenderedControlFrame::new(UiSnapshot::loading(ScreenId::Onboarding));
        let mut element = element! { Fixture(frame: Some(frame), output: output.clone(), height: 31u16, terminal_viewport: true) {
            crate::tui::components::ModalFrame { crate::tui::components::AccountSetupModal(visible: true) }
        }};
        let canvases = futures::executor::block_on(
            element
                .mock_terminal_render_loop(MockTerminalConfig::with_events(futures::stream::iter(
                    vec![TerminalEvent::Resize(1, 1)],
                )))
                .collect::<Vec<_>>(),
        );
        assert!(!canvases.is_empty());
        let observations = output.lock().expect("capture");
        let root = observations
            .iter()
            .find(|observation| observation.control == ControlId::OnboardingRoot)
            .expect("actual onboarding root");
        assert_eq!(root.state, ControlLayoutState::ZeroExtent);
    }
}
