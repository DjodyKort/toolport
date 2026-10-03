use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum AuthState {
    Unknown,
    Ok,
    Expiring { eta: i64 },
    NeedsReauth,
    Revoked,
    Misconfigured,
    Unreachable,
}

impl AuthState {
    pub fn name(&self) -> &'static str {
        match self {
            AuthState::Unknown => "unknown",
            AuthState::Ok => "ok",
            AuthState::Expiring { .. } => "expiring",
            AuthState::NeedsReauth => "needs_reauth",
            AuthState::Revoked => "revoked",
            AuthState::Misconfigured => "misconfigured",
            AuthState::Unreachable => "unreachable",
        }
    }

    pub fn is_issue(&self) -> bool {
        !matches!(self, AuthState::Unknown | AuthState::Ok)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProbeOutcome {
    Success,
    OauthError { code: String, description: String },
    HttpStatus { status: u16 },
    TransportError,
    FileChanged,
    TokenTtl { secs: i64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransientRun {
    pub count: u32,
    pub first_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tracked {
    #[serde(flatten)]
    pub state: AuthState,
    pub reason: String,
    pub since: i64,
    pub transient: Option<TransientRun>,
}

impl Tracked {
    pub fn unknown(now: i64) -> Self {
        Tracked {
            state: AuthState::Unknown,
            reason: "unknown".into(),
            since: now,
            transient: None,
        }
    }
}
