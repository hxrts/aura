// Synchronous custody transitions shared by runtime error and ACK owners.
// The guard is private and cannot escape through the callback's return type.

pub(crate) struct SynchronousCustodyCell<T> {
    #[expect(
        clippy::disallowed_types,
        reason = "Runtime synchronous custody state; private guard never escapes a non-async callback"
    )]
    state: parking_lot::Mutex<T>,
}

impl<T> SynchronousCustodyCell<T> {
    #[expect(
        clippy::disallowed_types,
        reason = "Canonical synchronous runtime custody constructor; no await or exported guard"
    )]
    pub(crate) fn new(value: T) -> Self {
        Self {
            state: parking_lot::Mutex::new(value),
        }
    }

    pub(crate) fn with<R>(&self, transition: impl FnOnce(&mut T) -> R) -> R {
        transition(&mut self.state.lock())
    }
}

#[cfg(test)]
mod tests {
    use super::SynchronousCustodyCell;

    #[test]
    fn original_value_moves_once_without_exporting_a_guard() {
        let original = Box::new(73u64);
        let pointer = std::ptr::from_ref(original.as_ref());
        let cell = SynchronousCustodyCell::new(Some(original));
        let Some(retained) = cell.with(Option::take) else {
            panic!("original retained value");
        };
        assert!(std::ptr::eq(pointer, retained.as_ref()));
        assert!(cell.with(Option::take).is_none());
    }

    #[test]
    fn callback_unwind_preserves_nonpoisoning_original_state() {
        let cell = SynchronousCustodyCell::new(Some(Box::new(91u64)));
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            cell.with(|_| panic!("original transition panic"));
        }));
        assert!(result.is_err());
        assert_eq!(
            cell.with(|value| match value.as_ref() {
                Some(original) => **original,
                None => panic!("original state remains retained"),
            }),
            91
        );
    }
}
