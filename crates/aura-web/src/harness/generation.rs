use aura_ui::UiController;
use futures::channel::oneshot;
use js_sys::Reflect;
use std::cell::{Cell, RefCell};
use std::sync::Arc;
use wasm_bindgen::JsValue;

pub(crate) use crate::harness::browser_contract::{
    UI_ACTIVE_GENERATION_KEY, UI_GENERATION_PHASE_KEY, UI_READY_GENERATION_KEY,
};

thread_local! {
    static CONTROLLER: RefCell<Option<Arc<UiController>>> = const { RefCell::new(None) };
    static LAST_PUBLISHED_UI_STATE_JSON: RefCell<Option<String>> = const { RefCell::new(None) };
    static RENDER_SEQ: RefCell<u64> = const { RefCell::new(0) };
    static ACTIVE_GENERATION: Cell<u64> = const { Cell::new(0) };
    static READY_GENERATION: Cell<u64> = const { Cell::new(0) };
    static BOOTSTRAP_TRANSITION_DETAIL: RefCell<Option<String>> = const { RefCell::new(None) };
    static BROWSER_SHELL_PHASE: Cell<BrowserShellPhase> =
        const { Cell::new(BrowserShellPhase::Bootstrapping) };
    static GENERATION_READY_WAITERS: RefCell<Vec<(u64, oneshot::Sender<()>)>> =
        const { RefCell::new(Vec::new()) };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrowserShellPhase {
    Bootstrapping,
    HandoffCommitted,
    Rebinding,
    Ready,
}

pub(crate) fn browser_shell_phase_label(phase: BrowserShellPhase) -> &'static str {
    match phase {
        BrowserShellPhase::Bootstrapping => "bootstrapping",
        BrowserShellPhase::HandoffCommitted => "handoff_committed",
        BrowserShellPhase::Rebinding => "rebinding",
        BrowserShellPhase::Ready => "ready",
    }
}

pub(crate) fn current_browser_shell_phase() -> BrowserShellPhase {
    BROWSER_SHELL_PHASE.with(|slot| slot.get())
}

pub(crate) fn set_browser_shell_phase_local(phase: BrowserShellPhase) {
    BROWSER_SHELL_PHASE.with(|slot| {
        slot.set(phase);
    });
}

pub(crate) fn generation_js_value(generation_id: u64) -> JsValue {
    if generation_id == 0 {
        JsValue::NULL
    } else {
        JsValue::from_f64(generation_id as f64)
    }
}

pub(crate) fn active_generation() -> u64 {
    ACTIVE_GENERATION.with(|slot| slot.get())
}

pub(crate) fn ready_generation() -> u64 {
    READY_GENERATION.with(|slot| slot.get())
}

pub(crate) fn current_bootstrap_transition_detail() -> Option<String> {
    BOOTSTRAP_TRANSITION_DETAIL.with(|slot| slot.borrow().clone())
}

pub(crate) fn set_bootstrap_transition_detail_local(detail: Option<String>) {
    BOOTSTRAP_TRANSITION_DETAIL.with(|slot| {
        *slot.borrow_mut() = detail;
    });
}

pub(crate) fn sync_generation_globals(window: &web_sys::Window) {
    let active_generation = active_generation();
    let ready_generation = ready_generation();
    let _ = Reflect::set(
        window.as_ref(),
        &JsValue::from_str(UI_ACTIVE_GENERATION_KEY),
        &generation_js_value(active_generation),
    );
    let _ = Reflect::set(
        window.as_ref(),
        &JsValue::from_str(UI_READY_GENERATION_KEY),
        &generation_js_value(ready_generation),
    );
}

pub(crate) fn set_active_generation_local(generation_id: u64) {
    ACTIVE_GENERATION.with(|slot| {
        slot.set(generation_id);
    });
}

pub(crate) fn mark_generation_ready(generation_id: u64) {
    if generation_id == 0 {
        return;
    }
    READY_GENERATION.with(|slot| {
        if slot.get() < generation_id {
            slot.set(generation_id);
        }
    });
    GENERATION_READY_WAITERS.with(|slot| {
        let mut waiters = slot.borrow_mut();
        let mut pending = Vec::with_capacity(waiters.len());
        for (required_generation, sender) in waiters.drain(..) {
            if generation_id >= required_generation {
                let _ = sender.send(());
            } else {
                pending.push((required_generation, sender));
            }
        }
        *waiters = pending;
    });
    if let Some(window) = web_sys::window() {
        sync_generation_globals(&window);
    }
}

pub(crate) async fn wait_for_generation_ready(generation_id: u64) -> Result<(), JsValue> {
    if generation_id == 0 || ready_generation() >= generation_id {
        return Ok(());
    }
    let (tx, rx) = oneshot::channel();
    GENERATION_READY_WAITERS.with(|slot| {
        slot.borrow_mut().push((generation_id, tx));
    });
    rx.await
        .map_err(|_| JsValue::from_str(&format!("generation_ready_wait_dropped:{generation_id}")))
}

pub(crate) fn set_controller(controller: Arc<UiController>) -> bool {
    CONTROLLER.with(|slot| {
        let mut slot = slot.borrow_mut();
        let changed = slot
            .as_ref()
            .map(|current| !Arc::ptr_eq(current, &controller))
            .unwrap_or(true);
        *slot = Some(controller);
        changed
    })
}

pub(crate) fn current_controller() -> Option<Arc<UiController>> {
    CONTROLLER.with(|slot| slot.borrow().clone())
}

pub(crate) fn clear_controller_state() {
    CONTROLLER.with(|slot| {
        *slot.borrow_mut() = None;
    });
    LAST_PUBLISHED_UI_STATE_JSON.with(|slot| {
        *slot.borrow_mut() = None;
    });
    RENDER_SEQ.with(|slot| {
        *slot.borrow_mut() = 0;
    });
    ACTIVE_GENERATION.with(|slot| {
        slot.set(0);
    });
}

pub(crate) fn note_published_ui_snapshot_json(json: &str) -> bool {
    LAST_PUBLISHED_UI_STATE_JSON.with(|slot| {
        let mut last = slot.borrow_mut();
        if last.as_deref() == Some(json) {
            false
        } else {
            *last = Some(json.to_string());
            true
        }
    })
}

pub(crate) fn reset_published_ui_snapshot_dedup() {
    LAST_PUBLISHED_UI_STATE_JSON.with(|slot| {
        *slot.borrow_mut() = None;
    });
}

pub(crate) fn next_render_seq() -> u64 {
    RENDER_SEQ.with(|slot| {
        let mut seq = slot.borrow_mut();
        *seq = seq.saturating_add(1);
        *seq
    })
}
