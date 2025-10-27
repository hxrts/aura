//! WebSocket connection handling for live network instrumentation API

use anyhow::{anyhow, Result};
use futures::channel::mpsc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{ErrorEvent, MessageEvent, WebSocket, CloseEvent};

use crate::{console_error, console_log, console_warn, log};

/// WebSocket connection for live network instrumentation API
pub struct LiveWebSocketConnection {
    websocket: WebSocket,
    _message_handler: Option<Closure<dyn FnMut(MessageEvent)>>,
    _error_handler: Option<Closure<dyn FnMut(ErrorEvent)>>,
    _close_handler: Option<Closure<dyn FnMut(CloseEvent)>>,
    _open_handler: Option<Closure<dyn FnMut()>>,
}

impl LiveWebSocketConnection {
    /// Create a new WebSocket connection to instrumentation API
    pub async fn new(url: &str) -> Result<Self> {
        console_log!("Creating live WebSocket connection to {}", url);

        let websocket = WebSocket::new(url)
            .map_err(|e| anyhow!("Failed to create WebSocket: {:?}", e))?;

        // Set binary type for potential binary message support
        websocket.set_binary_type(web_sys::BinaryType::Arraybuffer);

        let mut connection = LiveWebSocketConnection {
            websocket,
            _message_handler: None,
            _error_handler: None,
            _close_handler: None,
            _open_handler: None,
        };

        // Wait for connection to open
        connection.wait_for_open().await?;

        console_log!("Live WebSocket connection established");
        Ok(connection)
    }

    /// Wait for the WebSocket to open with better promise handling
    async fn wait_for_open(&mut self) -> Result<()> {
        use wasm_bindgen_futures::JsFuture;
        use js_sys::Promise;

        let websocket = &self.websocket;
        let ready_state = websocket.ready_state();

        if ready_state == WebSocket::OPEN {
            return Ok(());
        }

        if ready_state == WebSocket::CLOSED || ready_state == WebSocket::CLOSING {
            return Err(anyhow!("WebSocket is closed or closing"));
        }

        // Create a promise that resolves when the socket opens
        let promise = Promise::new(&mut |resolve, reject| {
            let websocket_clone = websocket.clone();
            
            let onopen = Closure::once(Box::new(move || {
                resolve.call0(&JsValue::NULL).unwrap();
            }));
            
            let onerror = Closure::once(Box::new(move |_: ErrorEvent| {
                reject.call1(&JsValue::NULL, &JsValue::from_str("WebSocket failed to open")).unwrap();
            }));

            websocket_clone.set_onopen(Some(onopen.as_ref().unchecked_ref()));
            websocket_clone.set_onerror(Some(onerror.as_ref().unchecked_ref()));

            onopen.forget();
            onerror.forget();
        });

        JsFuture::from(promise).await
            .map_err(|e| anyhow!("WebSocket failed to open: {:?}", e))?;

        Ok(())
    }

    /// Set up message handler with channel sender
    pub fn set_message_handler(&mut self, sender: mpsc::UnboundedSender<String>) {
        let websocket = &self.websocket;

        // Message handler for live events
        let onmessage = Closure::wrap(Box::new(move |event: MessageEvent| {
            if let Ok(text) = event.data().dyn_into::<js_sys::JsString>() {
                let message: String = text.into();
                console_log!("Received live event: {}", &message[..message.len().min(100)]);
                if let Err(e) = sender.unbounded_send(message) {
                    console_error!("Failed to send live event to handler: {:?}", e);
                }
            } else {
                console_warn!("Received non-text message from live node");
            }
        }) as Box<dyn FnMut(_)>);

        websocket.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
        self._message_handler = Some(onmessage);

        // Error handler
        let onerror = Closure::wrap(Box::new(move |event: ErrorEvent| {
            console_error!("Live WebSocket error: {:?}", event);
        }) as Box<dyn FnMut(_)>);

        websocket.set_onerror(Some(onerror.as_ref().unchecked_ref()));
        self._error_handler = Some(onerror);

        // Close handler
        let onclose = Closure::wrap(Box::new(move |event: CloseEvent| {
            console_log!("Live WebSocket closed: code={}, reason={}", event.code(), event.reason());
        }) as Box<dyn FnMut(_)>);

        websocket.set_onclose(Some(onclose.as_ref().unchecked_ref()));
        self._close_handler = Some(onclose);

        // Open handler
        let onopen = Closure::wrap(Box::new(move || {
            console_log!("Live WebSocket connection opened");
        }) as Box<dyn FnMut()>);

        websocket.set_onopen(Some(onopen.as_ref().unchecked_ref()));
        self._open_handler = Some(onopen);
    }

    /// Send command to live node
    pub async fn send_command(&self, command: &str) -> Result<()> {
        if self.websocket.ready_state() != WebSocket::OPEN {
            return Err(anyhow!("WebSocket is not open"));
        }

        console_log!("Sending command to live node: {}", &command[..command.len().min(100)]);

        self.websocket.send_with_str(command)
            .map_err(|e| anyhow!("Failed to send command: {:?}", e))?;

        Ok(())
    }

    /// Send authentication token
    pub async fn authenticate(&self, token: &str) -> Result<()> {
        let auth_message = format!(r#"{{"type":"auth","token":"{}"}}"#, token);
        self.send_command(&auth_message).await
    }

    /// Subscribe to specific event types
    pub async fn subscribe(&self, event_types: &[String]) -> Result<()> {
        let subscribe_message = serde_json::json!({
            "type": "subscribe",
            "event_types": event_types
        });
        self.send_command(&subscribe_message.to_string()).await
    }

    /// Send binary message (for future protocol support)
    pub async fn send_binary(&self, data: &[u8]) -> Result<()> {
        if self.websocket.ready_state() != WebSocket::OPEN {
            return Err(anyhow!("WebSocket is not open"));
        }

        let array = js_sys::Uint8Array::new_with_length(data.len() as u32);
        array.copy_from(data);

        self.websocket.send_with_u8_array(data)
            .map_err(|e| anyhow!("Failed to send binary data: {:?}", e))?;

        Ok(())
    }

    /// Check if connection is open
    pub fn is_connected(&self) -> bool {
        self.websocket.ready_state() == WebSocket::OPEN
    }

    /// Close the connection
    pub fn close(&self) {
        if self.is_connected() {
            let _ = self.websocket.close();
        }
    }

    /// Get ready state
    pub fn ready_state(&self) -> u16 {
        self.websocket.ready_state()
    }

    /// Get connection URL
    pub fn url(&self) -> String {
        self.websocket.url()
    }
}

impl Drop for LiveWebSocketConnection {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_bindgen_test::*;

    wasm_bindgen_test_configure!(run_in_browser);

    #[wasm_bindgen_test]
    fn test_websocket_ready_states() {
        // Test WebSocket constants
        assert_eq!(WebSocket::CONNECTING, 0);
        assert_eq!(WebSocket::OPEN, 1);
        assert_eq!(WebSocket::CLOSING, 2);
        assert_eq!(WebSocket::CLOSED, 3);
    }
}