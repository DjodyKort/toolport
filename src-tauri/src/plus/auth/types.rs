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

/// The state without its payload, as every surface names it (`AuthStateName` in `src/plus/api.ts`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthKind {
    Unknown,
    Ok,
    Expiring,
    NeedsReauth,
    Revoked,
    Misconfigured,
    Unreachable,
}

impl AuthKind {
    pub fn as_str(self) -> &'static str {
        match self {
            AuthKind::Unknown => "unknown",
            AuthKind::Ok => "ok",
            AuthKind::Expiring => "expiring",
            AuthKind::NeedsReauth => "needs_reauth",
            AuthKind::Revoked => "revoked",
            AuthKind::Misconfigured => "misconfigured",
            AuthKind::Unreachable => "unreachable",
        }
    }

    pub fn parse(name: &str) -> Option<AuthKind> {
        [
            AuthKind::Unknown,
            AuthKind::Ok,
            AuthKind::Expiring,
            AuthKind::NeedsReauth,
            AuthKind::Revoked,
            AuthKind::Misconfigured,
            AuthKind::Unreachable,
        ]
        .into_iter()
        .find(|kind| kind.as_str() == name)
    }
}

impl std::fmt::Display for AuthKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl AuthState {
    pub fn kind(&self) -> AuthKind {
        match self {
            AuthState::Unknown => AuthKind::Unknown,
            AuthState::Ok => AuthKind::Ok,
            AuthState::Expiring { .. } => AuthKind::Expiring,
            AuthState::NeedsReauth => AuthKind::NeedsReauth,
            AuthState::Revoked => AuthKind::Revoked,
            AuthState::Misconfigured => AuthKind::Misconfigured,
            AuthState::Unreachable => AuthKind::Unreachable,
        }
    }

    pub fn name(&self) -> &'static str {
        self.kind().as_str()
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
