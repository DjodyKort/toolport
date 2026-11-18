use crate::auth::{AuthKind, AuthRow};

use super::model::{Action, Item, Screen, Urgency};

const SOURCE: &str = "Credentials";
const MAX_DETAIL: usize = 200;

pub struct ServerFacts {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    /// Secret environment keys the server declares and the vault holds no value for.
    pub missing_secrets: Vec<String>,
}

pub struct CredentialInputs {
    pub servers: Vec<ServerFacts>,
    /// Set when the vault could not be read; secrets are then neither present nor missing.
    pub vault_error: Option<String>,
    pub logins: Vec<AuthRow>,
}

pub fn one_line(text: &str) -> String {
    let line = text.lines().next().unwrap_or("").trim();
    match line.char_indices().nth(MAX_DETAIL) {
        Some((end, _)) => format!("{}...", &line[..end]),
        None => line.to_string(),
    }
}

fn list(keys: &[String]) -> String {
    match keys {
        [one] => format!("{one} is not set"),
        _ => format!("{} are not set", keys.join(", ")),
    }
}

fn remaining(secs: i64) -> String {
    let hours = secs.max(0) / 3600;
    match hours {
        0 => "less than an hour".to_string(),
        1..=47 => format!("{hours} h"),
        _ => format!("{} days", hours / 24),
    }
}

pub fn items(inputs: &CredentialInputs) -> Vec<Item> {
    let mut out = Vec::new();
    if let Some(error) = &inputs.vault_error {
        out.push(Item {
            id: "credentials:vault".into(),
            urgency: Urgency::NeedsYou,
            title: "The secrets vault cannot be read".into(),
            detail: one_line(error),
            source: SOURCE,
            skip: None,
            since: None,
            action: Some(Action::open("Open Servers", Screen::Servers)),
        });
    }
    for server in inputs.servers.iter().filter(|s| s.enabled) {
        if inputs.vault_error.is_some() || server.missing_secrets.is_empty() {
            continue;
        }
        out.push(Item {
            id: format!("credentials:secret:{}", server.id),
            urgency: Urgency::NeedsYou,
            title: format!("{} is missing a secret", server.name),
            detail: format!(
                "{}, so the server cannot start.",
                list(&server.missing_secrets)
            ),
            source: SOURCE,
            skip: None,
            since: None,
            action: Some(Action::run(
                "Set secret",
                Screen::Servers,
                &[
                    "toolportctl",
                    "secret",
                    "set",
                    &server.id,
                    &server.missing_secrets[0],
                ],
            )),
        });
    }
    for row in &inputs.logins {
        let Some(server) = inputs.servers.iter().find(|s| s.id == row.server) else {
            continue;
        };
        if !server.enabled {
            continue;
        }
        let name = &server.name;
        let sign_in = Action::run(
            "Sign in",
            Screen::Servers,
            &["toolportctl", "auth", "login", &row.server],
        );
        let (urgency, title, detail, action) = match row.state {
            AuthKind::NeedsReauth => (
                Urgency::NeedsYou,
                format!("{name} needs a new sign-in"),
                one_line(&row.reason),
                sign_in,
            ),
            AuthKind::Revoked => (
                Urgency::NeedsYou,
                format!("{name} lost its access"),
                one_line(&row.reason),
                sign_in,
            ),
            AuthKind::Misconfigured => (
                Urgency::NeedsYou,
                format!("The login check of {name} is set up wrongly"),
                one_line(&row.reason),
                Action::open("Open Servers", Screen::Servers),
            ),
            AuthKind::Expiring => (
                Urgency::WorthALook,
                format!(
                    "{name} sign-in expires in {}",
                    remaining(row.ttl_secs.unwrap_or(0))
                ),
                "Signing in now avoids a failed call.".to_string(),
                sign_in,
            ),
            AuthKind::Unreachable => (
                Urgency::WorthALook,
                format!("{name} could not be checked"),
                one_line(&row.reason),
                Action::run(
                    "Check again",
                    Screen::Servers,
                    &[
                        "toolportctl",
                        "auth",
                        "probe",
                        "--server",
                        &row.server,
                        "--force",
                    ],
                ),
            ),
            AuthKind::Unknown | AuthKind::Ok => continue,
        };
        out.push(Item {
            id: format!("credentials:login:{}", row.server),
            urgency,
            title,
            detail,
            source: SOURCE,
            skip: None,
            since: Some(row.since),
            action: Some(action),
        });
    }
    out
}
