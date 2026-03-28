use futures::channel::oneshot;
use futures::future::{select, Either};
use futures::pin_mut;
use js_sys::{Promise, Reflect};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

use crate::error::WebUiError;
use aura_ui::FrontendUiOperation as WebUiOperation;

async fn browser_timer_ms(
    ms: u64,
    operation: WebUiOperation,
    unavailable_code: &'static str,
    schedule_failed_code: &'static str,
    dropped_code: &'static str,
    unavailable_message: &'static str,
    schedule_action: &'static str,
) -> Result<(), WebUiError> {
    let window = web_sys::window().ok_or_else(|| {
        WebUiError::operation(operation, unavailable_code, unavailable_message.to_string())
    })?;
    let timeout_ms = ms.min(i32::MAX as u64) as i32;
    let (tx, rx) = oneshot::channel::<()>();
    let callback = Closure::once(move || {
        let _ = tx.send(());
    });
    window
        .set_timeout_with_callback_and_timeout_and_arguments_0(
            callback.as_ref().unchecked_ref(),
            timeout_ms,
        )
        .map_err(|error| {
            WebUiError::operation(
                operation,
                schedule_failed_code,
                format!("failed to schedule {schedule_action}: {error:?}"),
            )
        })?;
    callback.forget();
    rx.await.map_err(|_| {
        WebUiError::operation(
            operation,
            dropped_code,
            format!("{schedule_action} dropped before completion"),
        )
    })?;
    Ok(())
}

pub(crate) async fn browser_sleep_ms(
    ms: u64,
    operation: WebUiOperation,
    unavailable_code: &'static str,
    schedule_failed_code: &'static str,
    dropped_code: &'static str,
    unavailable_message: &'static str,
    schedule_action: &'static str,
) -> Result<(), WebUiError> {
    browser_timer_ms(
        ms,
        operation,
        unavailable_code,
        schedule_failed_code,
        dropped_code,
        unavailable_message,
        schedule_action,
    )
    .await
}

pub(crate) async fn await_browser_promise_with_timeout(
    promise: Promise,
    timeout_ms: u64,
    operation: WebUiOperation,
    rejected_code: &'static str,
    timeout_code: &'static str,
    timer_schedule_failed_code: &'static str,
    timer_dropped_code: &'static str,
    action: &'static str,
    abort_on_timeout: Option<&web_sys::AbortController>,
) -> Result<JsValue, WebUiError> {
    let promise_future = JsFuture::from(promise);
    let timeout_future = browser_timer_ms(
        timeout_ms,
        operation,
        "WEB_BROWSER_PROMISE_WINDOW_UNAVAILABLE",
        timer_schedule_failed_code,
        timer_dropped_code,
        "window unavailable for bounded browser promise wait",
        action,
    );
    pin_mut!(promise_future);
    pin_mut!(timeout_future);
    match select(promise_future, timeout_future).await {
        Either::Left((result, _)) => result.map_err(|error| {
            WebUiError::operation(
                operation,
                rejected_code,
                format!("{action} rejected before completion: {error:?}"),
            )
        }),
        Either::Right((result, _)) => {
            result?;
            if let Some(controller) = abort_on_timeout {
                controller.abort();
            }
            Err(WebUiError::operation(
                operation,
                timeout_code,
                format!("{action} timed out after {timeout_ms}ms"),
            ))
        }
    }
}

pub(crate) async fn fetch_text_with_timeout(
    url: &str,
    timeout_ms: u64,
    operation: WebUiOperation,
    fetch_rejected_code: &'static str,
    fetch_timeout_code: &'static str,
    text_rejected_code: &'static str,
    text_timeout_code: &'static str,
) -> Result<String, WebUiError> {
    let window = web_sys::window().ok_or_else(|| {
        WebUiError::operation(
            operation,
            "WEB_BROWSER_PROMISE_WINDOW_UNAVAILABLE",
            "window unavailable for bounded browser fetch".to_string(),
        )
    })?;
    let abort_controller = web_sys::AbortController::new().map_err(|error| {
        WebUiError::operation(
            operation,
            "WEB_BROWSER_FETCH_ABORT_CONTROLLER_FAILED",
            format!("failed to create AbortController for browser fetch: {error:?}"),
        )
    })?;
    let request_init = web_sys::RequestInit::new();
    Reflect::set(
        request_init.as_ref(),
        &JsValue::from_str("signal"),
        abort_controller.signal().as_ref(),
    )
    .map_err(|error| {
        WebUiError::operation(
            operation,
            "WEB_BROWSER_FETCH_ABORT_SIGNAL_BIND_FAILED",
            format!("failed to bind AbortSignal for browser fetch: {error:?}"),
        )
    })?;
    let fetch_promise = window.fetch_with_str_and_init(url, &request_init);
    let response_value = await_browser_promise_with_timeout(
        fetch_promise,
        timeout_ms,
        operation,
        fetch_rejected_code,
        fetch_timeout_code,
        "WEB_BROWSER_FETCH_TIMEOUT_SCHEDULE_FAILED",
        "WEB_BROWSER_FETCH_TIMEOUT_DROPPED",
        "browser fetch",
        Some(&abort_controller),
    )
    .await?;
    let response: web_sys::Response = response_value.dyn_into().map_err(|value: JsValue| {
        WebUiError::operation(
            operation,
            "WEB_BROWSER_FETCH_RESPONSE_CAST_FAILED",
            format!("failed to cast browser fetch response: {value:?}"),
        )
    })?;
    let text_promise = response.text().map_err(|error| {
        WebUiError::operation(
            operation,
            "WEB_BROWSER_FETCH_TEXT_PROMISE_FAILED",
            format!("failed to create browser response text promise: {error:?}"),
        )
    })?;
    await_browser_promise_with_timeout(
        text_promise,
        timeout_ms,
        operation,
        text_rejected_code,
        text_timeout_code,
        "WEB_BROWSER_RESPONSE_TEXT_TIMEOUT_SCHEDULE_FAILED",
        "WEB_BROWSER_RESPONSE_TEXT_TIMEOUT_DROPPED",
        "browser response text read",
        None,
    )
    .await?
    .as_string()
    .ok_or_else(|| {
        WebUiError::operation(
            operation,
            "WEB_BROWSER_FETCH_TEXT_INVALID",
            "browser response text promise did not resolve to a string".to_string(),
        )
    })
}
