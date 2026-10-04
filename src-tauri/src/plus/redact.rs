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
const EXEMPT_KEYS: &[&str] = &["secretsbackend", "tokens"];
const MASK: &str = "[redacted]";

fn sensitive_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    !EXEMPT_KEYS.contains(&lower.as_str()) && SENSITIVE_KEYS.iter().any(|s| lower.contains(s))
}

const LIVE_SECRET_ENV: [&str; 3] = [
    crate::brand::SECRET_KEY,
    crate::brand::SECRET_KEY_LEGACY,
    "TOOLPORT_HTTP_TOKEN",
];

fn live_secrets() -> Vec<String> {
    LIVE_SECRET_ENV
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_count_object_is_not_a_secret_but_a_token_inside_it_still_is() {
        let scrubbed = scrub(serde_json::json!({
            "tokens": {"value": 12, "basis": "estimate", "accessToken": "abc"},
            "token": "abc",
        }));
        assert_eq!(scrubbed["tokens"]["value"], 12);
        assert_eq!(scrubbed["tokens"]["accessToken"], MASK);
        assert_eq!(scrubbed["token"], MASK);
    }

    #[test]
    fn both_secret_key_names_and_the_http_token_are_scrubbed() {
        assert_eq!(
            LIVE_SECRET_ENV,
            [
                "TOOLPORT_SECRET_KEY",
                "CONDUIT_SECRET_KEY",
                "TOOLPORT_HTTP_TOKEN"
            ]
        );
    }
}
