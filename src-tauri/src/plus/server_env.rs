//! Plain environment entries of a stdio server, written by `toolportctl server new|edit` and the
//! `servers_install` / `servers_update_config` tools. An entry written here is always
//! `secret: false` with its value inline in registry.json; credentials keep going through
//! `toolportctl secret set`. No message built here repeats a value, because refusals reach the
//! audit log and the agent's conversation.

use crate::registry::{self, EnvVar};

pub(crate) const RULE_INVALID_KEY: &str = "invalid-key";
pub(crate) const RULE_INVALID_VALUE: &str = "invalid-value";
pub(crate) const RULE_CREDENTIAL_NAME: &str = "credential-name";
pub(crate) const RULE_CREDENTIAL_VALUE: &str = "credential-value";
pub(crate) const RULE_LAUNCH_SCREEN: &str = "launch-screen";
pub(crate) const RULE_SECRET_ENTRY: &str = "secret-entry";
pub(crate) const RULE_STDIO_ONLY: &str = "stdio-only";

const MAX_KEY_LEN: usize = 128;
const MAX_VALUE_LEN: usize = 4096;
const BLOB_MIN_LEN: usize = 32;
const BLOB_MIN_ENTROPY: f64 = 3.5;
const NAME_AS_CREDENTIAL_MIN_LEN: usize = 20;

const CREDENTIAL_NAME_PARTS: [&str; 11] = [
    "KEY",
    "TOKEN",
    "SECRET",
    "PASSWORD",
    "PASSWD",
    "PASSPHRASE",
    "CREDENTIAL",
    "AUTH",
    "COOKIE",
    "BEARER",
    "PRIVATE",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Change {
    Set { key: String, value: String },
    Unset { key: String },
}

impl Change {
    pub(crate) fn key(&self) -> &str {
        match self {
            Change::Set { key, .. } | Change::Unset { key } => key,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Refusal {
    pub rule: &'static str,
    pub message: String,
}

impl Refusal {
    fn new(rule: &'static str, message: String) -> Self {
        Self { rule, message }
    }

    fn vault(rule: &'static str, key: &str, why: &str) -> Self {
        Self::new(
            rule,
            format!(
                "env {key} refused by rule {rule}: {why}. Credentials go in the vault: \
                 toolportctl secret set <server> {key} (value on stdin or --value-env <VAR>)"
            ),
        )
    }
}

fn shape_ok(key: &str) -> bool {
    let mut chars = key.chars();
    key.len() <= MAX_KEY_LEN
        && !key.starts_with("__")
        && chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn credential_name_part(key: &str) -> Option<&'static str> {
    let upper = key.to_ascii_uppercase();
    CREDENTIAL_NAME_PARTS
        .iter()
        .copied()
        .find(|part| upper.contains(part))
}

fn has_url_userinfo(value: &str) -> bool {
    value.split_whitespace().any(|word| {
        word.split_once("://").is_some_and(|(_, rest)| {
            rest.split(['/', '?', '#'])
                .next()
                .is_some_and(|authority| authority.contains('@'))
        })
    })
}

fn has_auth_scheme(value: &str) -> bool {
    let lower = value.trim().to_ascii_lowercase();
    ["bearer", "basic", "digest"].iter().any(|scheme| {
        lower
            .strip_prefix(scheme)
            .is_some_and(|rest| rest.starts_with(char::is_whitespace) && !rest.trim().is_empty())
    })
}

fn is_base64url(part: &str) -> bool {
    part.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '='))
}

fn is_jwt(word: &str) -> bool {
    let parts: Vec<&str> = word.split('.').collect();
    matches!(parts.len(), 3 | 5)
        && parts[0].starts_with("eyJ")
        && parts[..2].iter().all(|part| part.len() >= 4)
        && parts.iter().all(|part| is_base64url(part))
}

fn entropy(word: &str) -> f64 {
    let mut counts = std::collections::HashMap::new();
    for byte in word.bytes() {
        *counts.entry(byte).or_insert(0usize) += 1;
    }
    let total = word.len() as f64;
    counts
        .values()
        .map(|&count| {
            let p = count as f64 / total;
            -p * p.log2()
        })
        .sum()
}

/// Hex digests and mixed-case base64 are what keys look like; an all-lowercase slug such as a
/// model name is not, even when it is long and has digits.
fn is_opaque_blob(word: &str) -> bool {
    let has = |test: fn(&char) -> bool| word.chars().any(|c| test(&c));
    let hex = word.chars().all(|c| c.is_ascii_hexdigit());
    word.len() >= BLOB_MIN_LEN
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '=' | '_' | '-'))
        && has(char::is_ascii_digit)
        && has(char::is_ascii_alphabetic)
        && (hex || (has(char::is_ascii_uppercase) && has(char::is_ascii_lowercase)))
        && entropy(word) >= BLOB_MIN_ENTROPY
}

fn credential_reason(text: &str) -> Option<&'static str> {
    if has_url_userinfo(text) {
        return Some("it contains a URL with user info (user:pass@host)");
    }
    if has_auth_scheme(text) {
        return Some("it starts with an authorization scheme (Bearer, Basic or Digest)");
    }
    if text.split_whitespace().any(is_jwt) {
        return Some("it contains a JWT");
    }
    if registry::arg_looks_secret(text) || registry::redact_secret_text(text) != text {
        return Some(
            "it matches a credential format (a token prefix such as sk-, ghp_, xox or AKIA, \
             or an inline password=, token= or api-key= assignment)",
        );
    }
    if text.split_whitespace().any(is_opaque_blob) {
        return Some("it contains a long high-entropy string");
    }
    None
}

/// The rules that need nothing but the key and the value.
pub(crate) fn check_assignment(key: &str, value: &str) -> Result<(), Refusal> {
    if !shape_ok(key) {
        return Err(Refusal::new(
            RULE_INVALID_KEY,
            format!(
                "env name refused by rule {RULE_INVALID_KEY}: a name is letters, digits and \
                 underscores, starts with a letter or underscore, has at most {MAX_KEY_LEN} \
                 characters and does not begin with __"
            ),
        ));
    }
    if let Some(part) = credential_name_part(key) {
        return Err(Refusal::vault(
            RULE_CREDENTIAL_NAME,
            key,
            &format!("the name contains {part}, so the entry is treated as a credential"),
        ));
    }
    if key.len() >= NAME_AS_CREDENTIAL_MIN_LEN && credential_reason(key).is_some() {
        return Err(Refusal::new(
            RULE_CREDENTIAL_NAME,
            format!(
                "env name refused by rule {RULE_CREDENTIAL_NAME}: the name itself looks like \
                 a credential; check that the name and the value are not swapped"
            ),
        ));
    }
    if value.len() > MAX_VALUE_LEN || value.chars().any(char::is_control) {
        return Err(Refusal::new(
            RULE_INVALID_VALUE,
            format!(
                "env {key} refused by rule {RULE_INVALID_VALUE}: a value has at most \
                 {MAX_VALUE_LEN} bytes and no control characters"
            ),
        ));
    }
    if let Some(why) = credential_reason(value) {
        return Err(Refusal::vault(RULE_CREDENTIAL_VALUE, key, why));
    }
    if crate::downstream::screen_spawn_env(&[(key.to_string(), value.to_string())]).is_err() {
        return Err(Refusal::new(
            RULE_LAUNCH_SCREEN,
            format!(
                "env {key} refused by rule {RULE_LAUNCH_SCREEN}: the gateway refuses to \
                 launch a process whose environment preloads or injects code"
            ),
        ));
    }
    Ok(())
}

fn secret_entry(key: &str, action: &str) -> Refusal {
    Refusal::new(
        RULE_SECRET_ENTRY,
        format!(
            "env {key} refused by rule {RULE_SECRET_ENTRY}: {key} is a secret entry whose value \
             stays in the vault, so it is not {action} here. Change it with toolportctl secret \
             set <server> {key}; remove it with toolportctl secret rm <server> {key}"
        ),
    )
}

/// Applies `changes` to a server's env and returns the keys whose entry changed. Nothing is
/// modified when a change is refused.
pub(crate) fn apply(
    env: &mut Vec<EnvVar>,
    transport: &str,
    changes: &[Change],
) -> Result<Vec<String>, Refusal> {
    let mut next = env.clone();
    let mut changed = Vec::new();
    for change in changes {
        let key = change.key();
        let secret = next
            .iter()
            .any(|entry| entry.secret && entry.key.eq_ignore_ascii_case(key));
        match change {
            Change::Set { value, .. } => {
                check_assignment(key, value)?;
                if transport != "stdio" {
                    return Err(Refusal::new(
                        RULE_STDIO_ONLY,
                        format!(
                            "env {key} refused by rule {RULE_STDIO_ONLY}: environment entries \
                             apply to stdio servers and this one uses the {transport} transport"
                        ),
                    ));
                }
                if secret {
                    return Err(secret_entry(key, "overwritten or downgraded to plain"));
                }
                match next.iter_mut().find(|entry| entry.key == key) {
                    Some(entry) if entry.value.as_deref() == Some(value) => {}
                    Some(entry) => {
                        entry.value = Some(value.clone());
                        changed.push(key.to_string());
                    }
                    None => {
                        next.push(EnvVar {
                            key: key.to_string(),
                            value: Some(value.clone()),
                            secret: false,
                        });
                        changed.push(key.to_string());
                    }
                }
            }
            Change::Unset { .. } => {
                if secret {
                    return Err(secret_entry(key, "removed"));
                }
                let before = next.len();
                next.retain(|entry| entry.key != key);
                if next.len() != before {
                    changed.push(key.to_string());
                }
            }
        }
    }
    *env = next;
    Ok(changed)
}

/// One `KEY=VALUE` assignment as given on the command line.
pub(crate) fn parse_assignment(raw: &str) -> Result<Change, &'static str> {
    match raw.split_once('=') {
        Some((key, value)) if !key.is_empty() => Ok(Change::Set {
            key: key.to_string(),
            value: value.to_string(),
        }),
        _ => Err("--env takes KEY=VALUE"),
    }
}

/// Rejects a key named twice, so one command never carries two meanings for a key.
pub(crate) fn check_distinct(changes: &[Change]) -> Result<(), String> {
    for (index, change) in changes.iter().enumerate() {
        if changes[..index].iter().any(|seen| seen.key() == change.key()) {
            return Err(format!(
                "env {} is named more than once; give each name one --env or --unset-env",
                change.key()
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake(prefix: &str, body: usize) -> String {
        format!("{prefix}{}", "aB3dE5fG7hJ9kL2m".repeat(body))
    }

    fn plain(key: &str, value: Option<&str>) -> EnvVar {
        EnvVar {
            key: key.into(),
            value: value.map(String::from),
            secret: false,
        }
    }

    fn vaulted(key: &str) -> EnvVar {
        EnvVar {
            key: key.into(),
            value: None,
            secret: true,
        }
    }

    fn set(key: &str, value: &str) -> Change {
        Change::Set {
            key: key.into(),
            value: value.into(),
        }
    }

    fn unset(key: &str) -> Change {
        Change::Unset { key: key.into() }
    }

    fn rule_of(key: &str, value: &str) -> &'static str {
        check_assignment(key, value).unwrap_err().rule
    }

    #[test]
    fn plain_paths_urls_and_settings_are_accepted() {
        for (key, value) in [
            ("JAVA_HOME", "/opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home"),
            ("REA_JADX_MCP_JAR", "/Users/someone/.local/share/rea/jadx-mcp-server-all.jar"),
            ("ODOO_URL", "https://odoo.example.invalid/web?db=test"),
            ("ODOO_DB", "test"),
            ("LOG_LEVEL", "info"),
            ("PORT", "8080"),
            ("NODE_OPTIONS", "--max-old-space-size=4096"),
            ("WINDOWS_HOME", r"C:\Program Files\Java\jdk-21"),
            ("HTTPS_PROXY", "http://proxy.example.invalid:3128"),
            ("PYTHONPATH", "/srv/app:/srv/lib"),
            ("PATH", "/usr/local/bin:/usr/bin:/bin"),
            ("TZ", "Europe/Amsterdam"),
            ("JAVA_OPTS", "-Xms256m -Xmx1g -Dfile.encoding=UTF-8"),
            ("EMPTY", ""),
            ("_UNDERSCORE_FIRST", "x"),
            ("lower_case", "x"),
            ("SK_LEARN_HOME", "/opt/sk"),
        ] {
            assert_eq!(check_assignment(key, value), Ok(()), "{key}={value}");
        }
    }

    #[test]
    fn a_credential_looking_name_is_refused_whatever_the_value() {
        for key in [
            "API_KEY",
            "api_key",
            "GITHUB_TOKEN",
            "CLIENT_SECRET",
            "DB_PASSWORD",
            "DB_PASSWD",
            "SSH_PASSPHRASE",
            "AWS_CREDENTIALS",
            "SSH_AUTH_SOCK",
            "SESSION_COOKIE",
            "BEARER_VALUE",
            "PRIVATE_URL",
            "apiKey",
        ] {
            let refusal = check_assignment(key, "plain").unwrap_err();
            assert_eq!(refusal.rule, RULE_CREDENTIAL_NAME, "{key}");
            assert!(refusal.message.contains("toolportctl secret set"), "{key}");
            assert!(refusal.message.contains(key), "{key}");
        }
    }

    #[test]
    fn a_credential_looking_value_is_refused_under_a_harmless_name() {
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJmYWtlIn0.ZmFrZXNpZ25hdHVyZQ";
        let blob = "Zk9xQ2xWbTRyU3RkS0pMcDdoR3lCMk5hWDhFYzV1";
        for value in [
            fake("sk-", 2),
            fake("ghp_", 3),
            fake("gho_", 3),
            fake("github_pat_", 3),
            fake("xoxb-", 2),
            "AKIAFAKEFAKEFAKEFAKE".to_string(),
            "Bearer fake-bearer-value".to_string(),
            "basic ZmFrZTpmYWtl".to_string(),
            jwt.to_string(),
            format!("see {jwt}"),
            "https://fake-user:fake-pass@host.example.invalid/x".to_string(),
            "https://fake-user@host.example.invalid/x".to_string(),
            "postgres://fake:fake@db.example.invalid/app".to_string(),
            "https://host.example.invalid/x?token=fake-value".to_string(),
            "password=fake-value".to_string(),
            "--api-key=fake-value".to_string(),
            blob.to_string(),
            "0123456789abcdef0123456789abcdef0123abcd".to_string(),
        ] {
            let refusal = check_assignment("SOME_SETTING", &value).unwrap_err();
            assert_eq!(refusal.rule, RULE_CREDENTIAL_VALUE, "{value}");
            assert!(refusal.message.contains("toolportctl secret set"), "{value}");
            assert!(!refusal.message.contains(&value), "{value}");
        }
    }

    #[test]
    fn a_name_that_is_itself_a_token_is_refused_without_repeating_it() {
        let token = fake("ghp_", 3);
        let refusal = check_assignment(&token, "x").unwrap_err();
        assert_eq!(refusal.rule, RULE_CREDENTIAL_NAME);
        assert!(!refusal.message.contains(&token));
    }

    #[test]
    fn long_names_are_not_blobs_and_short_ones_are_not_credentials() {
        for value in [
            "jadx-mcp-server-release-candidate-build",
            "text-embedding-3-large-dimension-1536-v2",
            "/opt/some/long/path/with-digits-2024/and-letters/abcdefghijklmnop",
            "com.example.reverse.engineering.tools.Main",
            "sk-x",
            "Bearer",
            "basic",
        ] {
            assert_eq!(check_assignment("SOME_SETTING", value), Ok(()), "{value}");
        }
    }

    #[test]
    fn shape_and_size_rules_name_themselves() {
        for key in ["", "1ABC", "A-B", "A B", "A=B", "__RESERVED", "É"] {
            assert_eq!(rule_of(key, "x"), RULE_INVALID_KEY, "{key:?}");
        }
        assert_eq!(rule_of(&"A".repeat(MAX_KEY_LEN + 1), "x"), RULE_INVALID_KEY);
        assert_eq!(check_assignment(&"A".repeat(MAX_KEY_LEN), "x"), Ok(()));
        assert_eq!(rule_of("SETTING", "a\nb"), RULE_INVALID_VALUE);
        assert_eq!(rule_of("SETTING", "a\0b"), RULE_INVALID_VALUE);
        assert_eq!(rule_of("SETTING", &"v".repeat(MAX_VALUE_LEN + 1)), RULE_INVALID_VALUE);
        assert_eq!(check_assignment("SETTING", &"v ".repeat(MAX_VALUE_LEN / 2)), Ok(()));
    }

    #[test]
    fn code_injection_variables_are_refused_like_the_gateway_would() {
        assert_eq!(rule_of("LD_PRELOAD", "/tmp/x.so"), RULE_LAUNCH_SCREEN);
        assert_eq!(rule_of("NODE_OPTIONS", "--require ./x.js"), RULE_LAUNCH_SCREEN);
        assert_eq!(rule_of("JAVA_TOOL_OPTIONS", "-javaagent:/tmp/x.jar"), RULE_LAUNCH_SCREEN);
        assert_eq!(check_assignment("JAVA_TOOL_OPTIONS", "-Xmx512m"), Ok(()));
    }

    #[test]
    fn set_adds_replaces_and_reports_only_real_changes() {
        let mut env = vec![plain("MODE", Some("fast")), plain("DECLARED", None)];
        let changed = apply(
            &mut env,
            "stdio",
            &[
                set("JAVA_HOME", "/opt/jdk"),
                set("MODE", "slow"),
                set("DECLARED", "now"),
            ],
        )
        .unwrap();
        assert_eq!(changed, ["JAVA_HOME", "MODE", "DECLARED"]);
        assert_eq!(
            env,
            [
                plain("MODE", Some("slow")),
                plain("DECLARED", Some("now")),
                plain("JAVA_HOME", Some("/opt/jdk")),
            ]
        );
        assert_eq!(apply(&mut env, "stdio", &[set("MODE", "slow")]), Ok(vec![]));
    }

    #[test]
    fn unset_removes_a_plain_entry_and_ignores_an_absent_one() {
        let mut env = vec![plain("MODE", Some("fast")), plain("OTHER", Some("1"))];
        assert_eq!(
            apply(&mut env, "stdio", &[unset("MODE"), unset("MISSING")]),
            Ok(vec!["MODE".to_string()])
        );
        assert_eq!(env, [plain("OTHER", Some("1"))]);
    }

    #[test]
    fn an_existing_secret_is_never_overwritten_downgraded_or_removed() {
        for key in ["PROJECT_ALPHA", "project_alpha"] {
            let mut env = vec![vaulted("PROJECT_ALPHA"), plain("MODE", Some("fast"))];
            let before = env.clone();
            for change in [set(key, "plain"), unset(key)] {
                let refusal = apply(&mut env, "stdio", &[set("MODE", "slow"), change]).unwrap_err();
                assert_eq!(refusal.rule, RULE_SECRET_ENTRY, "{key}");
                assert!(refusal.message.contains("secret rm"), "{key}");
                assert_eq!(env, before, "a refused batch changes nothing");
            }
        }
    }

    #[test]
    fn a_remote_server_takes_no_new_env_but_loses_a_stale_entry() {
        let mut env = vec![plain("MODE", Some("fast"))];
        assert_eq!(
            apply(&mut env, "http", &[set("OTHER", "1")]).unwrap_err().rule,
            RULE_STDIO_ONLY
        );
        assert_eq!(apply(&mut env, "sse", &[unset("MODE")]), Ok(vec!["MODE".to_string()]));
        assert!(env.is_empty());
    }

    #[test]
    fn assignments_split_at_the_first_equals_sign() {
        assert_eq!(parse_assignment("A=b=c"), Ok(set("A", "b=c")));
        assert_eq!(parse_assignment("A="), Ok(set("A", "")));
        for bad in ["A", "=b", ""] {
            assert_eq!(parse_assignment(bad), Err("--env takes KEY=VALUE"), "{bad:?}");
        }
    }

    #[test]
    fn a_key_named_twice_in_one_command_is_ambiguous() {
        assert_eq!(check_distinct(&[set("A", "1"), unset("B")]), Ok(()));
        assert!(check_distinct(&[set("A", "1"), unset("A")]).unwrap_err().contains("env A"));
        assert!(check_distinct(&[set("A", "1"), set("A", "2")]).is_err());
    }
}
