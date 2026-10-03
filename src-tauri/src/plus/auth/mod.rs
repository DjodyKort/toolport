//! Expired-login state machine (A7): pure classification and transitions, no I/O.

mod cache;
mod flight;
mod issues;
mod machine;
mod prober;
mod probe;
mod types;

pub use cache::{AuthStore, EdgeEvent, ServerEntry, StatusFile};
pub use flight::SingleFlight;
pub use issues::{compute_issues, AuthIssue};
pub use machine::{
    classify, step, Classification, EXPIRING_WINDOW_SECS, UNREACHABLE_MIN_FAILURES,
    UNREACHABLE_WINDOW_SECS,
};
pub use probe::{
    Clock, FakeClock, MockProbe, Probe, ProbeKind, ProbeRegistry, ProbeSpec, SystemClock,
};
pub use prober::{backoff_delay, probe_due, status_handler, AuthProber, ProbeReport, Trigger};
pub use types::{AuthState, ProbeOutcome, Tracked, TransientRun};

#[cfg(test)]
mod tests;

#[cfg(test)]
mod probe_tests;
