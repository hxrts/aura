use super::*;

pub(super) const DISPLAY_CLOCK_MAX_CONSECUTIVE_FAILURES: u32 = 200;
pub(super) const DISPLAY_CLOCK_POLL_INTERVAL: Duration = Duration::from_millis(1000);

/// Best-effort physical display clock for relative-time formatting only.
///
/// This state is explicitly observed-only UI maintenance. It must not gate or
/// synthesize parity-critical lifecycle semantics. On repeated failures it
/// stops updating rather than retrying forever during shutdown.
pub fn use_display_clock_state(hooks: &mut Hooks, app_ctx: &AppCoreContext) -> State<Option<u64>> {
    let now_ms = hooks.use_state(|| None::<u64>);
    hooks.use_future({
        let app_core = app_ctx.app_core.clone();
        let mut now_ms = now_ms.clone();
        async move {
            let time = PhysicalTimeHandler::new();
            let mut consecutive_failures = 0u32;
            loop {
                match time_workflows::current_time_ms(app_core.raw()).await {
                    Ok(ts) => {
                        let next = Some(ts);
                        if now_ms.get() != next {
                            now_ms.set(next);
                        }
                        consecutive_failures = 0;
                    }
                    Err(_) => {
                        consecutive_failures += 1;
                    }
                }
                if consecutive_failures > DISPLAY_CLOCK_MAX_CONSECUTIVE_FAILURES {
                    break;
                }
                let _ = time
                    .sleep_ms(DISPLAY_CLOCK_POLL_INTERVAL.as_millis() as u64)
                    .await;
            }
        }
    });
    now_ms
}
