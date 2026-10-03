use serde_json::Value;

const SENSITIVE_KEYS: &[&str] = &[
    "secret",
    "token",
    "password",
    "passwd",
    "apikey",
    "api_key",
    "authorization",
    "credential",
    "bearer",
    "private_key",
    "passphrase",
];
const EXEMPT_KEYS: &[&str] = &["secretsbackend"];
const MASK: &str = "[redacted]";

fn sensitive_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    !EXEMPT_KEYS.contains(&lower.as_str()) && SENSITIVE_KEYS.iter().any(|s| lower.contains(s))
}

fn live_secrets() -> Vec<String> {
    [
        "TOOLPORT_SECRET_KEY",
        "CONDUIT_SECRET_KEY",
        "TOOLPORT_HTTP_TOKEN",
    ]
    .iter()
    .filter_map(|name| std::env::var(name).ok())
    .filter(|v| v.len() >= 8)
    .collect()
}

pub fn scrub_text(text: String) -> String {
    live_secrets()
        .iter()
        .fold(text, |acc, secret| acc.replace(secret.as_str(), MASK))
}

pub fn scrub(value: Value) -> Value {
    scrub_with(value, &live_secrets())
}

fn scrub_with(value: Value, secrets: &[String]) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(k, v)| {
                    let masked = sensitive_key(&k)
                        && matches!(v, Value::String(_) | Value::Array(_) | Value::Object(_));
                    let v = if masked {
                        Value::String(MASK.into())
                    } else {
                        scrub_with(v, secrets)
                    };
                    (k, v)
                })
                .collect(),
        ),
        Value::Array(items) => {
            Value::Array(items.into_iter().map(|v| scrub_with(v, secrets)).collect())
        }
        Value::String(s) => Value::String(
            secrets
                .iter()
                .fold(s, |acc, secret| acc.replace(secret.as_str(), MASK)),
        ),
        other => other,
    }
}
