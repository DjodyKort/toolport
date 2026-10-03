//! Expired-login state machine (A7): pure classification and transitions, no I/O.

mod issues;
mod machine;
mod types;

pub use issues::{compute_issues, AuthIssue};
pub use machine::{
    classify, step, Classification, EXPIRING_WINDOW_SECS, UNREACHABLE_MIN_FAILURES,
    UNREACHABLE_WINDOW_SECS,
};
pub use types::{AuthState, ProbeOutcome, Tracked, TransientRun};

#[cfg(test)]
mod tests;
