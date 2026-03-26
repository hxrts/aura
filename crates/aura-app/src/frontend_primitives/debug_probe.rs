use std::cell::RefCell;
use std::sync::Arc;

thread_local! {
    static FRONTEND_DEBUG_PROBE: RefCell<Option<Arc<dyn Fn(String)>>> = const { RefCell::new(None) };
}

pub fn set_frontend_debug_probe(probe: Option<Arc<dyn Fn(String)>>) {
    FRONTEND_DEBUG_PROBE.with(|slot| {
        *slot.borrow_mut() = probe;
    });
}

pub fn emit_frontend_debug_probe(message: impl Into<String>) {
    let message = message.into();
    FRONTEND_DEBUG_PROBE.with(|slot| {
        if let Some(probe) = slot.borrow().as_ref() {
            probe(message);
        }
    });
}
