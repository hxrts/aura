use js_sys::{Array, Function, Object, Reflect};
use wasm_bindgen::{JsCast, JsValue};

#[derive(Clone)]
pub(crate) struct HarnessWindowContract {
    window: web_sys::Window,
}

impl HarnessWindowContract {
    pub(crate) fn new(window: web_sys::Window) -> Self {
        Self { window }
    }

    pub(crate) fn current() -> Option<Self> {
        web_sys::window().map(Self::new)
    }

    pub(crate) fn raw_window(&self) -> &web_sys::Window {
        &self.window
    }

    pub(crate) fn get(&self, key: &str) -> Result<JsValue, JsValue> {
        Reflect::get(self.window.as_ref(), &JsValue::from_str(key))
    }

    pub(crate) fn set(&self, key: &str, value: &JsValue) -> Result<bool, JsValue> {
        Reflect::set(self.window.as_ref(), &JsValue::from_str(key), value)
    }

    pub(crate) fn function(&self, key: &str) -> Option<Function> {
        self.get(key)
            .ok()
            .and_then(|value| value.dyn_into::<Function>().ok())
    }

    pub(crate) fn ensure_nullish(&self, key: &str, default: &JsValue) -> Result<(), JsValue> {
        let existing = self.get(key)?;
        if existing.is_null() || existing.is_undefined() {
            self.set(key, default)?;
        }
        Ok(())
    }

    pub(crate) fn ensure_bool(&self, key: &str, value: bool) -> Result<(), JsValue> {
        let existing = self.get(key)?;
        if existing.is_undefined() {
            self.set(key, &JsValue::from_bool(value))?;
        }
        Ok(())
    }

    pub(crate) fn ensure_array(&self, key: &str) -> Result<Array, JsValue> {
        let existing = self.get(key)?;
        if Array::is_array(&existing) {
            Ok(Array::from(&existing))
        } else {
            let array = Array::new();
            self.set(key, &array)?;
            Ok(array)
        }
    }

    pub(crate) fn ensure_object(&self, key: &str) -> Result<Object, JsValue> {
        let existing = self.get(key)?;
        if existing.is_object() && !existing.is_null() {
            existing
                .dyn_into::<Object>()
                .map_err(|_| JsValue::from_str(&format!("failed to access object window.{key}")))
        } else {
            let object = Object::new();
            self.set(key, &object)?;
            Ok(object)
        }
    }
}

pub(crate) fn object_get(object: &Object, key: &str) -> Result<JsValue, JsValue> {
    Reflect::get(object.as_ref(), &JsValue::from_str(key))
}

pub(crate) fn object_set(object: &Object, key: &str, value: &JsValue) -> Result<bool, JsValue> {
    Reflect::set(object.as_ref(), &JsValue::from_str(key), value)
}

pub(crate) fn ensure_object_field(
    object: &Object,
    key: &str,
    default: &JsValue,
) -> Result<(), JsValue> {
    let existing = object_get(object, key)?;
    if existing.is_undefined() {
        object_set(object, key, default)?;
    }
    Ok(())
}
