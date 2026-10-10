//! Renderer-issued observation of actual committed control layout.
//! These observations identify rendered UI, never authorize business mutation.
use super::{ControlId, ProjectionRevision, UiSnapshot};
use crate::core::ProjectionSourceRevisions;
use serde::{Deserialize, Serialize};

/// Exact semantic snapshot source captured before renderer commit. Transport
/// render sequence numbers are added after publication and are not this source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlRenderSource {
    pub revision: ProjectionRevision,
    pub projections: ProjectionSourceRevisions,
}
impl ControlRenderSource {
    pub fn from_snapshot(snapshot: &UiSnapshot) -> Self {
        let mut revision = snapshot.revision;
        revision.render_seq = None;
        Self {
            revision,
            projections: snapshot.projection_source_revisions,
        }
    }
    pub fn matches(&self, snapshot: &UiSnapshot) -> bool {
        self == &Self::from_snapshot(snapshot)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlLayoutState {
    Visible,
    Hidden,
    Disposed,
    ZeroExtent,
    Unavailable,
}
/// Positive visibility requires a renderer commit and a live, nonhidden control
/// with nonempty clipped rendered layout. Missing controls remain unsupported.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenderedControlObservation {
    pub control: ControlId,
    pub capture_revision: u64,
    pub source: ControlRenderSource,
    pub state: ControlLayoutState,
}
/// Pure result from the browser/native actual node/layout adapter, not a
/// substitute for renderer commit. Bounds are already intersected with clipping.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderedControlLayout {
    pub connected: bool,
    pub hidden: bool,
    pub width: f64,
    pub height: f64,
}
/// One original source capture. Commit and measurement must refer to that same
/// renderer-owned capture; disposal is irreversible and rejects late callbacks.
pub struct ControlRenderCapture {
    control: ControlId,
    source: ControlRenderSource,
    capture_revision: u64,
    committed: bool,
    disposed: bool,
}
impl ControlRenderCapture {
    pub fn new(control: ControlId, source: ControlRenderSource, capture_revision: u64) -> Self {
        Self {
            control,
            source,
            capture_revision,
            committed: false,
            disposed: false,
        }
    }
    pub fn acknowledge_renderer_commit(&mut self, source: &ControlRenderSource) -> bool {
        if self.disposed || source != &self.source {
            return false;
        }
        self.committed = true;
        true
    }
    pub fn dispose(&mut self) {
        self.disposed = true;
    }
    pub fn observe(
        &self,
        current: &UiSnapshot,
        layout: RenderedControlLayout,
    ) -> RenderedControlObservation {
        let state = if self.disposed || !layout.connected {
            ControlLayoutState::Disposed
        } else if !self.committed || !self.source.matches(current) {
            ControlLayoutState::Unavailable
        } else if layout.hidden {
            ControlLayoutState::Hidden
        } else if !layout.width.is_finite()
            || !layout.height.is_finite()
            || layout.width <= 0.0
            || layout.height <= 0.0
        {
            ControlLayoutState::ZeroExtent
        } else {
            ControlLayoutState::Visible
        };
        RenderedControlObservation {
            control: self.control,
            capture_revision: self.capture_revision,
            source: self.source.clone(),
            state,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui_contract::ScreenId;
    fn layout() -> RenderedControlLayout {
        RenderedControlLayout {
            connected: true,
            hidden: false,
            width: 80.0,
            height: 20.0,
        }
    }
    #[test]
    fn eligible_mounted_focused_and_nonempty_layout_do_not_replace_renderer_commit() {
        let mut snapshot = UiSnapshot::loading(ScreenId::Onboarding);
        snapshot.focused_control = Some(ControlId::OnboardingRoot);
        let source = ControlRenderSource::from_snapshot(&snapshot);
        let mut capture = ControlRenderCapture::new(ControlId::OnboardingRoot, source.clone(), 1);
        assert_eq!(
            capture.observe(&snapshot, layout()).state,
            ControlLayoutState::Unavailable
        );
        assert!(capture.acknowledge_renderer_commit(&source));
        assert_eq!(
            capture.observe(&snapshot, layout()).state,
            ControlLayoutState::Visible
        );
    }
    #[test]
    fn disposed_hidden_zero_and_stale_layout_never_resurrect_visibility() {
        let snapshot = UiSnapshot::loading(ScreenId::Onboarding);
        let source = ControlRenderSource::from_snapshot(&snapshot);
        let mut capture = ControlRenderCapture::new(ControlId::OnboardingRoot, source.clone(), 1);
        assert!(capture.acknowledge_renderer_commit(&source));
        let mut hidden = layout();
        hidden.hidden = true;
        assert_eq!(
            capture.observe(&snapshot, hidden).state,
            ControlLayoutState::Hidden
        );
        let mut zero = layout();
        zero.width = 0.0;
        assert_eq!(
            capture.observe(&snapshot, zero).state,
            ControlLayoutState::ZeroExtent
        );
        let mut stale = snapshot.clone();
        stale.revision.semantic_seq += 1;
        assert_eq!(
            capture.observe(&stale, layout()).state,
            ControlLayoutState::Unavailable
        );
        let foreign = ControlRenderSource::from_snapshot(&stale);
        assert!(!capture.acknowledge_renderer_commit(&foreign));
        capture.dispose();
        assert!(!capture.acknowledge_renderer_commit(&source));
        assert_eq!(
            capture.observe(&snapshot, layout()).state,
            ControlLayoutState::Disposed
        );
    }
}

#[cfg(test)]
mod renderer_revision_tests {
    use super::*;
    use crate::ui_contract::ScreenId;
    #[test]
    fn same_semantic_source_hidden_rerender_disposes_original_capture() {
        let snapshot = UiSnapshot::loading(ScreenId::Onboarding);
        let source = ControlRenderSource::from_snapshot(&snapshot);
        let mut old = ControlRenderCapture::new(ControlId::OnboardingRoot, source.clone(), 1);
        assert!(old.acknowledge_renderer_commit(&source));
        old.dispose();
        let mut current = ControlRenderCapture::new(ControlId::OnboardingRoot, source.clone(), 2);
        assert!(current.acknowledge_renderer_commit(&source));
        let layout = RenderedControlLayout {
            connected: true,
            hidden: true,
            width: 80.0,
            height: 20.0,
        };
        assert_eq!(
            current.observe(&snapshot, layout).state,
            ControlLayoutState::Hidden
        );
        assert_eq!(current.observe(&snapshot, layout).capture_revision, 2);
        assert!(!old.acknowledge_renderer_commit(&source));
        assert_eq!(
            old.observe(
                &snapshot,
                RenderedControlLayout {
                    hidden: false,
                    ..layout
                }
            )
            .state,
            ControlLayoutState::Disposed
        );
    }
}

/// Evidence that one complete renderer batch was applied, including a batch
/// containing no controls. Invalidation is never an empty committed frame.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RenderedControlFrame {
    Unavailable,
    Committed {
        capture_revision: u64,
        layout_revision: u64,
        source: ControlRenderSource,
    },
}
