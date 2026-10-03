//! Expired-login state machine (A7): pure classification and transitions, no I/O.

mod cache;
mod flight;
mod gateway_state;
mod google;
mod http_probes;
mod issues;
mod machine;
mod prober;
mod probe;
pub mod surfaces;
mod types;

pub use cache::{AuthStore, EdgeEvent, ServerEntry, StatusFile};
pub use flight::SingleFlight;
pub use gateway_state::{gateway_registry, GatewayStateProbe};
pub use google::{probe_handler, GoogleRefreshProbe};
pub use http_probes::{combined_registry, http_registry, CompositeProbe, HttpProbe};
pub use issues::{compute_issues, AuthIssue};
pub use machine::{
    classify, step, Classification, EXPIRING_WINDOW_SECS, UNREACHABLE_MIN_FAILURES,
    UNREACHABLE_WINDOW_SECS,
};
pub use probe::{
    Clock, FakeClock, MockProbe, Probe, ProbeKind, ProbeRegistry, ProbeSpec, SystemClock,
};
pub use surfaces::rows_handler;
pub use prober::{backoff_delay, probe_due, status_handler, AuthProber, ProbeReport, Trigger};
pub use types::{AuthState, ProbeOutcome, Tracked, TransientRun};

#[cfg(test)]
mod tests;

#[cfg(test)]
mod probe_tests;

#[cfg(test)]
mod google_tests;

#[cfg(test)]
mod http_probes_tests;

#[cfg(test)]
mod surfaces_tests;

#[cfg(test)]
mod gateway_e2e_tests;
