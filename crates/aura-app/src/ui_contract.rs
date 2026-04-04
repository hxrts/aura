//! Shared UI-facing semantic contract for Aura frontends and harnesses.
//!
//! This module defines stable application-facing UI identifiers and snapshot
//! types that can be shared across the web UI, TUI, and harness tooling.

#![allow(missing_docs)] // Shared contract surface - refined incrementally during migration.

mod harness_metadata;
#[allow(dead_code)]
mod legacy;
mod parity;
mod shared_flow_support;

pub use harness_metadata::{
    BrowserCacheBoundary, BrowserCacheBoundaryMetadata, BrowserHarnessBridgeMethod,
    BrowserHarnessBridgeMethodKind, FrontendExecutionBoundary, FrontendExecutionBoundaryKind,
    FrontendId, HarnessModeAllowance, HarnessModeChangeKind, HarnessObservationSurface,
    ObservationMethodKind, ObservationSurfaceMethod, BROWSER_CACHE_BOUNDARIES,
    BROWSER_HARNESS_BRIDGE_API_VERSION, BROWSER_HARNESS_BRIDGE_METHODS,
    BROWSER_OBSERVATION_SURFACE_API_VERSION, BROWSER_OBSERVATION_SURFACE_GLOBAL,
    BROWSER_OBSERVATION_SURFACE_METHODS, FRONTEND_EXECUTION_BOUNDARIES, HARNESS_MODE_ALLOWLIST,
    TUI_OBSERVATION_SURFACE_API_VERSION, TUI_OBSERVATION_SURFACE_METHODS,
};
pub use legacy::{
    bridged_operation_statuses, classify_screen_item_id,
    classify_semantic_settings_section_item_id, classify_settings_section_item_id,
    contacts_friend_action_controls, list_item_dom_id, list_item_selector,
    nav_control_id_for_screen, next_projection_revision, screen_item_id,
    semantic_settings_section_item_id, semantic_settings_section_surface_id,
    settings_section_item_id, validate_harness_shell_structure, validate_render_convergence,
    AcceptedPendingChannelBinding, AuthoritativeSemanticFact, AuthoritativeSemanticFactKind,
    AuthoritativeSemanticFactsSnapshot, ChannelBindingWitness, ChannelFactKey, ConfirmationState,
    ControlId, FieldId, FrontendSpecificSettingsSectionId, HarnessShellMode,
    HarnessShellStructureSnapshot, HarnessUiCommand, HarnessUiCommandReceipt,
    HarnessUiOperationHandle, InvitationFactKind, ListId, ListItemSnapshot, ListSnapshot,
    MessageSnapshot, ModalId, OperationId, OperationInstanceId, OperationSnapshot, OperationState,
    ParityUiIdentity, ProjectionRevision, QuiescenceSnapshot, QuiescenceState, RenderHeartbeat,
    RuntimeEventId, RuntimeEventKind, RuntimeEventSnapshot, RuntimeFact, ScreenId,
    SelectionSnapshot, SemanticFailureCode, SemanticFailureDomain, SemanticOperationCausality,
    SemanticOperationError, SemanticOperationKind, SemanticOperationPhase, SemanticOperationStatus,
    SettingsSectionSurfaceId, SharedSettingsSectionId, ToastId, ToastKind, ToastSnapshot,
    UiReadiness, UiSnapshot, WorkflowTerminalOutcome, WorkflowTerminalStatus,
    FRONTEND_SPECIFIC_SETTINGS_SECTIONS, PARITY_CRITICAL_SETTINGS_SECTIONS,
};
pub use parity::{
    compare_ui_snapshots_for_parity, uncovered_ui_parity_mismatches, UiParityMismatch,
};
pub use shared_flow_support::{
    shared_flow_scenarios, shared_flow_source_areas, shared_flow_support, shared_list_support,
    shared_modal_support, shared_screen_module_map, shared_screen_support, FlowAvailability,
    ParityException, ParityExceptionMetadata, SharedFlowId, SharedFlowScenarioCoverage,
    SharedFlowSourceArea, SharedFlowSupport, SharedListSupport, SharedModalSupport,
    SharedScreenModuleMap, SharedScreenSupport, ALL_SHARED_FLOW_IDS, PARITY_EXCEPTION_METADATA,
    SHARED_FLOW_SCENARIO_COVERAGE, SHARED_FLOW_SOURCE_AREAS, SHARED_FLOW_SUPPORT,
    SHARED_LIST_SUPPORT, SHARED_MODAL_SUPPORT, SHARED_SCREEN_MODULE_MAP, SHARED_SCREEN_SUPPORT,
};
