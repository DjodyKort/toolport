use super::types::{AuthState, ProbeOutcome, Tracked, TransientRun};

pub const UNREACHABLE_MIN_FAILURES: u32 = 3;
pub const UNREACHABLE_WINDOW_SECS: i64 = 30 * 60;
pub const EXPIRING_WINDOW_SECS: i64 = 24 * 60 * 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Classification {
    Healthy,
    Ttl {
        secs: i64,
    },
    Definitive {
        state: AuthState,
        reason: &'static str,
    },
    Transient,
}

pub fn classify(outcome: &ProbeOutcome) -> Classification {
    match outcome {
        ProbeOutcome::Success | ProbeOutcome::FileChanged => Classification::Healthy,
        ProbeOutcome::TokenTtl { secs } => Classification::Ttl { secs: *secs },
        ProbeOutcome::HttpStatus { status } if (200..300).contains(status) => {
            Classification::Healthy
        }
        ProbeOutcome::HttpStatus { .. } | ProbeOutcome::TransportError => Classification::Transient,
        ProbeOutcome::OauthError { code, description } => classify_oauth(code, description),
    }
}

fn classify_oauth(code: &str, description: &str) -> Classification {
    match code.trim().to_ascii_lowercase().as_str() {
        "invalid_grant" => {
            if description.to_ascii_lowercase().contains("revoked") {
                Classification::Definitive {
                    state: AuthState::Revoked,
                    reason: "revoked",
                }
            } else {
                Classification::Definitive {
                    state: AuthState::NeedsReauth,
                    reason: "invalid_grant",
                }
            }
        }
        "no_token_file" => needs_reauth("no_token_file"),
        "bad_token_file" => needs_reauth("bad_token_file"),
        "invalid_auth" => needs_reauth("invalid_auth"),
        "not_authed" => needs_reauth("not_authed"),
        "token_expired" => needs_reauth("token_expired"),
        "token_revoked" => revoked("token_revoked"),
        "account_inactive" => revoked("account_inactive"),
        "access_denied" => needs_reauth("access_denied"),
        "invalidtoken" => needs_reauth("invalidtoken"),
        "unauthorized" => needs_reauth("unauthorized"),
        "no_token" => needs_reauth("no_token"),
        "accessexception" => misconfigured("accessexception"),
        "missing_config" => misconfigured("missing_config"),
        "bad_endpoint" => misconfigured("bad_endpoint"),
        "invalid_client" => misconfigured("invalid_client"),
        "deleted_client" => misconfigured("deleted_client"),
        "unauthorized_client" => misconfigured("unauthorized_client"),
        _ => Classification::Transient,
    }
}

fn needs_reauth(reason: &'static str) -> Classification {
    Classification::Definitive {
        state: AuthState::NeedsReauth,
        reason,
    }
}

fn revoked(reason: &'static str) -> Classification {
    Classification::Definitive {
        state: AuthState::Revoked,
        reason,
    }
}

fn misconfigured(reason: &'static str) -> Classification {
    Classification::Definitive {
        state: AuthState::Misconfigured,
        reason,
    }
}

pub fn step(prev: &Tracked, outcome: &ProbeOutcome, now: i64) -> Tracked {
    match classify(outcome) {
        Classification::Healthy => settle(prev, AuthState::Ok, "ok", now),
        Classification::Ttl { secs } if secs <= 0 => {
            settle(prev, AuthState::NeedsReauth, "token_expired", now)
        }
        Classification::Ttl { secs } if secs <= EXPIRING_WINDOW_SECS => settle(
            prev,
            AuthState::Expiring { eta: now + secs },
            "token_expiring",
            now,
        ),
        Classification::Ttl { .. } => settle(prev, AuthState::Ok, "ok", now),
        Classification::Definitive { state, reason } => settle(prev, state, reason, now),
        Classification::Transient => transient(prev, now),
    }
}

fn settle(prev: &Tracked, state: AuthState, reason: &str, now: i64) -> Tracked {
    let unchanged = prev.state.name() == state.name() && prev.reason == reason;
    Tracked {
        state,
        reason: reason.to_string(),
        since: if unchanged { prev.since } else { now },
        transient: None,
    }
}

fn transient(prev: &Tracked, now: i64) -> Tracked {
    let run = match prev.transient {
        Some(run) => TransientRun {
            count: run.count.saturating_add(1),
            first_at: run.first_at,
        },
        None => TransientRun {
            count: 1,
            first_at: now,
        },
    };
    let unreachable =
        run.count >= UNREACHABLE_MIN_FAILURES && now - run.first_at >= UNREACHABLE_WINDOW_SECS;
    let mut next = if unreachable {
        settle(prev, AuthState::Unreachable, "transient_failures", now)
    } else if let AuthState::Expiring { eta } = prev.state {
        if now >= eta {
            settle(prev, AuthState::NeedsReauth, "token_expired", now)
        } else {
            prev.clone()
        }
    } else {
        prev.clone()
    };
    next.transient = Some(run);
    next
}
