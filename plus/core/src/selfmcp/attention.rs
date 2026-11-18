//! `attention_list` (tier 1) is the same list as `toolportctl attention ls`; the write tools
//! quiet a row, bring it back and set what is checked, over `crate::attention::ops`.

use serde_json::{json, Value};
use toolport_plus_api::Host;

use super::ToolError;
use crate::args::{flag, str_nonempty};
use crate::attention::{self, ops, store::HideKind};

type Outcome = Result<Value, ToolError>;

pub(super) fn list(host: &dyn Host, args: &Value) -> Outcome {
    let snapshot = attention::snapshot(host);
    let mut data = snapshot.to_json();
    if flag(args, "hidden") {
        data["hidden"] = ops::hidden_json(&snapshot);
    }
    Ok(data)
}

fn required<'a>(args: &'a Value, key: &str) -> Result<&'a str, ToolError> {
    str_nonempty(args, key)
        .ok_or_else(|| ToolError::new("invalid_arguments", format!("{key} is required")))
}

fn hide(host: &dyn Host, args: &Value, kind: HideKind) -> Outcome {
    let until = match kind {
        HideKind::Snooze => Some(ops::snooze_end(
            str_nonempty(args, "for"),
            str_nonempty(args, "until"),
            host.now_unix(),
        )?),
        _ => None,
    };
    Ok(ops::hide(
        host,
        required(args, "id")?,
        kind,
        until,
        flag(args, "dry_run"),
    )?)
}

pub(super) fn snooze(host: &dyn Host, args: &Value) -> Outcome {
    hide(host, args, HideKind::Snooze)
}

pub(super) fn skip(host: &dyn Host, args: &Value) -> Outcome {
    hide(host, args, HideKind::Skip)
}

pub(super) fn dismiss(host: &dyn Host, args: &Value) -> Outcome {
    hide(host, args, HideKind::Dismiss)
}

pub(super) fn restore(host: &dyn Host, args: &Value) -> Outcome {
    Ok(ops::restore(host, required(args, "id")?)?)
}

pub(super) fn undo(host: &dyn Host, _args: &Value) -> Outcome {
    Ok(ops::undo(host)?)
}

pub(super) fn check(host: &dyn Host, _args: &Value) -> Outcome {
    Ok(ops::check(host)?.to_json())
}

pub(super) fn settings(host: &dyn Host, _args: &Value) -> Outcome {
    Ok(ops::settings(host)?)
}

pub(super) fn set(host: &dyn Host, args: &Value) -> Outcome {
    let on = args
        .get("on")
        .and_then(Value::as_bool)
        .ok_or_else(|| ToolError::new("invalid_arguments", "on is required (true or false)"))?;
    let key = required(args, "key")?;
    Ok(json!({"key": key, "on": on, "settings": ops::set_switch(host, key, on)?}))
}
