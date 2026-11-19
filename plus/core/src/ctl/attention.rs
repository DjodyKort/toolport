//! `toolportctl attention`: everything that needs the person right now, from `crate::attention`,
//! which the self-MCP tools and the app's footer bell read as well, and the commands that quiet a
//! row (snooze, skip, dismiss), bring it back (restore, undo) and set what is checked.

use serde_json::json;
use toolport_plus_api::Host;

use super::flags::{switch, value, Inline, Operands, Spec, Unknown};
use super::output::{CtlError, Output};
use crate::attention::{self, ops, store::HideKind, Urgency};

const fn plain(flags: &'static [super::flags::Flag], operands: Operands) -> Spec {
    Spec {
        flags,
        inline: Inline::Strict,
        unknown: Unknown::Argument,
        operands,
        ..Spec::PLAIN
    }
}

pub(super) const LS: Spec = plain(&[switch("--hidden"), value("--level")], Operands::Reject);
pub(super) const SNOOZE: Spec = plain(
    &[value("--for"), value("--until"), switch("--dry-run")],
    Operands::Collect,
);
pub(super) const HIDE: Spec = plain(&[switch("--dry-run")], Operands::Collect);
pub(super) const ONE: Spec = plain(&[], Operands::Collect);
pub(super) const NOTIFY: Spec = plain(
    &[switch("--test"), switch("--open-settings")],
    Operands::Reject,
);
pub(super) const NONE: Spec = plain(&[], Operands::Reject);

fn id_of(flags: &super::flags::Flags, usage: &str) -> Result<String, CtlError> {
    flags.single(usage).map(str::to_string)
}

pub fn ls(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = LS.parse(rest)?;
    let level = match flags.one("--level") {
        None => None,
        Some("needs-you") => Some(Some(Urgency::NeedsYou)),
        Some("look") => Some(Some(Urgency::WorthALook)),
        Some("fyi") => Some(None),
        Some(other) => {
            return Err(CtlError::usage(format!(
                "--level is needs-you, look or fyi, not '{other}'"
            )))
        }
    };
    let mut snapshot = attention::snapshot(host);
    if let Some(level) = level {
        snapshot.items.retain(|item| Some(item.urgency) == level);
    }
    let mut data = snapshot.to_json();
    let mut text = attention::human(&snapshot);
    if flags.on("--hidden") {
        data["hidden"] = ops::hidden_json(&snapshot);
        text = attention::human_hidden(&snapshot);
    }
    Ok(Output::new(data, text))
}

fn hide(
    host: &dyn Host,
    spec: &Spec,
    rest: &[String],
    usage: &str,
    kind: HideKind,
) -> Result<Output, CtlError> {
    let flags = spec.parse(rest)?;
    let id = id_of(&flags, usage)?;
    let until = match kind {
        HideKind::Snooze => Some(ops::snooze_end(
            flags.one("--for"),
            flags.one("--until"),
            host.now_unix(),
        )?),
        _ => None,
    };
    let data = ops::hide(host, &id, kind, until, flags.on("--dry-run"))?;
    let dry = flags.on("--dry-run");
    let text = format!(
        "{}{} {}{}. Undo: toolportctl attention undo",
        if dry { "Would have " } else { "" },
        kind.word(),
        data["title"].as_str().unwrap_or(&id),
        data["hidden"]["until"]
            .as_str()
            .map(|until| format!(" until {until}"))
            .unwrap_or_default()
    );
    Ok(Output::new(data, text))
}

pub fn snooze(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    hide(
        host,
        &SNOOZE,
        rest,
        "usage: toolportctl attention snooze <id> [--for 1h|--until <time>] [--dry-run]",
        HideKind::Snooze,
    )
}

pub fn skip(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    hide(
        host,
        &HIDE,
        rest,
        "usage: toolportctl attention skip <id> [--dry-run]",
        HideKind::Skip,
    )
}

pub fn dismiss(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    hide(
        host,
        &HIDE,
        rest,
        "usage: toolportctl attention dismiss <id> [--dry-run]",
        HideKind::Dismiss,
    )
}

pub fn restore(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = ONE.parse(rest)?;
    let id = id_of(&flags, "usage: toolportctl attention restore <id>")?;
    let data = ops::restore(host, &id)?;
    Ok(Output::new(data, format!("{id} is shown again")))
}

pub fn undo(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    NONE.parse(rest)?;
    let data = ops::undo(host)?;
    let text = format!(
        "Undid the last change to {}",
        data["id"].as_str().unwrap_or("")
    );
    Ok(Output::new(data, text))
}

pub fn check(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    NONE.parse(rest)?;
    let report = ops::check(host)?;
    Ok(Output::new(report.to_json(), report.human()))
}

pub fn settings(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    NONE.parse(rest)?;
    let data = ops::settings(host)?;
    Ok(Output::new(data.clone(), attention::human_settings(&data)))
}

pub fn set(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let usage = "usage: toolportctl attention set <notify|check.<name>|notify.<name>> <on|off> | stale-days <days>";
    let flags = ONE.parse(rest)?;
    let [key, state] = flags.operands() else {
        return Err(CtlError::usage(usage));
    };
    if key == "stale-days" {
        let days: u32 = state.parse().map_err(|_| CtlError::usage(usage))?;
        let data = ops::set_stale_days(host, days)?;
        return Ok(Output::new(
            json!({"key": key, "days": days, "settings": data}),
            format!("servers count as stale after {days} days without a successful call"),
        ));
    }
    let on = match state.as_str() {
        "on" => true,
        "off" => false,
        _ => return Err(CtlError::usage(usage)),
    };
    let data = ops::set_switch(host, key, on)?;
    Ok(Output::new(
        json!({"key": key, "on": on, "settings": data}),
        format!("{key} is {state}"),
    ))
}

pub fn notify(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let usage = "usage: toolportctl attention notify --test | --open-settings";
    let flags = NOTIFY.parse(rest)?;
    if flags.on("--test") == flags.on("--open-settings") {
        return Err(CtlError::usage(usage));
    }
    if flags.on("--open-settings") {
        let data =
            attention::test_toast::request_settings(host, attention::test_toast::DEFAULT_WAIT)?;
        return Ok(Output::new(
            data,
            "Opened the system's notification settings".to_string(),
        ));
    }
    let data = attention::test_toast::request(host, attention::test_toast::DEFAULT_WAIT)?;
    let human = match data["note"].as_str() {
        None => "Sent a test notification from the Toolport+ app".to_string(),
        Some(note) => format!("{note}."),
    };
    Ok(Output::new(data, human))
}
