use super::*;

/// Type-safe handle for TuiState that enforces proper reactivity patterns.
///
/// # Reactivity Model
///
/// iocraft's `Ref<T>` (from `use_ref`) does NOT trigger re-renders when modified.
/// iocraft's `State<T>` (from `use_state`) DOES trigger re-renders when `.set()` is called,
/// but ONLY if the component read the State during render via `.get()`.
///
/// This handle enforces the correct pattern:
/// - **During render**: Use `read_for_render()` which reads the version (establishing reactivity)
///   and returns a snapshot of the state. This is the ONLY way to read state during render.
/// - **During callbacks**: Use `replace()` or `with_mut()` which update the Ref and bump the
///   version, triggering a re-render.
///
/// # Compile-Time Safety
///
/// By making `read_for_render()` the only way to access state during render, we ensure
/// the version is always read. If you try to bypass this (e.g., holding onto the raw Ref),
/// you won't have a `TuiStateSnapshot` to pass to prop extraction functions.
#[derive(Clone)]
pub(super) struct TuiStateHandle {
    state: Ref<TuiState>,
    version: State<usize>,
}

impl TuiStateHandle {
    pub(super) fn new(state: Ref<TuiState>, version: State<usize>) -> Self {
        Self { state, version }
    }

    fn bump(&mut self) {
        self.version.set(self.version.get().wrapping_add(1));
    }

    /// Read state for rendering. This MUST be used during the render phase.
    ///
    /// This method:
    /// 1. Reads `version.get()` to establish reactivity (so the component re-renders when
    ///    `replace()` or `with_mut()` are called)
    /// 2. Returns a `TuiRenderState` that provides access to the TuiState
    ///
    /// # Why This Exists
    ///
    /// iocraft only re-renders a component when a `State<T>` it read during render changes.
    /// The TuiState lives in a `Ref<T>` which doesn't trigger re-renders. We use a separate
    /// `State<usize>` version counter that gets bumped on every state change.
    ///
    /// By reading the version here, we subscribe to changes. By returning a TuiRenderState,
    /// we ensure all render-time state access goes through this method.
    ///
    /// # Type Safety
    ///
    /// The returned `TuiRenderState` can only be created via this method, ensuring the
    /// version is always read during render. This makes the "forgot to read version" bug
    /// impossible - you can't access TuiState for rendering without going through here.
    pub(super) fn read_for_render(&self) -> TuiRenderState {
        // Read version to establish reactivity - this is the key to making re-renders work!
        let _version = self.version.get();
        TuiRenderState {
            state: self.state.read().clone(),
        }
    }

    /// Clone the current state (for use in event handlers where you need ownership).
    ///
    /// Note: This does NOT read the version, so it should only be used in callbacks,
    /// not during render. For render-time access, use `read_for_render()`.
    pub(super) fn read_clone(&self) -> TuiState {
        self.state.read().clone()
    }

    pub(super) fn with_mut<R>(&mut self, f: impl FnOnce(&mut TuiState) -> R) -> R {
        let mut guard = self.state.write();
        let out = f(&mut guard);
        drop(guard);
        self.bump();
        out
    }

    pub(super) fn replace(&mut self, new_state: TuiState) {
        self.with_mut(|state| *state = new_state);
    }

    /// Advance the active toast timer without forcing a full re-render on every tick.
    ///
    /// We only bump the render version when a toast is actually dismissed.
    pub(super) fn tick_active_toast_timer(&mut self) {
        let dismissed = {
            let mut guard = self.state.write();
            guard.toast_queue.tick()
        };
        if dismissed {
            self.bump();
        }
    }
}

/// A render-time state snapshot that can only be created via `TuiStateHandle::read_for_render()`.
///
/// This type enforces that the version State is read during render (establishing reactivity).
/// All render-time access to TuiState must go through this type.
///
/// The state is cloned once per render, which is acceptable since:
/// - Render happens at most once per frame (~60Hz)
/// - The state is read-only during render anyway
/// - This avoids unsafe code and keeps the API clean
///
/// Implements `Deref<Target = TuiState>` for convenient access to state fields.
pub(super) struct TuiRenderState {
    state: TuiState,
}

impl std::ops::Deref for TuiRenderState {
    type Target = TuiState;
    fn deref(&self) -> &TuiState {
        &self.state
    }
}

pub(super) fn sync_neighborhood_navigation_state(
    state: &mut TuiState,
    shared_homes: &Arc<std::sync::RwLock<Vec<String>>>,
    shared_channels: &Arc<std::sync::RwLock<Vec<Channel>>>,
    shared_home_meta: &SharedNeighborhoodHomeMeta,
) {
    let (home_count, selected_home_id, local_home_id) = shared_homes
        .read()
        .map(|guard| {
            let count = guard.len();
            let selected = guard.get(state.neighborhood.grid.current()).cloned();
            let local = guard.first().cloned();
            (count, selected, local)
        })
        .unwrap_or((0, None, None));
    state.neighborhood.home_count = home_count;
    // Neighborhood map is currently rendered as a single-column list.
    // Keep GridNav columns non-zero so map navigation works.
    state.neighborhood.grid.set_cols(1);
    state.neighborhood.grid.set_count(home_count);
    state.neighborhood.selected_home = state.neighborhood.grid.current();

    let is_local_home_selected = selected_home_id == local_home_id;
    let is_selected_home_entered = selected_home_id
        .as_ref()
        .map(|selected| state.neighborhood.entered_home_id.as_ref() == Some(selected))
        .unwrap_or(false);
    let is_detail_mode = matches!(
        state.neighborhood.mode,
        crate::tui::state_machine::NeighborhoodMode::Detail
    );
    let expose_remote_home_details = is_selected_home_entered
        && is_detail_mode
        && matches!(state.neighborhood.enter_depth, AccessLevel::Full);
    let expose_home_details = is_local_home_selected || expose_remote_home_details;

    let channel_count = if expose_home_details {
        shared_channels.read().map(|guard| guard.len()).unwrap_or(0)
    } else {
        0
    };
    state.neighborhood.channel_count = channel_count;
    state.neighborhood.selected_channel =
        clamp_list_index(state.neighborhood.selected_channel, channel_count);

    let home_meta = shared_home_meta
        .read()
        .map(|guard| *guard)
        .unwrap_or_default();
    let member_count = if expose_home_details {
        home_meta.member_count
    } else {
        0
    };
    state.neighborhood.member_count = member_count;
    state.neighborhood.moderator_actions_enabled = if expose_home_details {
        home_meta.moderator_actions_enabled
    } else {
        false
    };
    state.neighborhood.selected_member =
        clamp_list_index(state.neighborhood.selected_member, member_count);
}
