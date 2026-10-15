//! Opt-in startup milestones. Uses stderr, like the existing native diagnostics.
use std::sync::OnceLock;
use std::time::Instant;

static START: OnceLock<Instant> = OnceLock::new();

/// With `HARNESS_STARTUP_TRACE=1`, prints `step` with the elapsed time since main entry (the
/// first mark, after the helper child processes have been dispatched).
pub fn mark(step: &str) {
    let start = START.get_or_init(Instant::now);
    if std::env::var_os("HARNESS_STARTUP_TRACE").is_some_and(|v| v == "1") {
        eprintln!(
            "[startup] {:>10.3} ms {step}",
            start.elapsed().as_secs_f64() * 1000.0
        );
    }
}
