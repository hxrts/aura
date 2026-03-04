use std::sync::RwLock;

pub trait ClipboardPort: Send + Sync {
    fn write(&self, text: &str);
    fn read(&self) -> String;
}

#[derive(Default)]
pub struct MemoryClipboard {
    text: RwLock<String>,
}

impl ClipboardPort for MemoryClipboard {
    fn write(&self, text: &str) {
        if let Ok(mut guard) = self.text.write() {
            *guard = text.to_string();
        }
    }

    fn read(&self) -> String {
        if let Ok(guard) = self.text.read() {
            return guard.clone();
        }
        String::new()
    }
}
