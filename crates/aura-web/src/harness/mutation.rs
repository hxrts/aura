use aura_ui::UiController;
use futures::channel::oneshot;
use std::cell::RefCell as StdRefCell;
use std::rc::Rc;
use std::sync::Arc;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};

pub(crate) fn schedule_browser_task_next_tick(
    action: impl FnOnce() + 'static,
) -> Result<(), JsValue> {
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("window unavailable"))?;
    let action = Rc::new(StdRefCell::new(Some(Box::new(action) as Box<dyn FnOnce()>)));
    let callback_action = action.clone();
    let callback = Closure::once(move || {
        if let Some(action) = callback_action.borrow_mut().take() {
            action();
        }
    });
    window
        .set_timeout_with_callback_and_timeout_and_arguments_0(callback.as_ref().unchecked_ref(), 0)
        .map_err(|error| {
            JsValue::from_str(&format!("failed to schedule browser task: {error:?}"))
        })?;
    callback.forget();
    Ok(())
}

pub(crate) async fn schedule_browser_ui_mutation(
    controller: Arc<UiController>,
    action: impl FnOnce(&UiController) + 'static,
) -> Result<(), JsValue> {
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("window unavailable"))?;
    let (tx, rx) = oneshot::channel::<()>();
    let action = Rc::new(StdRefCell::new(Some(Box::new(move || {
        let snapshot = controller.semantic_model_snapshot();
        web_sys::console::log_1(
            &format!(
                "[web-ui-mutation] pre screen={:?};readiness={:?};focused={:?}",
                snapshot.screen, snapshot.readiness, snapshot.focused_control
            )
            .into(),
        );
        action(controller.as_ref());
        let final_snapshot =
            crate::harness_bridge::publish_semantic_controller_snapshot(controller.clone());
        web_sys::console::log_1(
            &format!(
                "[web-ui-mutation] post screen={:?};readiness={:?};focused={:?}",
                final_snapshot.screen, final_snapshot.readiness, final_snapshot.focused_control
            )
            .into(),
        );
    }) as Box<dyn FnOnce()>)));
    let callback_action = action.clone();
    let callback = Closure::once(move || {
        if let Some(action) = callback_action.borrow_mut().take() {
            action();
        }
        let _ = tx.send(());
    });
    window
        .set_timeout_with_callback_and_timeout_and_arguments_0(callback.as_ref().unchecked_ref(), 0)
        .map_err(|error| {
            JsValue::from_str(&format!("failed to schedule UI mutation: {error:?}"))
        })?;
    callback.forget();
    rx.await
        .map_err(|_| JsValue::from_str("scheduled UI mutation dropped before execution"))?;
    Ok(())
}

pub(crate) fn apply_browser_ui_mutation(
    controller: Arc<UiController>,
    action: impl FnOnce(&UiController),
) {
    let snapshot = controller.semantic_model_snapshot();
    web_sys::console::log_1(
        &format!(
            "[web-ui-mutation] pre screen={:?};readiness={:?};focused={:?}",
            snapshot.screen, snapshot.readiness, snapshot.focused_control
        )
        .into(),
    );
    action(controller.as_ref());
    let final_snapshot = crate::harness_bridge::publish_semantic_controller_snapshot(controller);
    web_sys::console::log_1(
        &format!(
            "[web-ui-mutation] post screen={:?};readiness={:?};focused={:?}",
            final_snapshot.screen, final_snapshot.readiness, final_snapshot.focused_control
        )
        .into(),
    );
}
