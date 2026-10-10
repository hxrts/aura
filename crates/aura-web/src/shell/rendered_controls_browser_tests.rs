//! Actual dioxus-web renderer fixture. These tests require a browser and cannot
//! be substituted by native VirtualDom mounted-event simulation.
use super::rendered_controls::{ObservedRendererFrame, RenderedOnboardingRoot};
use aura_app::ui::contract::{ControlId, ControlLayoutState, UiSnapshot};
use dioxus::prelude::*;
use futures::{channel::mpsc, StreamExt};
use std::{cell::RefCell, rc::Rc, sync::Arc};
use wasm_bindgen_test::*;
wasm_bindgen_test_configure!(run_in_browser);

fn fixture_controller() -> Arc<aura_ui::UiController> {
    Arc::new(aura_ui::UiController::new(
        Arc::new(async_lock::RwLock::new(
            aura_app::AppCore::new(aura_app::AppConfig::default()).expect("fixture app"),
        )),
        Arc::new(aura_ui::MemoryClipboard::default()),
    ))
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Visible,
    Hidden,
    ZeroExtent,
    Unmounted,
    Duplicate,
    Buttons,
    WakeOnly,
    Bounce,
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
                    if next == Mode::Bounce {
                        mode.set(Mode::Hidden);
                        // The effect runs after the first flush and schedules a
                        // second render before this renderer poll suspends.
                        dioxus::dioxus_core::queue_effect(move || mode.set(Mode::Visible));
                    } else if next != Mode::WakeOnly {
                        mode.set(next);
                    }
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
    let mut revoked = after_capture == 0;
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
        if matches!(
            snapshot.rendered_control_frame,
            aura_app::ui::contract::RenderedControlFrame::Unavailable
        ) {
            revoked = true;
        }
        if matches!(&snapshot.rendered_control_frame, aura_app::ui::contract::RenderedControlFrame::Committed { capture_revision, .. } if *capture_revision > after_capture)
            && snapshot.rendered_controls.iter().any(|observation| {
                observation.control == ControlId::OnboardingRoot && observation.state == state
            })
        {
            assert!(
                revoked,
                "old evidence must be revoked before acknowledging a renderer poll"
            );
            return snapshot;
        }
    }
}

#[wasm_bindgen_test]
async fn actual_renderer_visible_hidden_zero_unmount_and_same_source_remount() {
    let controller = fixture_controller();
    let (events_tx, mut events_rx) = mpsc::channel(32);
    let events_tx = async_lock::Mutex::new(events_tx);
    let latest = Arc::new(std::sync::Mutex::new(None));
    let delivered = latest.clone();
    controller.set_ui_snapshot_sink(Arc::new(move |snapshot| {
        *delivered.lock().expect("latest snapshot") = Some(snapshot.clone());
        if matches!(
            snapshot.rendered_control_frame,
            aura_app::ui::contract::RenderedControlFrame::Unavailable
        ) {
            assert!(
                !super::rendered_controls::capture_current_layout(),
                "reentrant layout reads cannot acknowledge an in-progress renderer poll"
            );
        }
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
    let config = dioxus_web::Config::new().rootnode(root.clone().into());
    let owner = crate::task_owner::new_web_task_owner();
    owner.spawn_local_cancellable(observer.run(dom, config));
    let visible = next_root(&mut events_rx, ControlLayoutState::Visible, 0).await;
    console_log!("initial visible publication received");
    let parent_renders = renders.get();
    // Wake the renderer without changing any signal or producing DOM edits.
    // A yielded no-op poll must preserve the actual final node state.
    commands
        .try_send(Mode::WakeOnly)
        .expect("wake without edits");
    let woke = next_root(
        &mut events_rx,
        ControlLayoutState::Visible,
        visible.rendered_controls[0].capture_revision,
    )
    .await;
    assert_eq!(
        woke.rendered_controls[0].source,
        visible.rendered_controls[0].source
    );
    commands.try_send(Mode::Bounce).expect("two render batches");
    let bounced = loop {
        let snapshot = events_rx.next().await.expect("batched render publication");
        if let aura_app::ui::contract::RenderedControlFrame::Committed {
            capture_revision, ..
        } = snapshot.rendered_control_frame
        {
            if capture_revision > woke.rendered_controls[0].capture_revision {
                assert!(
                    snapshot
                        .rendered_controls
                        .iter()
                        .all(|control| control.state == ControlLayoutState::Visible),
                    "only the final flushed state of the poll may be acknowledged"
                );
                break snapshot;
            }
        }
    };
    let initial = bounced.rendered_controls[0].capture_revision;
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
    let resize_observed = observer.observe_next_resize(root.clone());
    root.set_attribute("style", "width:400px;height:0px;overflow:auto")
        .expect("external layout resize");
    assert!(resize_observed
        .await
        .expect("actual root ResizeObserver must run"));
    let resized = latest
        .lock()
        .expect("resize delivery")
        .clone()
        .expect("resized frame");
    let aura_app::ui::contract::RenderedControlFrame::Committed {
        capture_revision: resized_capture,
        layout_revision: resized_layout,
        ..
    } = resized.rendered_control_frame
    else {
        panic!("resize must publish committed layout")
    };
    // An independently queued renderer poll may supersede the earlier receipt.
    // The exact-target callback, rather than receipt equality, witnesses resize.
    assert!(resized_capture >= capture_revision);
    assert!(resized_layout > layout_revision);
    assert_eq!(resized.rendered_controls.len(), 2);
    assert!(resized
        .rendered_controls
        .iter()
        .all(|control| control.state == ControlLayoutState::ZeroExtent));
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
    let before_fresh = latest
        .lock()
        .expect("scroll delivery")
        .clone()
        .expect("scroll frame");
    let aura_app::ui::contract::RenderedControlFrame::Committed {
        capture_revision: before_capture,
        layout_revision: before_layout,
        ..
    } = before_fresh.rendered_control_frame
    else {
        panic!("scroll must publish committed layout")
    };
    assert!(
        super::rendered_controls::capture_current_layout(),
        "fresh getter joins existing committed renderer frame"
    );
    let fresh = latest
        .lock()
        .expect("fresh delivery")
        .clone()
        .expect("fresh frame");
    let aura_app::ui::contract::RenderedControlFrame::Committed {
        capture_revision: fresh_capture,
        layout_revision: fresh_layout,
        ..
    } = fresh.rendered_control_frame
    else {
        panic!("fresh measurement must remain committed")
    };
    assert_eq!(
        fresh_capture, before_capture,
        "a fresh measurement cannot issue a renderer receipt"
    );
    assert!(fresh_layout > before_layout && fresh_layout > resized_layout);
    assert_eq!(fresh.rendered_controls.len(), 2);
    assert!(fresh
        .rendered_controls
        .iter()
        .all(|control| control.state == ControlLayoutState::ZeroExtent));
    assert_eq!(
        renders.get(),
        parent_renders,
        "all layout changes were child-owned; renderer parent remained memoized"
    );
    // Actual producer evidence above is authoritative. DOM references are used
    // to arrange layout changes and clean the fixture root; typed owner publications decide visibility.
    owner.shutdown();
    owner.wait_drained().await;
    assert!(
        !super::rendered_controls::capture_current_layout(),
        "cancelled renderer cannot retain committed evidence"
    );
    assert!(matches!(
        latest
            .lock()
            .expect("last delivery")
            .as_ref()
            .expect("disposal publishes revocation")
            .rendered_control_frame,
        aura_app::ui::contract::RenderedControlFrame::Unavailable
    ));
    controller
        .clear_ui_snapshot_sink()
        .expect("detach fixture receiver");
    root.remove();
}

/// Drive the real renderer directly so asset invalidation is checked in the
/// same JavaScript turn, before MutationObserver callbacks can be delivered.
#[wasm_bindgen_test]
fn actual_renderer_stylesheet_reload_withholds_commit_before_callback() {
    use std::{
        future::Future,
        task::{Context, Poll},
    };
    let controller = fixture_controller();
    let latest = Arc::new(std::sync::Mutex::new(None));
    let delivered = latest.clone();
    controller.set_ui_snapshot_sink(Arc::new(move |snapshot| {
        *delivered.lock().expect("latest snapshot") = Some(snapshot);
    }));
    let document = web_sys::window()
        .expect("browser")
        .document()
        .expect("document");
    let root = document.create_element("div").expect("renderer root");
    let link = document.create_element("link").expect("stylesheet");
    link.set_attribute("rel", "stylesheet")
        .expect("stylesheet relation");
    link.set_attribute("href", "data:text/css,")
        .expect("initial stylesheet");
    document
        .body()
        .expect("body")
        .append_child(&root)
        .expect("attach renderer");
    document
        .body()
        .expect("body")
        .append_child(&link)
        .expect("attach stylesheet");
    let (_commands, receiver) = mpsc::channel(1);
    let mut dom = VirtualDom::new_with_props(
        fixture,
        FixtureProps {
            controller: controller.clone(),
            commands: Commands(Rc::new(RefCell::new(Some(receiver)))),
            renders: Rc::new(std::cell::Cell::new(0)),
        },
    );
    let observer = super::rendered_controls::BrowserRendererObserver::default();
    observer.install_context(&mut dom);
    let mut renderer =
        Box::pin(observer.run(dom, dioxus_web::Config::new().rootnode(root.clone().into())));
    let waker = futures::task::noop_waker();
    let mut cx = Context::from_waker(&waker);
    assert!(matches!(renderer.as_mut().poll(&mut cx), Poll::Pending));
    assert!(matches!(
        latest
            .lock()
            .expect("initial delivery")
            .as_ref()
            .expect("committed frame")
            .rendered_control_frame,
        aura_app::ui::contract::RenderedControlFrame::Committed { .. }
    ));
    link.set_attribute("href", "data:text/css,/*reload*/")
        .expect("replace stylesheet");
    assert!(
        !super::rendered_controls::capture_current_layout(),
        "pending asset records must revoke evidence before asynchronous delivery"
    );
    assert!(matches!(
        latest
            .lock()
            .expect("asset delivery")
            .as_ref()
            .expect("revoked frame")
            .rendered_control_frame,
        aura_app::ui::contract::RenderedControlFrame::Unavailable
    ));
    assert!(renderer.as_mut().poll(&mut cx).is_pending());
    assert!(
        !super::rendered_controls::capture_current_layout(),
        "subsequent polls cannot recertify an unsupported asset reload"
    );
    drop(renderer);
    controller.clear_ui_snapshot_sink().expect("detach sink");
    root.remove();
    link.remove();
}
