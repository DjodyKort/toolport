//! `toolportctl spend ...`: model spend from Claude Code's OpenTelemetry export (S10). Thin
//! renderers over `spend::api`, which the spend tools and the Spend page call as well.

use super::flags::{switch, value, Inline, Operands, Spec, Unknown};
use super::output::{CtlError, Output};
use crate::spend::api;
use serde_json::Value;
use toolport_plus_api::Host;

const NO_ARGS: &str = "usage: spend status | receiver status | summary [--range today|7d|30d] [--project <folder>] | export-csv [--range <r>] [--out <file>] | prune [--dry-run] [--totals --yes] | setup plan|apply|undo [--port <n>] [--overwrite <KEY>|all]";

const BASE: Spec = Spec {
    flags: &[],
    inline: Inline::Strict,
    unknown: Unknown::Argument,
    operands: Operands::Max(0, NO_ARGS),
    ..Spec::PLAIN
};

pub(super) const STATUS: Spec = BASE;

pub(super) const SUMMARY: Spec = Spec {
    flags: &[value("--range"), value("--project")],
    ..BASE
};

pub(super) const EXPORT: Spec = Spec {
    flags: &[value("--range"), value("--out")],
    ..BASE
};

pub(super) const PRUNE: Spec = Spec {
    flags: &[switch("--dry-run"), switch("--totals"), switch("--yes")],
    ..BASE
};

pub(super) const SETUP_PLAN: Spec = Spec {
    flags: &[value("--port"), value("--overwrite")],
    ..BASE
};

pub(super) const SETUP_APPLY: Spec = Spec {
    flags: &[value("--port"), value("--overwrite"), switch("--dry-run")],
    ..BASE
};

pub(super) const SETUP_UNDO: Spec = Spec {
    flags: &[switch("--dry-run")],
    ..BASE
};

fn usd(value: &Value) -> String {
    format!(
        "${:.2} ({})",
        value["usd"].as_f64().unwrap_or(0.0),
        value["badge"].as_str().unwrap_or("")
    )
}

pub fn status(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    STATUS.parse(rest)?;
    let data = api::status(host)?;
    Ok(Output::new(data.clone(), status_text(&data)))
}

pub fn receiver_status(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    STATUS.parse(rest)?;
    let data = api::receiver_status(host)?;
    let human = match data["receiver"]["error"].as_str() {
        Some(error) => format!("Receiver: {error}"),
        None if data["receiver"]["listening"] == true => format!(
            "Receiver listening on {} (started by the {})",
            data["receiver"]["address"].as_str().unwrap_or(""),
            data["receiver"]["owner"].as_str().unwrap_or("app")
        ),
        None => format!(
            "Receiver not listening on {} (collection is {})",
            data["receiver"]["address"].as_str().unwrap_or(""),
            if data["enabled"] == true { "on" } else { "off" }
        ),
    };
    Ok(Output::new(data, human))
}

fn status_text(data: &Value) -> String {
    let store = &data["store"];
    format!(
        "Collecting: {}\nReceiver: {} (listening: {})\nLast event: {}\nStore: {} events, {} of {} bytes, daily totals {}",
        data["state"].as_str().unwrap_or(""),
        data["receiver"]["address"].as_str().unwrap_or(""),
        data["receiver"]["listening"],
        data["lastEvent"].as_str().unwrap_or("none yet"),
        store["events"],
        store["bytes"],
        store["maxBytes"],
        store["dailyLines"],
    )
}

pub fn summary(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = SUMMARY.parse(rest)?;
    let data = api::summary(host, flags.one("--range"), flags.one("--project"))?;
    let mut human = vec![format!(
        "Spend, {} since {}: {} · {} tokens",
        data["range"].as_str().unwrap_or(""),
        data["since"].as_str().unwrap_or(""),
        usd(&data["totals"]["cost"]),
        data["totals"]["tokens"]["total"]
    )];
    for (title, key) in [
        ("Project", "byProject"),
        ("Model", "byModel"),
        ("Task", "byTask"),
    ] {
        for row in data[key].as_array().into_iter().flatten() {
            human.push(format!(
                "{title:<8} {:<40} {}",
                row["key"].as_str().unwrap_or(""),
                usd(&row["cost"])
            ));
        }
    }
    Ok(Output::new(data, human.join("\n")))
}

pub fn export_csv(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = EXPORT.parse(rest)?;
    let (body, rows) = api::export_csv(host, flags.one("--range"))?;
    let Some(out) = flags.one("--out") else {
        return Ok(Output::new(
            serde_json::json!({"rows": rows, "csv": body}),
            body,
        ));
    };
    let path = std::path::absolute(out).map_err(|e| CtlError::usage(format!("--out: {e}")))?;
    crate::fsx::atomic_write(&path, &body, crate::fsx::Mode::KeepOrDefault)
        .map_err(|e| CtlError::failed("save_failed", format!("{}: {e}", path.display())))?;
    let shown = path.to_string_lossy().into_owned();
    Ok(Output::new(
        serde_json::json!({"rows": rows, "out": shown, "bytes": body.len()}),
        format!("Saved to {shown}"),
    ))
}

pub fn prune(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = PRUNE.parse(rest)?;
    let (dry, totals) = (flags.on("--dry-run"), flags.on("--totals"));
    if totals && !dry && !flags.on("--yes") {
        return Err(CtlError::usage(
            "--totals deletes the daily totals for good: add --yes, or --dry-run to look first",
        ));
    }
    let data = api::prune(host, dry, totals)?;
    let human = format!(
        "{} {} events ({} bytes); {} kept",
        if dry { "Would remove" } else { "Removed" },
        data["removedEvents"],
        data["removedBytes"],
        data["keptEvents"]
    );
    Ok(Output::new(data, human))
}

fn plan_text(plan: &Value) -> String {
    let mut human = vec![format!(
        "Endpoint {}",
        plan["endpoint"].as_str().unwrap_or("")
    )];
    for profile in plan["profiles"].as_array().into_iter().flatten() {
        human.push(profile["profile"].as_str().unwrap_or("").to_string());
        for e in profile["entries"].as_array().into_iter().flatten() {
            human.push(format!(
                "  {:<30} {}",
                e["key"].as_str().unwrap_or(""),
                e["action"].as_str().unwrap_or("")
            ));
        }
    }
    human.push(plan["note"].as_str().unwrap_or("").to_string());
    human.join("\n")
}

pub fn setup_plan(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = SETUP_PLAN.parse(rest)?;
    let data = api::setup_plan(host, flags.one("--port"), &flags.all("--overwrite"))?;
    Ok(Output::new(data.clone(), plan_text(&data)))
}

pub fn setup_apply(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = SETUP_APPLY.parse(rest)?;
    let data = api::setup_apply(
        host,
        flags.one("--port"),
        &flags.all("--overwrite"),
        flags.on("--dry-run"),
    )?;
    let human = if data["dryRun"] == true {
        format!("Dry run, nothing written\n{}", plan_text(&data["plan"]))
    } else if data["complete"] == true {
        "Collection is on. Restart open Claude Code sessions so they start exporting".to_string()
    } else {
        "Applied, but some keys already hold other values and were left alone; see setup plan"
            .to_string()
    };
    Ok(Output::new(data, human))
}

pub fn setup_undo(host: &dyn Host, rest: &[String]) -> Result<Output, CtlError> {
    let flags = SETUP_UNDO.parse(rest)?;
    let data = api::setup_undo(host, flags.on("--dry-run"))?;
    let human = if data["dryRun"] == true {
        format!("Dry run: {} profile(s) set up", data["profilesSetUp"])
    } else {
        "Collection is off; the keys Toolport wrote are removed".to_string()
    };
    Ok(Output::new(data, human))
}
