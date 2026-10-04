//! Expired-login state machine (A7): pure classification and transitions, no I/O.

mod cache;
mod flight;
mod gateway_state;
mod google;
mod http_probes;
mod issues;
pub mod login;
mod machine;
pub mod notify;
mod prober;
mod probe;
pub mod scan;
pub mod scheduler;
pub mod stdio;
pub mod surfaces;
mod types;

pub use cache::{AuthStore, EdgeEvent, ServerEntry, StatusFile};
pub use flight::SingleFlight;
pub use gateway_state::{gateway_registry, GatewayStateProbe};
pub use google::GoogleRefreshProbe;
pub use http_probes::{combined_registry, http_registry, CompositeProbe, HttpProbe};
pub use issues::{compute_issues, AuthIssue};
pub use machine::{
    classify, step, Classification, EXPIRING_WINDOW_SECS, UNREACHABLE_MIN_FAILURES,
    UNREACHABLE_WINDOW_SECS,
};
pub use probe::{
    Clock, FakeClock, MockProbe, Probe, ProbeKind, ProbeRegistry, ProbeSpec, SystemClock,
};
pub use login::login_handler;
pub use notify::notifications_handler;
pub use surfaces::rows_handler;
pub use prober::{
    backoff_delay, probe_all, probe_due, status_handler, AuthProber, ProbeReport, Trigger,
};
pub use scan::probe_handler;
pub use types::{AuthKind, AuthState, ProbeOutcome, Tracked, TransientRun};

const MAX_BODY_BYTES: u64 = 64 * 1024;

fn agent(timeout: std::time::Duration) -> ureq::Agent {
    ureq::AgentBuilder::new()
        .redirects(0)
        .timeout(timeout)
        .build()
}

fn read_capped(response: ureq::Response) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut buf = Vec::new();
    response
        .into_reader()
        .take(MAX_BODY_BYTES)
        .read_to_end(&mut buf)?;
    Ok(buf)
}

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

#[cfg(all(test, unix))]
mod gateway_e2e_tests;

#[cfg(test)]
pub(crate) mod testkit;

#[cfg(test)]
mod stdio_tests;

#[cfg(test)]
mod scan_tests;

#[cfg(test)]
mod scheduler_tests;

#[cfg(test)]
mod login_tests;

#[cfg(test)]
mod notify_tests;
