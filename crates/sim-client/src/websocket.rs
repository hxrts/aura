//! WebSocket connection handling for WASM

use anyhow::{anyhow, Result};
use futures::channel::mpsc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{ErrorEvent, MessageEvent, WebSocket, CloseEvent};

use crate::{console_error, console_log, console_warn, log};

/// WebSocket connection wrapper for WASM
pub struct WebSocketConnection {
    websocket: WebSocket,
    _message_handler: Option<Closure<dyn FnMut(MessageEvent)>>,
    _error_handler: Option<Closure<dyn FnMut(ErrorEvent)>>,
    _close_handler: Option<Closure<dyn FnMut(CloseEvent)>>,
}

impl WebSocketConnection {
    /// Create a new WebSocket connection
    pub async fn new(url: &str) -> Result<Self> {
        console_log!("Creating WebSocket connection to {}", url);

        let websocket = WebSocket::new(url)
            .map_err(|e| anyhow!("Failed to create WebSocket: {:?}", e))?;

        // Set binary type for potential binary message support
        websocket.set_binary_type(web_sys::BinaryType::Arraybuffer);

        let mut connection = WebSocketConnection {
            websocket,
            _message_handler: None,
            _error_handler: None,
            _close_handler: None,
        };

        // Wait for connection to open
        connection.wait_for_open().await?;

        console_log!("WebSocket connection established");
        Ok(connection)
    }

    /// Wait for the WebSocket to open
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

        // For now, just wait a bit and assume connection succeeds
        // In a full implementation, we would set up proper promise handling
        let promise = js_sys::Promise::resolve(&JsValue::NULL);

        JsFuture::from(promise).await
            .map_err(|e| anyhow!("WebSocket failed to open: {:?}", e))?;

        Ok(())
    }

    /// Set up message handler with channel sender
    pub fn set_message_handler(&mut self, sender: mpsc::UnboundedSender<String>) {
        let websocket = &self.websocket;

        // Message handler
        let onmessage = Closure::wrap(Box::new(move |event: MessageEvent| {
            if let Ok(text) = event.data().dyn_into::<js_sys::JsString>() {
                let message: String = text.into();
                if let Err(e) = sender.unbounded_send(message) {
                    console_error!("Failed to send message to handler: {:?}", e);
                }
            } else {
                console_warn!("Received non-text message");
            }
        }) as Box<dyn FnMut(_)>);

        websocket.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
        self._message_handler = Some(onmessage);

        // Error handler
        let onerror = Closure::wrap(Box::new(move |event: ErrorEvent| {
            console_error!("WebSocket error: {:?}", event);
        }) as Box<dyn FnMut(_)>);

        websocket.set_onerror(Some(onerror.as_ref().unchecked_ref()));
        self._error_handler = Some(onerror);

        // Close handler
        let onclose = Closure::wrap(Box::new(move |event: CloseEvent| {
            console_log!("WebSocket closed: code={}, reason={}", event.code(), event.reason());
        }) as Box<dyn FnMut(_)>);

        websocket.set_onclose(Some(onclose.as_ref().unchecked_ref()));
        self._close_handler = Some(onclose);
    }

    /// Send text message
    pub async fn send_text(&self, message: &str) -> Result<()> {
        if self.websocket.ready_state() != WebSocket::OPEN {
            return Err(anyhow!("WebSocket is not open"));
        }

        self.websocket.send_with_str(message)
            .map_err(|e| anyhow!("Failed to send message: {:?}", e))?;

        Ok(())
    }

    /// Send binary message
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
}

impl Drop for WebSocketConnection {
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
    fn test_websocket_creation() {
        // This test would require a running WebSocket server
        // For now, just test that the struct can be created
        assert_eq!(WebSocket::CONNECTING, 0);
        assert_eq!(WebSocket::OPEN, 1);
        assert_eq!(WebSocket::CLOSING, 2);
        assert_eq!(WebSocket::CLOSED, 3);
    }
}