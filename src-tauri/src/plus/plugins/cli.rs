//! The per-plugin `claude plugin details|configure` calls. Both are read-only.

use super::claude::ClaudeRunner;
use super::installed::valid_ident;
use super::manifest::OptionRow;
use serde_json::Value;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(30);

fn failure(output: &crate::plus::exec::CmdOutput, what: &str) -> String {
    let e = output.first_error_line();
    if e.is_empty() {
        format!("{what} exited {}", output.code)
    } else {
        e
    }
}

/// Claude Code's own always-on figure from the `plugin details` text: the number on its
/// `Always-on:   ~31 tok` line, thousands separators allowed.
pub fn parse_projected(text: &str) -> Option<u64> {
    let rest = text.lines().find_map(|l| l.trim().strip_prefix("Always-on:"))?;
    let digits: String = rest
        .trim_start()
        .trim_start_matches('~')
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == ',')
        .filter(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

/// The projected always-on tokens of a plugin; `None` when `details` does not print the line.
pub fn details(runner: &dyn ClaudeRunner, id: &str) -> Result<Option<u64>, String> {
    if !valid_ident(id) {
        return Err(format!("invalid plugin id: {id}"));
    }
    let out = runner.run(&["plugin", "details", id], TIMEOUT)?;
    if !out.ok() {
        return Err(failure(&out, "claude plugin details"));
    }
    Ok(parse_projected(&out.stdout))
}

pub fn parse_configure(stdout: &str) -> Result<Vec<OptionRow>, String> {
    let doc: Value =
        serde_json::from_str(stdout).map_err(|e| format!("unparseable plugin options: {e}"))?;
    let Some(schema) = doc.get("schema").and_then(Value::as_object) else {
        return Err("unexpected plugin options shape".into());
    };
    let inputs = doc.get("inputs").and_then(Value::as_object);
    let choices = doc.get("choices").and_then(Value::as_object);
    let configured: Vec<&str> = doc
        .get("configured")
        .and_then(Value::as_array)
        .map(|l| l.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    Ok(schema
        .iter()
        .map(|(key, spec)| {
            let mut row = OptionRow::from_schema(key, spec);
            if let Some(list) = choices.and_then(|c| c.get(key)).and_then(Value::as_array) {
                row.choices = Some(list.iter().filter_map(Value::as_str).map(String::from).collect());
            }
            if !row.sensitive {
                row.current = inputs
                    .and_then(|i| i.get(key))
                    .and_then(Value::as_str)
                    .filter(|v| !v.is_empty())
                    .map(String::from);
            }
            row.configured = configured.contains(&key.as_str());
            row
        })
        .collect())
}

/// The option schema, choices and current values of a plugin (`plugin configure <id> --json`).
/// A plugin without options prints text, not JSON; that is an empty list.
pub fn configure(runner: &dyn ClaudeRunner, id: &str) -> Result<Vec<OptionRow>, String> {
    if !valid_ident(id) {
        return Err(format!("invalid plugin id: {id}"));
    }
    let out = runner.run(&["plugin", "configure", id, "--json"], TIMEOUT)?;
    if !out.ok() {
        return Err(failure(&out, "claude plugin configure"));
    }
    if out.stdout.contains("has no options") {
        return Ok(Vec::new());
    }
    parse_configure(&out.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projected_cost_is_read_from_the_always_on_line() {
        let text = "ecc 2.2.0\nProjected token cost\n  Always-on:   ~40,639 tok   added to every session\n";
        assert_eq!(parse_projected(text), Some(40639));
        assert_eq!(parse_projected("Always-on: ~31 tok"), Some(31));
        assert_eq!(parse_projected("nothing here"), None);
        assert_eq!(parse_projected("Always-on: none"), None);
    }

    #[test]
    fn configure_keeps_choices_and_hides_sensitive_values() {
        let out = r#"{"pluginId":"p@m","displayName":"p",
          "schema":{"mode":{"type":"string","title":"Mode","description":"d","default":"a"},
                    "api_token":{"type":"string","title":"Token","sensitive":true},
                    "on":{"type":"boolean","title":"On","default":true}},
          "inputs":{"mode":"b","api_token":"","on":"true"},
          "choices":{"mode":["a","b"],"on":["true","false"]},
          "configured":["mode","api_token"],"unconfigured":["on"]}"#;
        let rows = parse_configure(out).unwrap();
        let by = |k: &str| rows.iter().find(|r| r.key == k).unwrap();
        assert_eq!(by("mode").current.as_deref(), Some("b"));
        assert_eq!(by("mode").choices, Some(vec!["a".into(), "b".into()]));
        assert!(by("api_token").sensitive && by("api_token").current.is_none());
        assert!(by("api_token").configured);
        assert!(!by("on").configured);
    }

    #[test]
    fn configure_rejects_other_shapes() {
        assert!(parse_configure("not json").is_err());
        assert!(parse_configure("{}").is_err());
    }
}
