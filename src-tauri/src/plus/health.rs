use serde::Serialize;

/// The verdict of one doctor check, `ok`, `warn` or `fail` on the wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Health {
    Ok,
    Warn,
    Fail,
}

impl Health {
    pub fn as_str(self) -> &'static str {
        match self {
            Health::Ok => "ok",
            Health::Warn => "warn",
            Health::Fail => "fail",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verdicts_keep_their_wire_strings() {
        for (health, wire) in [
            (Health::Ok, "ok"),
            (Health::Warn, "warn"),
            (Health::Fail, "fail"),
        ] {
            assert_eq!(health.as_str(), wire);
            assert_eq!(serde_json::to_value(health).unwrap(), wire);
        }
    }
}
