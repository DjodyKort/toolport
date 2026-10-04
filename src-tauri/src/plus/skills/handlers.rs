//! `plus.skills.{sync,list,lint,diff,status}`: adapters over the typed operations in `api`.
//! Arguments use the keys of the self-MCP tools. Like a tool call, all but `diff` reject an
//! unknown or mistyped argument and scrub their result.

use super::api::{self, Args};
use crate::plus::args::{check, is_string_list};
use crate::plus::op::OpError;
use crate::plus::redact;
use serde_json::{json, Value};

type Keys = &'static [(&'static str, fn(&Value) -> bool)];
type Op = fn(&Args) -> Result<Value, OpError>;

const REPO: (&str, fn(&Value) -> bool) = ("repo_path", Value::is_string);
const CLIENTS: (&str, fn(&Value) -> bool) = ("client_keys", is_string_list);
const LIST: Keys = &[REPO];
const LINT: Keys = &[REPO, ("names", is_string_list)];
const STATUS: Keys = &[REPO, CLIENTS];
const SYNC: Keys = &[
    REPO,
    CLIENTS,
    ("dry_run", Value::is_boolean),
    ("global_mode", Value::is_boolean),
    ("migrate", Value::is_boolean),
];

fn served(args: Value, keys: Keys, op: Op) -> Result<Value, String> {
    let args = if args.is_null() { json!({}) } else { args };
    check(&args, keys).map_err(|e| e.message)?;
    op(&Args::from_json(&args))
        .map(redact::scrub)
        .map_err(|e| e.message)
}

pub fn sync_handler(args: Value) -> Result<Value, String> {
    served(args, SYNC, api::sync)
}

pub fn list_handler(args: Value) -> Result<Value, String> {
    served(args, LIST, api::list_skills)
}

pub fn lint_handler(args: Value) -> Result<Value, String> {
    served(args, LINT, api::lint)
}

pub fn diff_handler(args: Value) -> Result<Value, String> {
    api::diff(&Args::from_json(&args)).map_err(|e| e.message)
}

pub fn status_handler(args: Value) -> Result<Value, String> {
    served(args, STATUS, api::status)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plus::testutil::DataDirFx;

    const HANDLERS: [(&str, fn(Value) -> Result<Value, String>); 4] = [
        ("list", list_handler),
        ("lint", lint_handler),
        ("sync", sync_handler),
        ("status", status_handler),
    ];

    #[test]
    fn every_handler_but_diff_rejects_unknown_and_mistyped_arguments() {
        for (name, handler) in HANDLERS {
            assert_eq!(
                handler(json!({"confirm": true})).unwrap_err(),
                "unknown argument: confirm",
                "{name}"
            );
            assert_eq!(
                handler(json!({"repo_path": 3})).unwrap_err(),
                "argument repo_path has the wrong type",
                "{name}"
            );
            assert_eq!(
                handler(json!([])).unwrap_err(),
                "arguments must be an object",
                "{name}"
            );
        }
        assert_eq!(
            sync_handler(json!({"dry_run": "yes"})).unwrap_err(),
            "argument dry_run has the wrong type"
        );
        assert_eq!(
            status_handler(json!({"client_keys": ["a", 1]})).unwrap_err(),
            "argument client_keys has the wrong type"
        );
    }

    #[test]
    fn diff_ignores_what_the_others_reject() {
        let fx = DataDirFx::new("skills-handlers", "diff");
        let nowhere = fx.dir.join("nowhere");
        let error = diff_handler(json!({"repo_path": nowhere, "confirm": true})).unwrap_err();
        assert_eq!(error, "no skills repository found");
        for (name, handler) in HANDLERS {
            let error = handler(json!({"repo_path": nowhere})).unwrap_err();
            assert_eq!(error, "no skills repository found", "{name}");
        }
    }

    #[test]
    fn served_results_are_scrubbed_of_live_secrets_and_secret_keys() {
        let _fx = DataDirFx::new("skills-handlers", "scrub").with_secret_key("synthetic-key-0123");
        let op: Op = |_| Ok(json!({"repo": "x synthetic-key-0123", "token": "abc", "n": 1}));
        let scrubbed = served(Value::Null, LIST, op).unwrap();
        assert_eq!(
            scrubbed,
            json!({"repo": "x [redacted]", "token": "[redacted]", "n": 1})
        );
    }
}
