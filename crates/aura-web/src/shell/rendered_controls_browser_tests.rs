//! Actual dioxus-web renderer fixture. These tests require a browser and cannot
//! be substituted by native VirtualDom mounted-event simulation.
use super::rendered_controls::{ObservedRendererFrame, RenderedOnboardingRoot};
use aura_app::ui::contract::{ControlId, ControlLayoutState, UiSnapshot};
use dioxus::prelude::*;
use futures::{channel::mpsc, StreamExt};
use std::{cell::RefCell, rc::Rc, sync::Arc};
use wasm_bindgen_test::*;
wasm_bindgen_test_configure!(run_in_browser);

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Visible,
    Hidden,
    ZeroExtent,
    Unmounted,
    Duplicate,
    Buttons,
}
#[derive(Clone)]
struct Commands(Rc<RefCell<Option<mpsc::Receiver<Mode>>>>);
impl PartialEq for Commands {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

#[derive(Clone)]
struct FixtureProps {
    controller: Arc<aura_ui::UiController>,
    commands: Commands,
    renders: Rc<std::cell::Cell<u32>>,
}
fn fixture(props: FixtureProps) -> Element {
    props.renders.set(props.renders.get() + 1);
    let snapshot = props.controller.semantic_model_snapshot();
    rsx! { ObservedRendererFrame { snapshot, controller: props.controller.clone(), ChildLocalSurface { commands: props.commands.clone() } } }
}

#[component]
fn ChildLocalSurface(commands: Commands) -> Element {
    let mut mode = use_signal(|| Mode::Visible);
    use_future(move || {
        let receiver = commands.0.borrow_mut().take();
        async move {
            if let Some(mut receiver) = receiver {
                while let Some(next) = receiver.next().await {
                    mode.set(next);
                }
            }
        }
    });
    let style = match mode() {
        Mode::Hidden => "width:200px;height:100px;visibility:hidden;overflow:visible",
        Mode::ZeroExtent => "width:0;height:0;visibility:visible;overflow:hidden",
        _ => "width:200px;height:100px;visibility:visible;overflow:visible",
    };
    rsx! { div { style,
        if mode() != Mode::Unmounted && mode() != Mode::Buttons {
            RenderedOnboardingRoot {
                input { value: match mode() { Mode::Hidden => "hidden", Mode::ZeroExtent => "zero", _ => "visible" } }
            }
        }
        if mode() == Mode::Duplicate { RenderedOnboardingRoot { input { value: "duplicate" } } }
        if mode() == Mode::Buttons {
            for label in ["first", "second"] {
                aura_ui::components::UiButton {
                    control_id: Some(ControlId::ContactsCreateInvitationButton),
                    label: label.to_string(),
                    variant: aura_ui::components::ButtonVariant::Secondary,
                    onclick: move |_| {},
                }
            }
        }
    }}
}
async fn next_root(
    events: &mut mpsc::Receiver<UiSnapshot>,
    state: ControlLayoutState,
    after_capture: u64,
) -> UiSnapshot {
    console_log!("await root state={state:?} after_capture={after_capture}");
    loop {
        let snapshot = events
            .next()
            .await
            .expect("actual renderer snapshot stream");
        console_log!(
            "actual renderer event frame={:?} controls={:?}",
            snapshot.rendered_control_frame,
            snapshot.rendered_controls
        );
        if matches!(&snapshot.rendered_control_frame, aura_app::ui::contract::RenderedControlFrame::Committed { capture_revision, .. } if *capture_revision > after_capture)
            && snapshot.rendered_controls.iter().any(|observation| {
                observation.control == ControlId::OnboardingRoot && observation.state == state
            })
        {
            return snapshot;
        }
    }
}

#[wasm_bindgen_test]
async fn actual_renderer_visible_hidden_zero_unmount_and_same_source_remount() {
    let controller = Arc::new(aura_ui::UiController::new(
        Arc::new(async_lock::RwLock::new(
            aura_app::AppCore::new(aura_app::AppConfig::default()).expect("fixture app"),
        )),
        Arc::new(aura_ui::MemoryClipboard::default()),
    ));
    let (events_tx, mut events_rx) = mpsc::channel(32);
    let events_tx = async_lock::Mutex::new(events_tx);
    controller.set_ui_snapshot_sink(Arc::new(move |snapshot| {
        events_tx
            .try_lock()
            .expect("fixture event sender available")
            .try_send(snapshot)
            .expect("bounded renderer event capacity");
    }));
    let (mut commands, commands_rx) = mpsc::channel(1);
    let renders = Rc::new(std::cell::Cell::new(0));
    let mut dom = VirtualDom::new_with_props(
        fixture,
        FixtureProps {
            controller: controller.clone(),
            commands: Commands(Rc::new(RefCell::new(Some(commands_rx)))),
            renders: renders.clone(),
        },
    );
    let document = web_sys::window()
        .expect("browser")
        .document()
        .expect("document");
    let root = document.create_element("div").expect("fixture root");
    document
        .body()
        .expect("body")
        .append_child(&root)
        .expect("attach actual renderer root");
    let observer = super::rendered_controls::BrowserRendererObserver::default();
    observer.install_context(&mut dom);
    let config = observer.config(dioxus::web::Config::new().rootnode(root.clone().into()));
    let owner = crate::task_owner::new_web_task_owner();
    owner.spawn_local_cancellable(async move { dioxus::web::run(dom, config).await });
    let visible = next_root(&mut events_rx, ControlLayoutState::Visible, 0).await;
    console_log!("initial visible publication received");
    let parent_renders = renders.get();
    let initial = visible.rendered_controls[0].capture_revision;
    let source = visible.rendered_controls[0].source.clone();
    commands.try_send(Mode::Hidden).expect("hidden render");
    let hidden = next_root(&mut events_rx, ControlLayoutState::Hidden, initial).await;
    assert_eq!(hidden.rendered_controls[0].source, source);
    assert!(hidden.rendered_controls[0].capture_revision > initial);
    commands.try_send(Mode::ZeroExtent).expect("zero render");
    let zero = next_root(
        &mut events_rx,
        ControlLayoutState::ZeroExtent,
        hidden.rendered_controls[0].capture_revision,
    )
    .await;
    commands.try_send(Mode::Unmounted).expect("unmount");
    console_log!("await actual unmount publication");
    let unmounted_capture = loop {
        let snapshot = events_rx.next().await.expect("unmount event");
        if let aura_app::ui::contract::RenderedControlFrame::Committed {
            capture_revision, ..
        } = snapshot.rendered_control_frame
        {
            if capture_revision > zero.rendered_controls[0].capture_revision
                && snapshot.rendered_controls.is_empty()
            {
                break capture_revision;
            }
        }
    };
    commands
        .try_send(Mode::Visible)
        .expect("same source remount");
    let remounted = next_root(
        &mut events_rx,
        ControlLayoutState::Visible,
        unmounted_capture,
    )
    .await;
    assert_eq!(remounted.rendered_controls[0].source, source);
    assert!(
        remounted.rendered_controls[0].capture_revision
            > hidden.rendered_controls[0].capture_revision
    );
    commands
        .try_send(Mode::Duplicate)
        .expect("duplicate typed roots");
    console_log!("await actual duplicate roots publication");
    let duplicate_roots = loop {
        let snapshot = events_rx.next().await.expect("duplicate event");
        if matches!(&snapshot.rendered_control_frame, aura_app::ui::contract::RenderedControlFrame::Committed { capture_revision, .. } if *capture_revision > remounted.rendered_controls[0].capture_revision)
            && snapshot.rendered_controls.len() == 2
            && snapshot
                .rendered_controls
                .iter()
                .all(|control| control.state == ControlLayoutState::Visible)
        {
            break snapshot;
        }
    };
    commands
        .try_send(Mode::Buttons)
        .expect("actual shared button producers");
    console_log!("await actual shared buttons publication");
    let duplicate = loop {
        let snapshot = events_rx
            .next()
            .await
            .expect("actual UiButton layout publication");
        if matches!(&snapshot.rendered_control_frame, aura_app::ui::contract::RenderedControlFrame::Committed { capture_revision, .. } if *capture_revision > duplicate_roots.rendered_controls[0].capture_revision)
            && snapshot.rendered_controls.len() == 2
            && snapshot.rendered_controls.iter().all(|control| {
                control.control == ControlId::ContactsCreateInvitationButton
                    && control.state == ControlLayoutState::Visible
            })
        {
            break snapshot;
        }
    };
    let aura_app::ui::contract::RenderedControlFrame::Committed {
        capture_revision,
        layout_revision,
        ..
    } = duplicate.rendered_control_frame
    else {
        panic!("committed duplicate frame");
    };
    root.set_attribute("style", "width:400px;height:0px;overflow:auto")
        .expect("external layout resize");
    console_log!("await actual resized layout publication");
    let resized_layout = loop {
        let snapshot = events_rx
            .next()
            .await
            .expect("actual ResizeObserver publication");
        if let aura_app::ui::contract::RenderedControlFrame::Committed {
            capture_revision: observed_capture,
            layout_revision: observed_layout,
            ..
        } = snapshot.rendered_control_frame
        {
            if observed_capture == capture_revision
                && observed_layout > layout_revision
                && snapshot.rendered_controls.len() == 2
                && snapshot
                    .rendered_controls
                    .iter()
                    .all(|control| control.state == ControlLayoutState::ZeroExtent)
            {
                break observed_layout;
            }
        }
    };
    root.set_attribute("style", "width:400px;height:40px;overflow:auto")
        .expect("scroll viewport");
    let spacer = document
        .create_element("div")
        .expect("owned scroll fixture");
    spacer
        .set_attribute("style", "height:1000px")
        .expect("scroll extent");
    root.append_child(&spacer).expect("owned fixture child");
    // Establish the current non-scrolled layout before arranging the actual
    // browser scroll. Only the canonical listener for this exact element can
    // acknowledge the next action; ResizeObserver/getter cannot satisfy it.
    assert!(super::rendered_controls::capture_current_layout());
    let scroll_observed = observer.observe_next_scroll(root.clone().into());
    root.set_scroll_top(1000);
    assert!(
        root.scroll_top() > 0,
        "fixture must perform actual scrolling"
    );
    assert!(scroll_observed
        .await
        .expect("actual owner scroll listener must run"));
    assert!(
        super::rendered_controls::capture_current_layout(),
        "fresh getter joins existing committed renderer frame"
    );
    loop {
        let snapshot = events_rx.next().await.expect("fresh scrolled layout");
        if let aura_app::ui::contract::RenderedControlFrame::Committed {
            capture_revision: observed_capture,
            layout_revision: observed_layout,
            ..
        } = snapshot.rendered_control_frame
        {
            if observed_capture == capture_revision
                && observed_layout > resized_layout
                && snapshot.rendered_controls.len() == 2
                && snapshot
                    .rendered_controls
                    .iter()
                    .all(|control| control.state == ControlLayoutState::ZeroExtent)
            {
                break;
            }
        }
    }
    assert_eq!(
        renders.get(),
        parent_renders,
        "all layout changes were child-owned; renderer parent remained memoized"
    );
    // Actual producer evidence above is authoritative. DOM references are used
    // to arrange layout changes and clean the fixture root; typed owner publications decide visibility.
    controller
        .clear_ui_snapshot_sink()
        .expect("detach fixture snapshot receiver before producer disposal");
    owner.shutdown();
    owner.wait_drained().await;
    root.remove();
}
