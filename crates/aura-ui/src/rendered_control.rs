//! Platform-neutral registration of an actual rendered control instance.
//! The shell adapter owns node references and renderer commit acknowledgment.
use aura_app::ui::contract::ControlId;
use dioxus::prelude::*;
use std::rc::Rc;

/// Observer callbacks for one actual component lifetime. These callbacks do not
/// authorize semantic work or infer visibility from control eligibility.
pub struct RenderedControlRegistration {
    pub update_control: Rc<dyn Fn(Option<ControlId>)>,
    pub mounted: Rc<dyn Fn(MountedEvent)>,
    pub dispose: Rc<dyn Fn()>,
}

/// Optional renderer-provided registration adapter. Native component-only
/// fixtures may render without an adapter; that produces no visibility proof.
#[derive(Clone)]
pub struct RenderedControlRegistrar(
    pub Rc<dyn Fn(Option<ControlId>) -> RenderedControlRegistration>,
);

/// Register an actual element and return its mounted callback. The component
/// owns disposal; the adapter owns the platform-specific node reference.
pub fn use_rendered_control(control: ControlId) -> EventHandler<MountedEvent> {
    use_optional_rendered_control(Some(control))
}

/// Register an optional typed identity without conditional hook allocation.
pub fn use_optional_rendered_control(control: Option<ControlId>) -> EventHandler<MountedEvent> {
    let registrar = try_use_context::<RenderedControlRegistrar>();
    let registration = use_hook(move || registrar.map(|registrar| Rc::new((registrar.0)(control))));
    if let Some(registration) = registration.as_ref() {
        (registration.update_control)(control);
    }
    use_drop({
        let registration = registration.clone();
        move || {
            if let Some(registration) = registration {
                (registration.dispose)();
            }
        }
    });
    EventHandler::new(move |event| {
        if let Some(registration) = registration.as_ref() {
            (registration.mounted)(event);
        }
    })
}

#[derive(Props, Clone, PartialEq)]
pub struct RenderedInputProps {
    #[props(default)]
    pub autofocus: bool,
    #[props(default)]
    pub onfocus: EventHandler<FocusEvent>,
    #[props(default)]
    pub oninput: EventHandler<FormEvent>,
    #[props(default)]
    pub onkeydown: EventHandler<KeyboardEvent>,

    pub control_id: ControlId,
    #[props(extends = GlobalAttributes)]
    #[props(extends = input)]
    pub attributes: Vec<Attribute>,
}
/// Preserve the actual input element while owning its typed registration.
#[component]
pub fn RenderedInput(props: RenderedInputProps) -> Element {
    let mounted = use_rendered_control(props.control_id);
    rsx! { input {
    autofocus: props.autofocus,
    onfocus: move |event| props.onfocus.call(event),
    oninput: move |event| props.oninput.call(event),
    onkeydown: move |event| props.onkeydown.call(event),

        id: props.control_id.web_dom_id(),
        onmounted: move |event| mounted.call(event),
        ..props.attributes,
    }}
}

#[derive(Props, Clone, PartialEq)]
pub(crate) struct RenderedRegionProps {
    pub control_id: ControlId,
    pub children: Element,
    #[props(extends = GlobalAttributes)]
    #[props(extends = div)]
    pub attributes: Vec<Attribute>,
}
/// Preserve an actual region div and register its rendered node.
#[component]
pub(crate) fn RenderedRegion(props: RenderedRegionProps) -> Element {
    let mounted = use_rendered_control(props.control_id);
    rsx! { div {
        id: props.control_id.web_dom_id(),
        onmounted: move |event| mounted.call(event),
        ..props.attributes,
        {props.children}
    }}
}

#[derive(Props, Clone, PartialEq)]
pub(crate) struct RenderedScrollViewportProps {
    #[props(default)]
    pub class: Option<String>,
    pub list_id: aura_app::ui::contract::ListId,
    pub children: Element,
    #[props(extends = GlobalAttributes)]
    #[props(extends = div)]
    pub attributes: Vec<Attribute>,
}
/// Register the actual scroll viewport node. The pinned shadcn viewport
/// (ee0ac75, blocks/src/components/scroll_area.rs) is a pure div with these
/// classes, data-slot and tab index; it exposes no attributes or event props.
/// Keeping that complete element here permits typed node registration without
/// adding another layout element. The upstream viewport has no hooks/context
/// reads, scrolling handlers or lifecycle behavior to forward.
#[component]
pub(crate) fn RenderedScrollViewport(props: RenderedScrollViewportProps) -> Element {
    let mounted = use_rendered_control(ControlId::List(props.list_id));
    let class = format!(
        "size-full rounded-[inherit] overflow-auto transition-[color,box-shadow] outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50 focus-visible:outline-1 [&::-webkit-scrollbar]:hidden [-ms-overflow-style:none] [scrollbar-width:none] {}",
        props.class.as_deref().unwrap_or("")
    );
    rsx! { div {
        id: props.list_id.web_dom_id(),
        class,
        "data-slot": "scroll-area-viewport",
        tabindex: "0",
        onmounted: move |event| mounted.call(event),
        ..props.attributes,
        {props.children}
    }}
}

#[derive(Props, Clone, PartialEq)]
pub struct RenderedTextareaProps {
    #[props(default)]
    pub onfocus: EventHandler<FocusEvent>,
    #[props(default)]
    pub oninput: EventHandler<FormEvent>,
    #[props(default)]
    pub onkeydown: EventHandler<KeyboardEvent>,

    pub control_id: ControlId,
    #[props(extends = GlobalAttributes)]
    #[props(extends = textarea)]
    pub attributes: Vec<Attribute>,
}
/// Register a real textarea while preserving its actual attributes/events.
#[component]
pub fn RenderedTextarea(props: RenderedTextareaProps) -> Element {
    let mounted = use_rendered_control(props.control_id);
    rsx! { textarea {
    onfocus: move |event| props.onfocus.call(event),
    oninput: move |event| props.oninput.call(event),
    onkeydown: move |event| props.onkeydown.call(event),

        id: props.control_id.web_dom_id(),
        onmounted: move |event| mounted.call(event),
        ..props.attributes,
    }}
}

#[derive(Props, Clone, PartialEq)]
pub struct RenderedButtonProps {
    #[props(default)]
    pub onclick: EventHandler<MouseEvent>,

    pub control_id: ControlId,
    pub children: Element,
    #[props(extends = GlobalAttributes)]
    #[props(extends = button)]
    pub attributes: Vec<Attribute>,
}
/// Register a real button without introducing a layout wrapper or changing style.
#[component]
pub fn RenderedButton(props: RenderedButtonProps) -> Element {
    let mounted = use_rendered_control(props.control_id);
    rsx! { button {
    onclick: move |event| props.onclick.call(event),

        id: props.control_id.web_dom_id(),
        onmounted: move |event| mounted.call(event),
        ..props.attributes,
        {props.children}
    }}
}
