use tokio::sync::broadcast;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeDiagnosticSeverity {
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeDiagnosticKind {
    ReactiveFactPublishFailed,
    ReactiveShutdownSignalDropped,
    SupervisedTaskFailed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeDiagnostic {
    pub severity: RuntimeDiagnosticSeverity,
    pub kind: RuntimeDiagnosticKind,
    pub component: &'static str,
    pub message: String,
}

#[derive(Debug)]
pub struct RuntimeDiagnosticSink {
    tx: broadcast::Sender<RuntimeDiagnostic>,
}

impl RuntimeDiagnosticSink {
    const CHANNEL_CAPACITY: usize = 128;

    pub fn new() -> Self {
        let (tx, _rx) = broadcast::channel(Self::CHANNEL_CAPACITY);
        Self { tx }
    }

    pub fn emit(&self, diagnostic: RuntimeDiagnostic) {
        let _ = self.tx.send(diagnostic);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<RuntimeDiagnostic> {
        self.tx.subscribe()
    }
}

impl Default for RuntimeDiagnosticSink {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_diagnostic_sink_broadcasts_events() {
        let sink = RuntimeDiagnosticSink::new();
        let mut rx = sink.subscribe();

        let diagnostic = RuntimeDiagnostic {
            severity: RuntimeDiagnosticSeverity::Error,
            kind: RuntimeDiagnosticKind::ReactiveFactPublishFailed,
            component: "reactive_pipeline",
            message: "fact sink closed".to_string(),
        };
        sink.emit(diagnostic.clone());

        let received = rx.try_recv().expect("diagnostic received");
        assert_eq!(received, diagnostic);
    }
}
