//! Actual renderer-backed onboarding observations. No DOM lookup or timer is
//! permitted here: the renderer supplies the node and post-flush commit event.
use std::{cell::RefCell, rc::Rc};

use aura_app::ui::contract::{
    ControlId, ControlRenderCapture, ControlRenderSource, RenderedControlLayout,
    RenderedControlObservation, UiSnapshot,
};
use aura_ui::dom_ids::RequiredDomId;
use dioxus::prelude::*;

/// Read the actual registered node, intersecting its layout with the viewport
/// and clipping ancestors. Hidden/disconnected nodes never become visible.
fn committed_node_layout(element: &web_sys::Element) -> Option<RenderedControlLayout> {
    let window = web_sys::window()?;
    let mut hidden = false;
    let rect = element.get_bounding_client_rect();
    let mut left = rect.left().max(0.0);
    let mut top = rect.top().max(0.0);
    let mut right = rect.right().min(window.inner_width().ok()?.as_f64()?);
    let mut bottom = rect.bottom().min(window.inner_height().ok()?.as_f64()?);
    let mut ancestor = Some(element.clone());
    while let Some(node) = ancestor {
        let style = window.get_computed_style(&node).ok()??;
        let property = |name| style.get_property_value(name).ok();
        hidden |= node.has_attribute("hidden")
            || node.get_attribute("aria-hidden").as_deref() == Some("true")
            || property("display")? == "none"
            || matches!(property("visibility")?.as_str(), "hidden" | "collapse")
            || property("content-visibility")? == "hidden"
            || property("opacity")?.parse::<f64>().ok()? <= 0.0;
        // Overflow applies to descendants, not the node's own border box.
        if node != *element {
            let bounds = node.get_bounding_client_rect();
            if matches!(
                property("overflow-x")?.as_str(),
                "hidden" | "clip" | "scroll" | "auto"
            ) {
                left = left.max(bounds.left());
                right = right.min(bounds.right());
            }
            if matches!(
                property("overflow-y")?.as_str(),
                "hidden" | "clip" | "scroll" | "auto"
            ) {
                top = top.max(bounds.top());
                bottom = bottom.min(bounds.bottom());
            }
        }
        ancestor = node.parent_element();
    }
    Some(RenderedControlLayout {
        connected: element.is_connected(),
        hidden,
        width: (right - left).max(0.0),
        height: (bottom - top).max(0.0),
    })
}

struct RegisteredNode {
    control: Option<ControlId>,
    node: Option<web_sys::Element>,
    disposed: bool,
}
#[derive(Default)]
struct RendererRegistry {
    observer: std::rc::Weak<RefCell<ObserverState>>,
    controller: Option<std::sync::Arc<aura_ui::UiController>>,
    snapshot: Option<UiSnapshot>,
    nodes: Vec<std::rc::Weak<RefCell<RegisteredNode>>>,
    current: std::rc::Weak<RendererFrame>,
}
struct RendererFrame {
    controller: std::sync::Arc<aura_ui::UiController>,
    receipt: Option<aura_ui::RendererCaptureReceipt>,
    snapshot: UiSnapshot,
    registry: Rc<RefCell<RendererRegistry>>,
    committed: std::cell::Cell<bool>,
    disposed: std::cell::Cell<bool>,
}
impl RendererFrame {
    fn nodes(&self) -> Vec<Rc<RefCell<RegisteredNode>>> {
        let mut registry = self.registry.borrow_mut();
        registry.nodes.retain(|node| node.strong_count() > 0);
        registry
            .nodes
            .iter()
            .filter_map(|node| node.upgrade())
            .filter(|node| node.borrow().control.is_some())
            .collect()
    }
    fn observation(
        &self,
        control: ControlId,
        layout: RenderedControlLayout,
    ) -> RenderedControlObservation {
        let source = ControlRenderSource::from_snapshot(&self.snapshot);
        let mut capture = ControlRenderCapture::new(
            control,
            source.clone(),
            self.receipt
                .as_ref()
                .map_or(0, |receipt| receipt.revision()),
        );
        capture.acknowledge_renderer_commit(&source);
        capture.observe(&self.snapshot, layout)
    }
    fn dispose(&self) {
        self.disposed.set(true);
        if let Some(receipt) = self.receipt.as_ref() {
            self.controller.invalidate_rendered_control_layout(receipt);
        }
    }
    fn publish_if_complete(&self) -> bool {
        if self.disposed.get() || !self.committed.get() {
            return false;
        }
        let Some(receipt) = self.receipt.as_ref() else {
            return false;
        };
        let mut observations = Vec::new();
        for node in self.nodes() {
            let registered = node.borrow();
            if registered.disposed || registered.control.is_none() {
                continue;
            }
            // Registration belongs to an actual renderer component instance.
            // Withhold the entire frame until all initial node callbacks join
            // the complete edit-batch ACK; never publish a partial root count.
            let Some(element) = registered.node.as_ref() else {
                return false;
            };
            let Some(layout) = committed_node_layout(element) else {
                return false;
            };
            observations.push(self.observation(
                registered.control.expect("registered typed control"),
                layout,
            ));
        }
        self.controller.publish_rendered_control_snapshot(
            receipt,
            self.snapshot.clone(),
            observations,
        );
        true
    }
}
#[derive(Default)]
struct ObserverState {
    registry: std::rc::Weak<RefCell<RendererRegistry>>,
    pending: Option<aura_ui::RendererCaptureReceipt>,
    active: Option<Rc<RendererFrame>>,
    unsupported: bool,
    measuring: bool,
    layout_listeners: Vec<BrowserLayoutListener>,
    resize_observer: Option<BrowserResizeObserver>,
    #[cfg(test)]
    next_scroll: Option<(
        web_sys::EventTarget,
        futures::channel::oneshot::Sender<bool>,
    )>,
}
struct BrowserResizeObserver {
    observer: web_sys::ResizeObserver,
    _callback: wasm_bindgen::closure::Closure<dyn FnMut(js_sys::Array, web_sys::ResizeObserver)>,
}
impl Drop for BrowserResizeObserver {
    fn drop(&mut self) {
        self.observer.disconnect();
    }
}
struct BrowserLayoutListener {
    target: web_sys::EventTarget,
    event: &'static str,
    callback: wasm_bindgen::closure::Closure<dyn FnMut(web_sys::Event)>,
}
impl Drop for BrowserLayoutListener {
    fn drop(&mut self) {
        use wasm_bindgen::JsCast;
        let _ = self.target.remove_event_listener_with_callback_and_bool(
            self.event,
            self.callback.as_ref().unchecked_ref(),
            true,
        );
    }
}
fn invalidate_current_layout(owner: &Rc<RefCell<ObserverState>>) {
    capture_owner_layout(owner.clone());
}
thread_local! {
    // Observed lookup only. The actual root/config retains the sole owner.
    static ACTIVE_RENDERER_OBSERVATION: RefCell<std::rc::Weak<RefCell<ObserverState>>> = RefCell::new(std::rc::Weak::new());
}
struct LayoutMeasurementGuard(Rc<RefCell<ObserverState>>);
impl Drop for LayoutMeasurementGuard {
    fn drop(&mut self) {
        self.0.borrow_mut().measuring = false;
    }
}
/// Fresh measurement of registered nodes under their existing committed batch.
/// It cannot turn pending edits into renderer commit acknowledgment.
pub(crate) fn capture_current_layout() -> bool {
    let Some(owner) = ACTIVE_RENDERER_OBSERVATION.with(|slot| slot.borrow().upgrade()) else {
        return false;
    };
    capture_owner_layout(owner)
}
fn capture_owner_layout(owner: Rc<RefCell<ObserverState>>) -> bool {
    let active = {
        let mut state = owner.borrow_mut();
        if state.unsupported || state.measuring || state.pending.is_some() {
            return false;
        }
        let Some(active) = state.active.clone() else {
            return false;
        };
        if active.disposed.get() || !active.committed.get() {
            return false;
        }
        if active.controller.is_delivering_ui_snapshot() {
            return false;
        }
        state.measuring = true;
        active
    };
    let _guard = LayoutMeasurementGuard(owner);
    if let Some(receipt) = active.receipt.as_ref() {
        active
            .controller
            .invalidate_rendered_control_layout(receipt);
    }
    active.publish_if_complete()
}
/// Root renderer configuration, shared with its actual registered-node context.
/// No task, selector, timer, or independent semantic lifecycle is created.
#[derive(Clone, Default)]
pub(crate) struct BrowserRendererObserver(Rc<RefCell<ObserverState>>);
impl BrowserRendererObserver {
    #[cfg(test)]
    pub(super) fn observe_next_scroll(
        &self,
        target: web_sys::EventTarget,
    ) -> futures::channel::oneshot::Receiver<bool> {
        let (sender, receiver) = futures::channel::oneshot::channel();
        assert!(self
            .0
            .borrow_mut()
            .next_scroll
            .replace((target, sender))
            .is_none());
        receiver
    }

    pub(crate) fn install_context(&self, dom: &mut VirtualDom) {
        ACTIVE_RENDERER_OBSERVATION.with(|slot| *slot.borrow_mut() = Rc::downgrade(&self.0));
        dom.provide_root_context(self.clone());
        {
            use wasm_bindgen::JsCast;
            let weak = Rc::downgrade(&self.0);
            let callback = wasm_bindgen::closure::Closure::wrap(Box::new(
                move |_: js_sys::Array, _: web_sys::ResizeObserver| {
                    if let Some(owner) = weak.upgrade() {
                        invalidate_current_layout(&owner);
                    }
                },
            )
                as Box<dyn FnMut(js_sys::Array, web_sys::ResizeObserver)>);
            match web_sys::ResizeObserver::new(callback.as_ref().unchecked_ref()) {
                Ok(observer) => {
                    self.0.borrow_mut().resize_observer = Some(BrowserResizeObserver {
                        observer,
                        _callback: callback,
                    })
                }
                Err(_) => self.0.borrow_mut().unsupported = true,
            }
        }
        if let Some(window) = web_sys::window() {
            use wasm_bindgen::JsCast;
            let target: web_sys::EventTarget = window.into();
            for event in ["resize", "scroll"] {
                let weak = Rc::downgrade(&self.0);
                let callback = wasm_bindgen::closure::Closure::wrap(Box::new(
                    move |observed_event: web_sys::Event| {
                        if let Some(owner) = weak.upgrade() {
                            let captured = capture_owner_layout(owner.clone());
                            #[cfg(test)]
                            if event == "scroll" {
                                let notification = {
                                    let mut state = owner.borrow_mut();
                                    if state.next_scroll.as_ref().is_some_and(|(expected, _)| {
                                        observed_event.target().as_ref() == Some(expected)
                                    }) {
                                        state.next_scroll.take()
                                    } else {
                                        None
                                    }
                                };
                                if let Some((_, sender)) = notification {
                                    let _ = sender.send(captured);
                                }
                            }
                            #[cfg(not(test))]
                            let _ = (captured, observed_event);
                        }
                    },
                )
                    as Box<dyn FnMut(web_sys::Event)>);
                if target
                    .add_event_listener_with_callback_and_bool(
                        event,
                        callback.as_ref().unchecked_ref(),
                        true,
                    )
                    .is_ok()
                {
                    self.0
                        .borrow_mut()
                        .layout_listeners
                        .push(BrowserLayoutListener {
                            target: target.clone(),
                            event,
                            callback,
                        });
                } else {
                    self.0.borrow_mut().unsupported = true;
                }
            }
        }
    }
    pub(crate) fn config(&self, config: dioxus::web::Config) -> dioxus::web::Config {
        let observer = self.clone();
        config.render_batch_observer(Rc::new(move |phase| observer.observe(phase)))
    }
    fn observe(&self, phase: dioxus::web::RenderBatchPhase) {
        use dioxus::web::RenderBatchPhase;
        if matches!(
            phase,
            RenderBatchPhase::HydrationUnavailable | RenderBatchPhase::ExternalMutationUnavailable
        ) {
            let active = {
                let mut state = self.0.borrow_mut();
                state.unsupported = true;
                state.pending = None;
                state.active.take()
            };
            if let Some(active) = active {
                active.dispose();
            }
            return;
        }
        if self.0.borrow().unsupported {
            return;
        }
        let Some(registry) = self.0.borrow().registry.upgrade() else {
            return;
        };
        let Some(controller) = registry.borrow().controller.clone() else {
            return;
        };
        match phase {
            RenderBatchPhase::BeforeRender => {
                let pending = controller.begin_renderer_capture();
                let active = {
                    let mut state = self.0.borrow_mut();
                    state.pending = pending;
                    state.active.take()
                };
                if let Some(active) = active {
                    active.dispose();
                }
            }
            RenderBatchPhase::BeforeCommit => {
                let Some(snapshot) = registry.borrow().snapshot.clone() else {
                    return;
                };
                let pending = self.0.borrow_mut().pending.take();
                let receipt = pending.or_else(|| controller.begin_renderer_capture());
                let frame = Rc::new(RendererFrame {
                    controller,
                    receipt,
                    snapshot,
                    registry: registry.clone(),
                    committed: std::cell::Cell::new(false),
                    disposed: std::cell::Cell::new(false),
                });
                registry.borrow_mut().current = Rc::downgrade(&frame);
                self.0.borrow_mut().active = Some(frame);
            }
            RenderBatchPhase::Committed => {
                let active = self.0.borrow().active.clone();
                if let Some(active) = active {
                    active.committed.set(true);
                    active.publish_if_complete();
                }
            }
            RenderBatchPhase::HydrationUnavailable
            | RenderBatchPhase::ExternalMutationUnavailable => unreachable!(),
        }
    }
}

pub(crate) fn launch_observed(root: fn() -> Element) {
    let observer = BrowserRendererObserver::default();
    let mut dom = VirtualDom::new(root);
    observer.install_context(&mut dom);
    dioxus::web::launch::launch_virtual_dom(dom, observer.config(dioxus::web::Config::new()));
}

/// Bind the semantic source actually rendered by this owner. Renderer callbacks
/// cover every batch, including child-only local updates with this owner memoized.
#[component]
pub(super) fn ObservedRendererFrame(
    snapshot: UiSnapshot,
    controller: std::sync::Arc<aura_ui::UiController>,
    children: Element,
) -> Element {
    let observer = use_context::<BrowserRendererObserver>();
    let registry = use_hook(|| Rc::new(RefCell::new(RendererRegistry::default())));
    use_context_provider({
        let registry = registry.clone();
        move || {
            aura_ui::rendered_control::RenderedControlRegistrar(Rc::new(move |control| {
                register_control(registry.clone(), control)
            }))
        }
    });
    {
        let mut registry = registry.borrow_mut();
        registry.controller = Some(controller);
        registry.snapshot = Some(snapshot);
        registry.observer = Rc::downgrade(&observer.0);
    }
    observer.0.borrow_mut().registry = Rc::downgrade(&registry);
    use_drop({
        let registry = registry.clone();
        move || {
            let mut state = observer.0.borrow_mut();
            if state
                .registry
                .upgrade()
                .is_some_and(|current| Rc::ptr_eq(&current, &registry))
            {
                state.registry = std::rc::Weak::new();
                state.pending = None;
                let active = state.active.take();
                drop(state);
                if let Some(active) = active {
                    active.dispose();
                }
            }
        }
    });
    rsx! { {children} }
}
fn register_control(
    registry: Rc<RefCell<RendererRegistry>>,
    control: Option<ControlId>,
) -> aura_ui::rendered_control::RenderedControlRegistration {
    let registered = Rc::new(RefCell::new(RegisteredNode {
        control,
        node: None,
        disposed: false,
    }));
    registry.borrow_mut().nodes.push(Rc::downgrade(&registered));
    let update_node = registered.clone();
    let update_registry = registry.clone();
    let dispose_node = registered.clone();
    let dispose_registry = registry.clone();
    aura_ui::rendered_control::RenderedControlRegistration {
        update_control: Rc::new(move |control| {
            let changed = {
                let mut node = update_node.borrow_mut();
                let changed = node.control != control;
                node.control = control;
                changed
            };
            if changed {
                refresh_resize_targets(&update_registry);
            }
        }),
        dispose: Rc::new(move || {
            let mut node = dispose_node.borrow_mut();
            node.disposed = true;
            node.node = None;
            drop(node);
            refresh_resize_targets(&dispose_registry);
        }),
        mounted: Rc::new(move |event| {
            if let Some(node) = event.data().downcast::<web_sys::Element>() {
                let mut registration = registered.borrow_mut();
                if registration.disposed {
                    return;
                }
                registration.node = Some(node.clone());
                drop(registration);
                refresh_resize_targets(&registry);
                let current = registry.borrow().current.upgrade();
                if let Some(frame) = current {
                    frame.publish_if_complete();
                }
            }
        }),
    }
}

fn refresh_resize_targets(registry: &Rc<RefCell<RendererRegistry>>) {
    let (owner, nodes) = {
        let registry = registry.borrow();
        let nodes = registry
            .nodes
            .iter()
            .filter_map(|node| node.upgrade())
            .filter_map(|node| {
                let node = node.borrow();
                if node.disposed || node.control.is_none() {
                    None
                } else {
                    node.node.clone()
                }
            })
            .collect::<Vec<_>>();
        (registry.observer.upgrade(), nodes)
    };
    if let Some(owner) = owner {
        if let Some(observer) = owner.borrow().resize_observer.as_ref() {
            observer.observer.disconnect();
            for node in nodes {
                let mut ancestor = Some(node);
                while let Some(element) = ancestor {
                    observer.observer.observe(&element);
                    ancestor = element.parent_element();
                }
            }
        }
    }
}

/// Onboarding delegates actual node registration to the same adapter used by
/// shared Ready controls. The web shell alone retains the Element reference.
#[component]
pub(super) fn RenderedOnboardingRoot(children: Element) -> Element {
    let mounted = aura_ui::rendered_control::use_rendered_control(ControlId::OnboardingRoot);
    rsx! { div {
        id: ControlId::OnboardingRoot.required_dom_id("ControlId::OnboardingRoot"),
        class: "w-full max-w-xl",
        onmounted: move |event| mounted.call(event),
        {children}
    }}
}
