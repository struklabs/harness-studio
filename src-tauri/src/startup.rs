//! Opt-in startup milestones. Uses stderr, like the existing native diagnostics.
use std::sync::OnceLock;
use std::time::Instant;

static START: OnceLock<Instant> = OnceLock::new();

pub fn mark(step: &str) {
    let start = START.get_or_init(Instant::now);
    if std::env::var_os("HARNESS_STARTUP_TRACE").is_some_and(|v| v == "1") {
        eprintln!(
            "[startup] {:>10.3} ms {step}",
            start.elapsed().as_secs_f64() * 1000.0
        );
    }
}
