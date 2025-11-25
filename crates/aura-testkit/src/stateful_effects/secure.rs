//! Mock secure storage effect handlers for testing

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Mock secure entry
#[derive(Debug, Clone)]
pub struct SecureEntry {
    pub data: Vec<u8>,
    pub expiry: Option<u64>,
}

/// Mock secure storage handler for testing
#[derive(Debug)]
pub struct MockSecureStorageHandler {
    #[allow(dead_code)]
    storage: Arc<Mutex<HashMap<String, SecureEntry>>>,
    #[allow(dead_code)]
    current_time: Arc<Mutex<u64>>,
}

impl Default for MockSecureStorageHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl MockSecureStorageHandler {
    pub fn new() -> Self {
        Self {
            storage: Arc::new(Mutex::new(HashMap::new())),
            current_time: Arc::new(Mutex::new(0)),
        }
    }
}
