use aura_app::errors::ErrorCategory;
use aura_app::frontend_primitives::FrontendUiOperation as WebUiOperation;

#[derive(Clone, Debug)]
pub(crate) struct WebUiError {
    operation: WebUiOperation,
    category: ErrorCategory,
    code: &'static str,
    message: String,
    source: Option<std::sync::Arc<dyn std::error::Error + Send + Sync>>,
}

impl WebUiError {
    pub(crate) fn new(
        operation: WebUiOperation,
        category: ErrorCategory,
        code: &'static str,
        message: impl Into<String>,
    ) -> Self {
        Self {
            operation,
            category,
            code,
            message: message.into(),
            source: None,
        }
    }

    pub(crate) fn config(
        operation: WebUiOperation,
        code: &'static str,
        message: impl Into<String>,
    ) -> Self {
        Self::new(operation, ErrorCategory::Config, code, message)
    }

    pub(crate) fn input(
        operation: WebUiOperation,
        code: &'static str,
        message: impl Into<String>,
    ) -> Self {
        Self::new(operation, ErrorCategory::Input, code, message)
    }

    pub(crate) fn operation(
        operation: WebUiOperation,
        code: &'static str,
        message: impl Into<String>,
    ) -> Self {
        Self::new(operation, ErrorCategory::Operation, code, message)
    }

    pub(crate) fn with_operation(&self, operation: WebUiOperation) -> Self {
        Self {
            operation,
            category: self.category,
            code: self.code,
            message: self.message.clone(),
            source: self.source.clone(),
        }
    }

    pub(crate) fn with_source(
        mut self,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        self.source = Some(std::sync::Arc::new(source));
        self
    }
    pub(crate) fn user_message(&self) -> String {
        format!(
            "[{}] {}: {}. Hint: {}",
            self.code,
            self.operation.label(),
            self.message,
            self.category.resolution_hint()
        )
    }
}

impl std::fmt::Display for WebUiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.user_message())
    }
}

impl PartialEq for WebUiError {
    fn eq(&self, other: &Self) -> bool {
        self.operation == other.operation
            && self.category == other.category
            && self.code == other.code
            && self.message == other.message
    }
}
impl Eq for WebUiError {}
impl std::error::Error for WebUiError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_deref()
            .map(|e| e as &(dyn std::error::Error + 'static))
    }
}

pub(crate) fn log_web_error(level: &str, error: &WebUiError) {
    let rendered = error.user_message();
    match level {
        "error" => web_sys::console::error_1(&rendered.into()),
        "warn" => web_sys::console::warn_1(&rendered.into()),
        _ => web_sys::console::log_1(&rendered.into()),
    }
}
