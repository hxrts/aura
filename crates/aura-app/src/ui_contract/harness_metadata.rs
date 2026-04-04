use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FrontendId {
    Web,
    Tui,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserCacheBoundary {
    SessionStart,
    AuthoritySwitch,
    DeviceImport,
    StorageReset,
    NavigationRecovery,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserCacheBoundaryMetadata {
    pub boundary: BrowserCacheBoundary,
    pub reason_code: &'static str,
}

pub const BROWSER_CACHE_BOUNDARIES: &[BrowserCacheBoundaryMetadata] = &[
    BrowserCacheBoundaryMetadata {
        boundary: BrowserCacheBoundary::SessionStart,
        reason_code: "session_start",
    },
    BrowserCacheBoundaryMetadata {
        boundary: BrowserCacheBoundary::AuthoritySwitch,
        reason_code: "authority_switch",
    },
    BrowserCacheBoundaryMetadata {
        boundary: BrowserCacheBoundary::DeviceImport,
        reason_code: "device_import",
    },
    BrowserCacheBoundaryMetadata {
        boundary: BrowserCacheBoundary::StorageReset,
        reason_code: "storage_reset",
    },
    BrowserCacheBoundaryMetadata {
        boundary: BrowserCacheBoundary::NavigationRecovery,
        reason_code: "navigation_recovery",
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserHarnessBridgeMethodKind {
    Action,
    ReadState,
    Diagnostic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserHarnessBridgeMethod {
    pub name: &'static str,
    pub kind: BrowserHarnessBridgeMethodKind,
    pub deterministic: bool,
    pub returns_semantic_state: bool,
    pub returns_render_signal: bool,
}

pub const BROWSER_HARNESS_BRIDGE_API_VERSION: u32 = 3;

pub const BROWSER_HARNESS_BRIDGE_METHODS: &[BrowserHarnessBridgeMethod] = &[
    BrowserHarnessBridgeMethod {
        name: "send_keys",
        kind: BrowserHarnessBridgeMethodKind::Action,
        deterministic: false,
        returns_semantic_state: false,
        returns_render_signal: false,
    },
    BrowserHarnessBridgeMethod {
        name: "send_key",
        kind: BrowserHarnessBridgeMethodKind::Action,
        deterministic: false,
        returns_semantic_state: false,
        returns_render_signal: false,
    },
    BrowserHarnessBridgeMethod {
        name: "navigate_screen",
        kind: BrowserHarnessBridgeMethodKind::Action,
        deterministic: false,
        returns_semantic_state: false,
        returns_render_signal: false,
    },
    BrowserHarnessBridgeMethod {
        name: "open_settings_section",
        kind: BrowserHarnessBridgeMethodKind::Action,
        deterministic: false,
        returns_semantic_state: false,
        returns_render_signal: false,
    },
    BrowserHarnessBridgeMethod {
        name: "snapshot",
        kind: BrowserHarnessBridgeMethodKind::ReadState,
        deterministic: true,
        returns_semantic_state: false,
        returns_render_signal: true,
    },
    BrowserHarnessBridgeMethod {
        name: "ui_state",
        kind: BrowserHarnessBridgeMethodKind::ReadState,
        deterministic: true,
        returns_semantic_state: true,
        returns_render_signal: false,
    },
    BrowserHarnessBridgeMethod {
        name: "read_clipboard",
        kind: BrowserHarnessBridgeMethodKind::ReadState,
        deterministic: true,
        returns_semantic_state: false,
        returns_render_signal: false,
    },
    BrowserHarnessBridgeMethod {
        name: "submit_semantic_command",
        kind: BrowserHarnessBridgeMethodKind::Action,
        deterministic: false,
        returns_semantic_state: false,
        returns_render_signal: false,
    },
    BrowserHarnessBridgeMethod {
        name: "get_authority_id",
        kind: BrowserHarnessBridgeMethodKind::ReadState,
        deterministic: true,
        returns_semantic_state: false,
        returns_render_signal: false,
    },
    BrowserHarnessBridgeMethod {
        name: "tail_log",
        kind: BrowserHarnessBridgeMethodKind::Diagnostic,
        deterministic: true,
        returns_semantic_state: false,
        returns_render_signal: false,
    },
    BrowserHarnessBridgeMethod {
        name: "root_structure",
        kind: BrowserHarnessBridgeMethodKind::Diagnostic,
        deterministic: true,
        returns_semantic_state: false,
        returns_render_signal: true,
    },
    BrowserHarnessBridgeMethod {
        name: "inject_message",
        kind: BrowserHarnessBridgeMethodKind::Diagnostic,
        deterministic: false,
        returns_semantic_state: false,
        returns_render_signal: false,
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessObservationSurface {
    Browser,
    Tui,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationMethodKind {
    SemanticState,
    RenderSignal,
    Clipboard,
    Diagnostic,
    Identity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservationSurfaceMethod {
    pub name: &'static str,
    pub kind: ObservationMethodKind,
    pub deterministic: bool,
    pub returns_semantic_state: bool,
    pub returns_render_signal: bool,
}

pub const BROWSER_OBSERVATION_SURFACE_GLOBAL: &str = "__AURA_HARNESS_OBSERVE__";
pub const BROWSER_OBSERVATION_SURFACE_API_VERSION: u32 = 1;

pub const BROWSER_OBSERVATION_SURFACE_METHODS: &[ObservationSurfaceMethod] = &[
    ObservationSurfaceMethod {
        name: "snapshot",
        kind: ObservationMethodKind::RenderSignal,
        deterministic: true,
        returns_semantic_state: false,
        returns_render_signal: true,
    },
    ObservationSurfaceMethod {
        name: "ui_state",
        kind: ObservationMethodKind::SemanticState,
        deterministic: true,
        returns_semantic_state: true,
        returns_render_signal: false,
    },
    ObservationSurfaceMethod {
        name: "render_heartbeat",
        kind: ObservationMethodKind::RenderSignal,
        deterministic: true,
        returns_semantic_state: false,
        returns_render_signal: true,
    },
    ObservationSurfaceMethod {
        name: "read_clipboard",
        kind: ObservationMethodKind::Clipboard,
        deterministic: true,
        returns_semantic_state: false,
        returns_render_signal: false,
    },
    ObservationSurfaceMethod {
        name: "get_authority_id",
        kind: ObservationMethodKind::Identity,
        deterministic: true,
        returns_semantic_state: false,
        returns_render_signal: false,
    },
    ObservationSurfaceMethod {
        name: "tail_log",
        kind: ObservationMethodKind::Diagnostic,
        deterministic: true,
        returns_semantic_state: false,
        returns_render_signal: false,
    },
    ObservationSurfaceMethod {
        name: "root_structure",
        kind: ObservationMethodKind::Diagnostic,
        deterministic: true,
        returns_semantic_state: false,
        returns_render_signal: true,
    },
];

pub const TUI_OBSERVATION_SURFACE_API_VERSION: u32 = 1;

pub const TUI_OBSERVATION_SURFACE_METHODS: &[ObservationSurfaceMethod] = &[
    ObservationSurfaceMethod {
        name: "snapshot",
        kind: ObservationMethodKind::RenderSignal,
        deterministic: true,
        returns_semantic_state: false,
        returns_render_signal: true,
    },
    ObservationSurfaceMethod {
        name: "snapshot_dom",
        kind: ObservationMethodKind::RenderSignal,
        deterministic: true,
        returns_semantic_state: false,
        returns_render_signal: true,
    },
    ObservationSurfaceMethod {
        name: "ui_snapshot",
        kind: ObservationMethodKind::SemanticState,
        deterministic: true,
        returns_semantic_state: true,
        returns_render_signal: false,
    },
    ObservationSurfaceMethod {
        name: "wait_for_ui_snapshot_event",
        kind: ObservationMethodKind::SemanticState,
        deterministic: true,
        returns_semantic_state: true,
        returns_render_signal: false,
    },
    ObservationSurfaceMethod {
        name: "wait_for_dom_patterns",
        kind: ObservationMethodKind::Diagnostic,
        deterministic: true,
        returns_semantic_state: false,
        returns_render_signal: true,
    },
    ObservationSurfaceMethod {
        name: "wait_for_target",
        kind: ObservationMethodKind::Diagnostic,
        deterministic: true,
        returns_semantic_state: false,
        returns_render_signal: true,
    },
    ObservationSurfaceMethod {
        name: "tail_log",
        kind: ObservationMethodKind::Diagnostic,
        deterministic: true,
        returns_semantic_state: false,
        returns_render_signal: false,
    },
    ObservationSurfaceMethod {
        name: "read_clipboard",
        kind: ObservationMethodKind::Clipboard,
        deterministic: true,
        returns_semantic_state: false,
        returns_render_signal: false,
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HarnessModeChangeKind {
    Observation,
    TimingDiscipline,
    RenderingStability,
    Instrumentation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessModeAllowance {
    pub path: &'static str,
    pub kind: HarnessModeChangeKind,
    pub owner: &'static str,
    pub design_ref: &'static str,
}

pub const HARNESS_MODE_ALLOWLIST: &[HarnessModeAllowance] = &[
    HarnessModeAllowance {
        path: "crates/aura-app/src/workflows/runtime.rs",
        kind: HarnessModeChangeKind::TimingDiscipline,
        owner: "aura-app-runtime",
        design_ref: "docs/804_testing_guide.md",
    },
    HarnessModeAllowance {
        path: "crates/aura-app/src/workflows/invitation.rs",
        kind: HarnessModeChangeKind::Instrumentation,
        owner: "aura-app-invitation",
        design_ref: "docs/804_testing_guide.md",
    },
    HarnessModeAllowance {
        path: "crates/aura-agent/src/handlers/invitation.rs",
        kind: HarnessModeChangeKind::Instrumentation,
        owner: "aura-agent-invitation",
        design_ref: "docs/804_testing_guide.md",
    },
    HarnessModeAllowance {
        path: "crates/aura-agent/src/runtime/effects.rs",
        kind: HarnessModeChangeKind::Instrumentation,
        owner: "aura-agent-runtime-effects",
        design_ref: "docs/804_testing_guide.md",
    },
    HarnessModeAllowance {
        path: "crates/aura-agent/src/runtime_bridge/mod.rs",
        kind: HarnessModeChangeKind::Instrumentation,
        owner: "aura-agent-runtime-bridge",
        design_ref: "docs/804_testing_guide.md",
    },
    HarnessModeAllowance {
        path: "crates/aura-terminal/src/tui/context/io_context.rs",
        kind: HarnessModeChangeKind::Instrumentation,
        owner: "aura-terminal-tui-context",
        design_ref: "crates/aura-terminal/ARCHITECTURE.md",
    },
    HarnessModeAllowance {
        path: "crates/aura-web/src/main.rs",
        kind: HarnessModeChangeKind::Instrumentation,
        owner: "aura-web-main",
        design_ref: "docs/804_testing_guide.md",
    },
    HarnessModeAllowance {
        path: "crates/aura-web/src/shell/maintenance.rs",
        kind: HarnessModeChangeKind::TimingDiscipline,
        owner: "aura-web-browser-maintenance",
        design_ref: "crates/aura-web/ARCHITECTURE.md",
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FrontendExecutionBoundaryKind {
    DriverBackend,
    ScenarioExecutor,
    ScenarioEntrypoint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrontendExecutionBoundary {
    pub path: &'static str,
    pub kind: FrontendExecutionBoundaryKind,
    pub owner: &'static str,
}

pub const FRONTEND_EXECUTION_BOUNDARIES: &[FrontendExecutionBoundary] = &[
    FrontendExecutionBoundary {
        path: "crates/aura-harness/src/backend/local_pty.rs",
        kind: FrontendExecutionBoundaryKind::DriverBackend,
        owner: "aura-harness-backend-local-pty",
    },
    FrontendExecutionBoundary {
        path: "crates/aura-harness/src/backend/playwright_browser.rs",
        kind: FrontendExecutionBoundaryKind::DriverBackend,
        owner: "aura-harness-backend-playwright",
    },
    FrontendExecutionBoundary {
        path: "crates/aura-harness/src/executor.rs",
        kind: FrontendExecutionBoundaryKind::ScenarioExecutor,
        owner: "aura-harness-executor",
    },
    FrontendExecutionBoundary {
        path: "scripts/harness/run-matrix.sh",
        kind: FrontendExecutionBoundaryKind::ScenarioEntrypoint,
        owner: "aura-harness-matrix",
    },
    FrontendExecutionBoundary {
        path: ".github/workflows/harness.yml",
        kind: FrontendExecutionBoundaryKind::ScenarioEntrypoint,
        owner: "aura-harness-ci",
    },
];
