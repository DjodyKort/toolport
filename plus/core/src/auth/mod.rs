//! Login health of the servers that sign in outside upstream's own OAuth: stdio servers with an
//! `auth` subcommand and the Google Workspace ones. Remote OAuth servers show their state in
//! upstream's server row and are not probed here.

mod google;
mod login;
mod machine;
mod probe;
mod prober;
mod stdio;
mod store;
mod surfaces;
mod types;

pub use google::{endpoint_allowed, GoogleRefreshProbe};
pub use login::{
    follow_up, login, plan, report_value as login_report_value, review_gate, start_sign_in,
    FollowUp, LoginError, LoginOptions, LoginReport, Plan, UrlSink,
};
pub use probe::{ProbeKind, ProbeRegistry, ProbeSpec};
pub use prober::{
    auth_dir, backoff_delay, probe_registry, read_servers, report_value, status_section,
    visible_servers, ProbeFailure, ProbeReport, ProbeRun, Prober, Selector, Trigger,
};
pub use stdio::{find_consent_url, Capture, StdioProbe};
pub use store::{AuthStore, StatusFile};
pub use surfaces::{counts, hook, rows_for, status_summary, statusline, AuthCounts, AuthRow};
pub use types::{AuthKind, AuthState, ProbeOutcome, Tracked};

#[cfg(test)]
mod machine_tests;
#[cfg(test)]
mod tests;
