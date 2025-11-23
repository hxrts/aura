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
    storage: Arc<Mutex<HashMap<String, SecureEntry>>>,
    current_time: Arc<Mutex<u64>>,
}

impl MockSecureStorageHandler {
    pub fn new() -> Self {
        Self {
            storage: Arc::new(Mutex::new(HashMap::new())),
            current_time: Arc::new(Mutex::new(0)),
        }
    }
}