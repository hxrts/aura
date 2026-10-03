//! Actual independent browsing contexts and broker-confirmed owner death.
#![cfg(target_arch = "wasm32")]
use aura_core::effects::profile_storage::ProfileStorageError;
use aura_effects::profile_storage::FilesystemProfileStorageHandler;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_test::*;
wasm_bindgen_test_configure!(run_in_browser);

async fn bounded_browser_promise(promise: js_sys::Promise) -> JsValue {
    use futures::future::{select, Either};
    let response = Box::pin(wasm_bindgen_futures::JsFuture::from(promise));
    let timeout = Box::pin(gloo_timers::future::TimeoutFuture::new(10_000));
    match select(response, timeout).await {
        Either::Left((response, _)) => response.expect("actual browser resource promise succeeds"),
        Either::Right(_) => panic!("actual browser resource acknowledgement timed out"),
    }
}

#[wasm_bindgen_test]
async fn actual_other_frame_owns_profile_until_context_destruction() {
    // The other same-origin frame directly owns the actual browser broker lock.
    // It cannot invoke or fabricate the Rust concrete lease constructor.
    let profile = format!("test/other-context-{}", js_sys::Math::random());
    let lock = format!("aura-profile-owner:{profile}");
    let lock_literal = serde_json::to_string(&lock).unwrap();
    let source = format!(
        r#"
        return new Promise((resolve, reject) => {{
            const frame = document.createElement('iframe');
            const marker = {lock_literal};
            function receive(event) {{
                if (event.source !== frame.contentWindow || !event.data || event.data.marker !== marker) return;
                window.removeEventListener('message', receive);
                if (event.data.error) {{ frame.remove(); reject(new Error(event.data.error)); }}
                else resolve(frame);
            }}
            window.addEventListener('message', receive);
            frame.srcdoc = '<script>navigator.locks.request(' + JSON.stringify(marker) +
                ', {{mode:"exclusive"}}, () => {{ parent.postMessage({{marker:' + JSON.stringify(marker) +
                '}}, "*"); return new Promise(() => {{}}); }}).catch(error => parent.postMessage({{marker:' +
                JSON.stringify(marker) + ',error:String(error)}}, "*"));<\/script>';
            document.body.appendChild(frame);
        }});
    "#
    );
    let frame = bounded_browser_promise(
        js_sys::Function::new_no_args(&source)
            .call0(&JsValue::UNDEFINED)
            .unwrap()
            .dyn_into::<js_sys::Promise>()
            .unwrap(),
    )
    .await;
    let adapter = FilesystemProfileStorageHandler::new(profile.into());
    assert!(matches!(
        adapter.acquire_owned_browser().await,
        Err(ProfileStorageError::Busy)
    ));
    // Destroying the actual owner realm is the browser process/page cancellation
    // case. The queued broker request acknowledges release; no sleeps/polling.
    let remove = js_sys::Reflect::get(&frame, &JsValue::from_str("remove"))
        .unwrap()
        .dyn_into::<js_sys::Function>()
        .unwrap();
    remove.call0(&frame).unwrap();
    let acknowledge = js_sys::Function::new_with_args(
        "name",
        "return navigator.locks.request(name, {mode:'exclusive'}, () => true);",
    );
    bounded_browser_promise(
        acknowledge
            .call1(&JsValue::UNDEFINED, &JsValue::from_str(&lock))
            .unwrap()
            .dyn_into::<js_sys::Promise>()
            .unwrap(),
    )
    .await;
    let owner = adapter
        .acquire_owned_browser()
        .await
        .expect("destroyed other realm releases exact profile");
    owner.release().await.unwrap();
}
