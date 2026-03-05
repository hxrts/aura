use tokio::sync::RwLock;

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
        *self.text.blocking_write() = text.to_string();
    }

    fn read(&self) -> String {
        self.text.blocking_read().clone()
    }
}
