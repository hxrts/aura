//! Type-level stdio policy for the fullscreen TUI.
//!
//! Goal: make it difficult to accidentally write to stdout/stderr while iocraft
//! is running fullscreen.
//!
//! - All stdout/stderr writes in the TUI launcher must go through these tokens.
//! - The pre-fullscreen token is consumed while fullscreen is running, so there
//!   is no stdio capability available in that scope.
//! - Tokens are `!Send` to prevent moving them into background tasks.

use std::fmt;
use std::future::Future;
use std::rc::Rc;

use crate::tui::fullscreen_stdio::FullscreenStdioGuard;

pub struct PreFullscreenStdio {
    _no_send: Rc<()>,
}

pub struct PostFullscreenStdio {
    _no_send: Rc<()>,
}

impl Default for PreFullscreenStdio {
    fn default() -> Self {
        Self {
            _no_send: Rc::new(()),
        }
    }
}

impl PreFullscreenStdio {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[allow(clippy::print_stdout)]
    pub fn println(&self, args: fmt::Arguments<'_>) {
        println!("{args}");
    }

    #[allow(clippy::print_stdout)]
    pub fn newline(&self) {
        println!();
    }

    #[allow(clippy::print_stderr)]
    pub fn eprintln(&self, args: fmt::Arguments<'_>) {
        eprintln!("{args}");
    }
}

impl PostFullscreenStdio {
    #[allow(clippy::print_stdout)]
    pub fn println(&self, args: fmt::Arguments<'_>) {
        println!("{args}");
    }

    #[allow(clippy::print_stdout)]
    pub fn newline(&self) {
        println!();
    }

    #[allow(clippy::print_stderr)]
    pub fn eprintln(&self, args: fmt::Arguments<'_>) {
        eprintln!("{args}");
    }
}

/// Run a future while the fullscreen TUI is active.
///
/// Consumes the pre-fullscreen stdio token, making it unavailable while `fut`
/// is awaited. Returns a post-fullscreen token afterwards.
pub async fn during_fullscreen<R, Fut>(_: PreFullscreenStdio, fut: Fut) -> (PostFullscreenStdio, R)
where
    Fut: Future<Output = R>,
{
    // While fullscreen is active, keep stderr redirected away from the terminal
    // to avoid scroll artifacts (e.g., a "duplicated" nav bar).
    //
    // `AURA_TUI_ALLOW_STDIO=1` disables redirection for debugging.
    let _stdio_guard = if std::env::var("AURA_TUI_ALLOW_STDIO").ok().as_deref() == Some("1") {
        None
    } else {
        FullscreenStdioGuard::redirect_stderr_to_null().ok()
    };

    let result = fut.await;
    (
        PostFullscreenStdio {
            _no_send: Rc::new(()),
        },
        result,
    )
}
