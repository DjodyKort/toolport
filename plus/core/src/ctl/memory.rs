//! `toolportctl memory ...` and `toolportctl sessions ...`: auto memory, instruction files and
//! sessions per repository (D-144, D-145). Thin renderers over `crate::memory::ops`, which the
//! `memory_*` and `sessions_*` tools call as well.

use super::context::{flagged, plan_text};
use super::flags::{switch, value, Inline, Operands, Spec, Unknown};
use super::output::{CtlError, Output};
use crate::memory::ops::{self as api, ClearOpts, SessionsClear};
use crate::memory::store::Dest;
use crate::memory::switch::Position;
use serde_json::Value;
use toolport_plus_api::Host;

const SET_USAGE: &str = "usage: memory set default|on|off (--cwd <dir> | --global) [--dry-run]";
const UNDO_USAGE: &str = "usage: memory undo [<id>] [--cwd <dir>] [--dry-run]";
const SESSIONS_UNDO_USAGE: &str = "usage: sessions undo [<id>] [--cwd <dir>] [--dry-run]";
const RESTORE_USAGE: &str = "usage: sessions restore <archive-id> [--dry-run]";
const CLEAR_USAGE: &str = "usage: sessions clear --cwd <dir> (--ids <csv> | --older-than <days>) [--to trash|archive | --forever --confirm <name>] [--dry-run]";

const LS_BASE: Spec = Spec {
    flags: &[],
    inline: Inline::Strict,
    unknown: Unknown::Argument,
    operands: Operands::Reject,
    ..Spec::PLAIN
};

pub(super) const LS: Spec = Spec {
    flags: &[value("--cwd"), switch("--all")],
    ..LS_BASE
};

pub(super) const STATUS: Spec = Spec {
    flags: &[value("--cwd")],
    ..LS_BASE
};

pub(super) const SET: Spec = Spec {
    flags: &[value("--cwd"), switch("--global"), switch("--dry-run")],
    operands: Operands::Max(1, SET_USAGE),
    ..LS_BASE
};

pub(super) const CLEAR: Spec = Spec {
    flags: &[
        value("--cwd"),
        switch("--forever"),
        value("--confirm"),
        switch("--also-off"),
        switch("--dry-run"),
    ],
    ..LS_BASE
};

pub(super) const UNDO: Spec = Spec {
    flags: &[value("--cwd"), switch("--dry-run")],
    operands: Operands::Max(1, UNDO_USAGE),
    ..LS_BASE
};

pub(super) const OVERVIEW: Spec = Spec {
    flags: &[
        value("--cwd"),
        switch("--all"),
        value("--sort"),
        value("--filter"),
    ],
    ..LS_BASE
};

pub(super) const SESSIONS_CLEAR: Spec = Spec {
    flags: &[
        value("--cwd"),
        value("--ids"),
        value("--older-than").count("needs a whole number of days"),
        value("--to"),
        switch("--forever"),
        value("--confirm"),
        switch("--dry-run"),
    ],
    ..LS_BASE
};

pub(super) const SESSIONS_UNDO: Spec = Spec {
    operands: Operands::Max(1, SESSIONS_UNDO_USAGE),
    ..UNDO
};

pub(super) const RESTORE: Spec = Spec {
    flags: &[switch("--dry-run")],
    operands: Operands::Max(1, RESTORE_USAGE),
    ..LS_BASE
};

pub(super) const RETENTION: Spec = Spec {
    flags: &[
        value("--set").count("needs a whole number of days"),
        switch("--dry-run"),
    ],
    ..LS_BASE
};

pub fn bytes_text(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key].as_str().unwrap_or("")
}

pub fn ls(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = LS.parse(rest)?;
    let all = flags.on("--all");
    let data = api::ls(host, flags.one("--cwd"), all).map_err(flagged)?;
    let mut human = String::new();
    if all {
        for row in data["repositories"].as_array().into_iter().flatten() {
            human.push_str(&format!(
                "{:<28} {:<7} {:>4} files {:>9}  {}\n",
                text(row, "name"),
                row["switch"].as_str().unwrap_or(""),
                row["files"],
                bytes_text(row["bytes"].as_u64().unwrap_or(0)),
                text(row, "setBy"),
            ));
        }
        human.push_str(&format!(
            "{} other folder(s) with memory\n",
            data["otherFolders"].as_array().map_or(0, Vec::len)
        ));
    } else {
        human.push_str(&format!("{}\n", text(&data["directory"], "path")));
        for file in data["files"].as_array().into_iter().flatten() {
            human.push_str(&format!(
                "  {:<32} {:>9}  {}\n",
                text(file, "name"),
                bytes_text(file["bytes"].as_u64().unwrap_or(0)),
                file["type"].as_str().unwrap_or("")
            ));
        }
        if data["files"].as_array().is_none_or(Vec::is_empty) {
            human.push_str("  no auto memory kept\n");
        }
    }
    Ok(Output::new(data, human))
}

pub fn status(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = STATUS.parse(rest)?;
    let data = api::status(host, flags.one("--cwd")).map_err(flagged)?;
    let state = &data["state"];
    let mut human = format!(
        "{}: auto memory is {} (set by {}, switch {})\n",
        text(&data["repository"], "name"),
        if state["enabled"] == true {
            "on"
        } else {
            "off"
        },
        text(state, "setBy"),
        text(state, "switch"),
    );
    human.push_str(&format!(
        "  folder {}{}\n  {} file(s), {}\n",
        text(&data["directory"], "path"),
        if data["directory"]["custom"] == true {
            " (autoMemoryDirectory)"
        } else {
            ""
        },
        data["memory"]["count"],
        bytes_text(data["memory"]["bytes"].as_u64().unwrap_or(0)),
    ));
    if let Some(tokens) = data["index"]["tokens"].as_u64() {
        human.push_str(&format!(
            "  index about {tokens} tokens at every start (estimate)\n"
        ));
    }
    human.push_str(&format!(
        "  instruction files: {} row(s), about {} tokens loaded, {} left out\n",
        data["instructions"]["rows"].as_array().map_or(0, Vec::len),
        data["instructions"]["totalTokens"],
        data["instructions"]["excluded"],
    ));
    human.push_str(&format!(
        "  sessions: {} ({}), {} expiring this week at {} days\n",
        data["sessions"]["totals"]["count"],
        bytes_text(data["sessions"]["totals"]["bytes"].as_u64().unwrap_or(0)),
        data["sessions"]["totals"]["expiringThisWeek"],
        data["sessions"]["retention"]["days"],
    ));
    for note in data["notes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        human.push_str(&format!("note: {note}\n"));
    }
    Ok(Output::new(data, human))
}

pub fn set(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = SET.parse(rest)?;
    let position = flags
        .operands()
        .first()
        .and_then(|p| Position::parse(p))
        .ok_or_else(|| CtlError::usage(SET_USAGE))?;
    let global = flags.on("--global");
    if global == flags.one("--cwd").is_some() {
        return Err(CtlError::usage(SET_USAGE));
    }
    let data = api::set(
        host,
        flags.one("--cwd"),
        global,
        position,
        flags.on("--dry-run"),
    )
    .map_err(flagged)?;
    let human = plan_text(&data);
    Ok(Output::new(data, human))
}

pub fn clear(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = CLEAR.parse(rest)?;
    if flags.one("--cwd").is_none() {
        return Err(CtlError::usage(
            "usage: memory clear --cwd <dir> [--forever --confirm <name>] [--also-off] [--dry-run]",
        ));
    }
    let data = api::clear(
        host,
        flags.one("--cwd"),
        &ClearOpts {
            forever: flags.on("--forever"),
            confirm: flags.one("--confirm"),
            also_off: flags.on("--also-off"),
            dry_run: flags.on("--dry-run"),
        },
    )
    .map_err(flagged)?;
    let mut human = plan_text(&data);
    human.push_str(&format!(
        "{} file(s), {}\n",
        data["totals"]["count"],
        bytes_text(data["totals"]["bytes"].as_u64().unwrap_or(0))
    ));
    Ok(Output::new(data, human))
}

pub fn undo(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = UNDO.parse(rest)?;
    let data = api::undo(
        host,
        flags.operands().first().map(String::as_str),
        flags.one("--cwd"),
        flags.on("--dry-run"),
    )
    .map_err(flagged)?;
    let human = plan_text(&data);
    Ok(Output::new(data, human))
}

pub fn overview(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = OVERVIEW.parse(rest)?;
    let all = flags.on("--all");
    if all && flags.one("--cwd").is_some() {
        return Err(CtlError::usage("use --all or --cwd, not both"));
    }
    let data = api::overview(
        host,
        flags.one("--cwd"),
        all,
        flags.one("--sort").unwrap_or("name"),
        flags.one("--filter"),
    )
    .map_err(flagged)?;
    let mut human = String::new();
    for row in data["rows"].as_array().into_iter().flatten() {
        human.push_str(&format!(
            "{:<26} memory {:<3} {:>9}  instr ~{:>6} tok  sessions {:>4} {:>9}\n",
            text(row, "name"),
            if row["memory"]["enabled"] == true {
                "on"
            } else {
                "off"
            },
            bytes_text(row["memory"]["bytes"].as_u64().unwrap_or(0)),
            row["instructions"]["tokens"],
            row["sessions"]["count"],
            bytes_text(row["sessions"]["bytes"].as_u64().unwrap_or(0)),
        ));
    }
    human.push_str(&format!(
        "{} repositories, {} on disk\n",
        data["totals"]["repositories"],
        bytes_text(data["totals"]["bytes"].as_u64().unwrap_or(0))
    ));
    Ok(Output::new(data, human))
}

pub fn sessions_ls(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = LS.parse(rest)?;
    let all = flags.on("--all");
    let data = api::sessions_ls(host, flags.one("--cwd"), all).map_err(flagged)?;
    let mut human = String::new();
    if all {
        for row in data["repositories"].as_array().into_iter().flatten() {
            human.push_str(&format!(
                "{:<28} {:>4} sessions {:>9}\n",
                text(row, "name"),
                row["sessions"]["count"],
                bytes_text(row["sessions"]["bytes"].as_u64().unwrap_or(0)),
            ));
        }
    } else {
        for session in data["sessions"].as_array().into_iter().flatten() {
            human.push_str(&format!(
                "  {:<38} {:>9}  {:>3} days old, {} days left{}\n",
                text(session, "id"),
                bytes_text(session["bytes"].as_u64().unwrap_or(0)),
                session["ageDays"],
                session["daysLeft"],
                if session["expiring"] == true {
                    "  (expiring)"
                } else {
                    ""
                },
            ));
        }
        human.push_str(&format!(
            "{} sessions, {}; kept {} days (one value for every project)\n",
            data["totals"]["count"],
            bytes_text(data["totals"]["bytes"].as_u64().unwrap_or(0)),
            data["retention"]["days"],
        ));
    }
    Ok(Output::new(data, human))
}

pub fn sessions_clear(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = SESSIONS_CLEAR.parse(rest)?;
    if flags.one("--cwd").is_none() {
        return Err(CtlError::usage(CLEAR_USAGE));
    }
    let dest = match (flags.on("--forever"), flags.one("--to")) {
        (true, None) => Dest::Forever,
        (false, None | Some("trash")) => Dest::Trash,
        (false, Some("archive")) => Dest::Archive,
        _ => return Err(CtlError::usage(CLEAR_USAGE)),
    };
    let ids: Vec<String> = flags
        .one("--ids")
        .map(|csv| {
            csv.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();
    let data = api::sessions_clear(
        host,
        flags.one("--cwd"),
        &SessionsClear {
            ids,
            older_than: flags.count("--older-than"),
            dest,
            confirm: flags.one("--confirm"),
            dry_run: flags.on("--dry-run"),
        },
    )
    .map_err(flagged)?;
    let mut human = plan_text(&data);
    human.push_str(&format!(
        "{} session(s), {}\n",
        data["sessions"].as_array().map_or(0, Vec::len),
        bytes_text(data["totalBytes"].as_u64().unwrap_or(0))
    ));
    Ok(Output::new(data, human))
}

pub fn sessions_undo(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = SESSIONS_UNDO.parse(rest)?;
    let data = api::sessions_undo(
        host,
        flags.operands().first().map(String::as_str),
        flags.one("--cwd"),
        flags.on("--dry-run"),
    )
    .map_err(flagged)?;
    let human = plan_text(&data);
    Ok(Output::new(data, human))
}

pub fn sessions_restore(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = RESTORE.parse(rest)?;
    let id = flags.single(RESTORE_USAGE)?;
    let data = api::sessions_restore(host, id, flags.on("--dry-run"))?;
    let human = plan_text(&data);
    Ok(Output::new(data, human))
}

pub fn sessions_retention(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = RETENTION.parse(rest)?;
    let data = api::sessions_retention(host, flags.count("--set"), flags.on("--dry-run"))?;
    let human = if data["plan"].is_null() {
        format!(
            "sessions are kept {} days ({}); {}\n",
            data["retention"]["days"],
            text(&data["retention"], "source"),
            text(&data, "note")
        )
    } else {
        plan_text(&data)
    };
    Ok(Output::new(data, human))
}
