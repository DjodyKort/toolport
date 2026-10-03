use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::types::{AuthState, Tracked};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthIssue {
    pub id: String,
    pub server: String,
    #[serde(flatten)]
    pub state: AuthState,
    pub reason: String,
    pub since: i64,
    pub fix_hint: String,
}

fn fix_hint(server: &str, state: &AuthState) -> String {
    match state {
        AuthState::Expiring { .. } => format!("Re-authenticate {server} before the login expires."),
        AuthState::NeedsReauth => format!("Sign in to {server} again."),
        AuthState::Revoked => {
            format!("Access to {server} was revoked; sign in again and re-consent.")
        }
        AuthState::Misconfigured => {
            format!("Check the OAuth client configuration of {server}.")
        }
        AuthState::Unreachable => {
            format!("{server} is not reachable; check the network or service.")
        }
        AuthState::Unknown | AuthState::Ok => String::new(),
    }
}

pub fn compute_issues(states: &BTreeMap<String, Tracked>) -> Vec<AuthIssue> {
    let mut issues: Vec<AuthIssue> = states
        .iter()
        .filter(|(_, tracked)| tracked.state.is_issue())
        .map(|(server, tracked)| AuthIssue {
            id: format!("{server}:{}:{}", tracked.state.name(), tracked.reason),
            server: server.clone(),
            state: tracked.state,
            reason: tracked.reason.clone(),
            since: tracked.since,
            fix_hint: fix_hint(server, &tracked.state),
        })
        .collect();
    issues.sort_by(|a, b| a.id.cmp(&b.id));
    issues
}
