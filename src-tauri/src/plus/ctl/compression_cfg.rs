//! `toolportctl compression enable|disable|set-provider|use|sync|pin|seal|env|doctor`: flag
//! parsing and mcpm-style text over the cores in `plus::compression::manage`.

use super::compression::spec;
use super::flags::{switch, value, Spec};
use super::output::{CtlError, Output};
use crate::plus::compression::manage::{
    self, parse_mode, parse_provider, parse_telemetry, with_system, CmdError, Ctx, EnableReq, Kind,
    PinReq, SealReq,
};
use crate::plus::compression::model::ProviderName;
use crate::plus::compression::verify::HealthProbe;
use serde_json::Value;
use std::path::PathBuf;

pub(super) const USAGE: &str = "usage: compression \
     status|presets|enable|disable|set-provider|use|sync|pin|seal|env|doctor|run|verify|ledger|proxy|update \
     (enable: [--provider <name>] [--port <n>] [--telemetry on|off] [--preset <name>] \
     [--mode cache|token] [--dry-run]; disable: [--teardown] [--dry-run]; \
     set-provider: <provider> [--dry-run]; use: <preset> [--dry-run]; \
     sync: [--mcpm-root <dir>] [--dry-run]; pin: [<version>] [--install] [--refresh] [--dry-run]; \
     seal: [<preset>] [--apply] [--dry-run]; env: [--cwd <dir>]; presets: [--refresh] [--dry-run])";

impl From<CmdError> for CtlError {
    fn from(e: CmdError) -> Self {
        match e.kind {
            Kind::Usage => CtlError::usage(e.message),
            Kind::Failed(code) => CtlError::failed(code, e.message),
        }
    }
}

pub(super) const ENABLE: Spec = spec(&[
    value("--provider"),
    value("--port"),
    value("--telemetry"),
    value("--preset"),
    value("--mode"),
    switch("--dry-run"),
]);
pub(super) const DISABLE: Spec = spec(&[switch("--teardown"), switch("--dry-run")]);
pub(super) const DRY: Spec = spec(&[switch("--dry-run")]);
pub(super) const SYNC: Spec = spec(&[value("--mcpm-root"), switch("--dry-run")]);
pub(super) const PIN: Spec = spec(&[
    switch("--install"),
    switch("--refresh"),
    switch("--dry-run"),
]);
pub(super) const SEAL: Spec = spec(&[switch("--apply"), switch("--dry-run")]);
pub(super) const ENV: Spec = spec(&[value("--cwd")]);
pub(super) const PRESETS: Spec = spec(&[switch("--refresh"), switch("--dry-run")]);

pub fn group(_rest: &[String]) -> Result<Output, CtlError> {
    Err(CtlError::usage(USAGE))
}

fn text(data: &Value, key: &str) -> String {
    data[key].as_str().unwrap_or_default().to_string()
}

fn lines(data: &Value, key: &str) -> Vec<String> {
    data[key]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

fn dash(value: &Value) -> String {
    value.as_str().unwrap_or("\u{2014}").to_string()
}

fn preset_line(data: &Value) -> String {
    let p = &data["preset"];
    format!(
        "mode={}, profile={}, port={}",
        text(p, "mode"),
        dash(&p["savingsProfile"]),
        p["port"]
    )
}

fn report(data: &Value, out: &mut Vec<String>) {
    out.extend(
        lines(data, "actions")
            .into_iter()
            .map(|a| format!("  \u{2713} {a}")),
    );
    out.extend(
        lines(data, "warnings")
            .into_iter()
            .map(|w| format!("  ! {w}")),
    );
    if data["dryRun"] == true {
        out.push("  (dry run: nothing was written)".into());
    }
}

fn render(data: Value, out: Vec<String>) -> Output {
    Output::new(data, out.join("\n"))
}

fn no_operands(flags: &super::flags::Flags) -> Result<(), CtlError> {
    super::output::no_args(flags.operands())
}

pub(super) fn enable_with(cx: &mut Ctx, rest: &[String]) -> Result<Output, CtlError> {
    let flags = ENABLE.parse(rest)?;
    no_operands(&flags)?;
    let req = EnableReq {
        provider: flags.one("--provider").map(parse_provider).transpose()?,
        port: flags.number::<u16>("--port")?,
        telemetry: flags.one("--telemetry").map(parse_telemetry).transpose()?,
        preset: flags.one("--preset").map(String::from),
        mode: flags.one("--mode").map(parse_mode).transpose()?,
        dry_run: flags.on("--dry-run"),
    };
    let data = manage::enable(cx, &req)?;
    let mut out = vec![format!(
        "Enabling compression: {} (preset {}, mode {})",
        text(&data, "provider"),
        text(&data["preset"], "name"),
        text(&data["preset"], "mode"),
    )];
    report(&data, &mut out);
    out.push(String::new());
    out.push("Next steps:".into());
    out.extend(
        lines(&data, "nextSteps")
            .into_iter()
            .map(|s| format!("  \u{2022} {s}")),
    );
    Ok(render(data, out))
}

pub fn enable(rest: &[String]) -> Result<Output, CtlError> {
    with_system(|cx| enable_with(cx, rest))
}

pub(super) fn disable_with(cx: &mut Ctx, rest: &[String]) -> Result<Output, CtlError> {
    let flags = DISABLE.parse(rest)?;
    no_operands(&flags)?;
    let data = manage::disable(cx, flags.on("--teardown"), flags.on("--dry-run"))?;
    let mut out = vec!["Disabling compression".to_string()];
    report(&data, &mut out);
    if flags.on("--teardown") {
        out.push("  Note: ~/.headroom/ data (toin/savings/memory) is left in place.".into());
    }
    Ok(render(data, out))
}

pub fn disable(rest: &[String]) -> Result<Output, CtlError> {
    with_system(|cx| disable_with(cx, rest))
}

pub(super) fn set_provider_with(cx: &mut Ctx, rest: &[String]) -> Result<Output, CtlError> {
    let flags = DRY.parse(rest)?;
    let name = flags.single(&format!(
        "set-provider needs a provider ({})",
        ProviderName::ALL.map(ProviderName::as_str).join(", ")
    ))?;
    let provider = parse_provider(name)?;
    let data = manage::set_provider(cx, provider, flags.on("--dry-run"))?;
    let mut out = vec![format!("Switching provider \u{2192} {}", provider.as_str())];
    report(&data, &mut out);
    out.push("  Then toolportctl client sync.".into());
    Ok(render(data, out))
}

pub fn set_provider(rest: &[String]) -> Result<Output, CtlError> {
    with_system(|cx| set_provider_with(cx, rest))
}

pub(super) fn use_with(cx: &mut Ctx, rest: &[String]) -> Result<Output, CtlError> {
    let flags = DRY.parse(rest)?;
    let name = flags.single("use needs a preset name")?;
    let data = manage::use_preset(cx, name, flags.on("--dry-run"))?;
    let mut out = vec![format!(
        "Active preset \u{2192} {name} ({})",
        preset_line(&data)
    )];
    report(&data, &mut out);
    out.push(
        "  Note: mode is fixed at proxy cold-start \u{2014} restart the proxy to apply a mode change."
            .into(),
    );
    Ok(render(data, out))
}

pub fn use_preset(rest: &[String]) -> Result<Output, CtlError> {
    with_system(|cx| use_with(cx, rest))
}

pub(super) fn sync_with(cx: &mut Ctx, rest: &[String]) -> Result<Output, CtlError> {
    let flags = SYNC.parse(rest)?;
    no_operands(&flags)?;
    let root = flags.one("--mcpm-root").map(PathBuf::from);
    let data = manage::sync(cx, root.as_deref(), flags.on("--dry-run"))?;
    let mut out = Vec::new();
    report(&data, &mut out);
    Ok(render(data, out))
}

pub fn sync(rest: &[String]) -> Result<Output, CtlError> {
    with_system(|cx| sync_with(cx, rest))
}

pub(super) fn env_with(cx: &mut Ctx, rest: &[String]) -> Result<Output, CtlError> {
    let flags = ENV.parse(rest)?;
    no_operands(&flags)?;
    let cwd = match flags.one("--cwd") {
        Some(cwd) => cwd.to_string(),
        None => std::env::current_dir()
            .map_err(|e| CtlError::failed("cwd", e.to_string()))?
            .to_string_lossy()
            .into_owned(),
    };
    let data = manage::env(cx, &cwd)?;
    let human = lines(&data, "lines").join("\n");
    Ok(Output::new(data, human))
}

pub fn env(rest: &[String]) -> Result<Output, CtlError> {
    with_system(|cx| env_with(cx, rest))
}

fn pin_table(data: &Value) -> Vec<String> {
    let installed = data["installed"].as_str().unwrap_or("not on PATH");
    let mut rows = vec![
        ("pin", text(data, "pin")),
        ("requirement", text(data, "requirement")),
        ("installed", installed.to_string()),
    ];
    if data["drift"] == true {
        rows.push(("", "drift \u{2014} installed \u{2260} pin".into()));
    }
    let width = rows.iter().map(|(k, _)| k.len()).max().unwrap_or(0);
    rows.iter()
        .map(|(k, v)| format!("{k:<width$}  {v}").trim_end().to_string())
        .collect()
}

pub(super) fn refresh_lines(refresh: &Value, dry_run: bool) -> Vec<String> {
    let version = refresh["version"].as_str().unwrap_or("unknown");
    let verb = if dry_run {
        "would be re-snapshotted"
    } else {
        "re-snapshotted"
    };
    let mut out = Vec::new();
    let mut changed = false;
    for p in refresh["presets"].as_array().into_iter().flatten() {
        if p["changed"] == true {
            changed = true;
            out.push(format!(
                "  \u{2713} preset '{}' {verb} from {version}",
                text(p, "name")
            ));
            for a in p["added"].as_array().into_iter().flatten() {
                out.push(format!("      + {}={}", text(a, "knob"), text(a, "value")));
            }
            for r in p["removed"].as_array().into_iter().flatten() {
                out.push(format!(
                    "      - {}  (was {})",
                    text(r, "knob"),
                    text(r, "was")
                ));
            }
            for m in p["moved"].as_array().into_iter().flatten() {
                out.push(format!(
                    "      ~ {}: {} \u{2192} {}",
                    text(m, "knob"),
                    text(m, "from"),
                    text(m, "to")
                ));
            }
        }
        let kept = lines(p, "kept");
        if !kept.is_empty() {
            out.push(format!(
                "      kept {} declared knob(s): {}",
                kept.len(),
                kept.join(", ")
            ));
        }
    }
    if !changed {
        out.push(format!("  \u{2713} preset knobs already match {version}"));
    }
    out
}

pub(super) fn pin_with(cx: &mut Ctx, rest: &[String]) -> Result<Output, CtlError> {
    let flags = PIN.parse(rest)?;
    let version = match flags.operands() {
        [] => None,
        [one] => Some(one.clone()),
        [_, extra, ..] => return Err(CtlError::usage(format!("unexpected argument: {extra}"))),
    };
    let req = PinReq {
        version,
        install: flags.on("--install"),
        refresh: flags.on("--refresh"),
        dry_run: flags.on("--dry-run"),
    };
    let data = manage::pin(cx, &req)?;
    let mut out = Vec::new();
    if data["set"] == true {
        let verb = if req.dry_run { "Would pin" } else { "Pin" };
        out.push(format!(
            "{verb} \u{2192} {}  (requirement: {})",
            text(&data, "pin"),
            text(&data, "requirement")
        ));
        out.push(
            "  presets still hold values snapshotted from another build \u{2014} re-snapshot \
             with `--install --refresh`."
                .into(),
        );
    }
    out.extend(pin_table(&data));
    if !req.install && !req.refresh {
        if data["drift"] == true {
            out.push(String::new());
            out.push("  Install the pin: toolportctl compression pin --install --refresh".into());
        }
        return Ok(render(data, out));
    }
    if req.install {
        out.push(String::new());
        let install = &data["install"];
        if req.dry_run {
            out.push(format!(
                "Would install {} (dry run)",
                text(install, "requirement")
            ));
        } else {
            out.push(format!("Installing {}", text(install, "requirement")));
            out.push(format!("  \u{2713} {}", text(install, "detail")));
        }
    }
    if req.refresh {
        if req.install {
            out.extend(refresh_lines(&data["refresh"], req.dry_run));
        } else {
            out.push(String::new());
            out.extend(refresh_lines(&data["refresh"], req.dry_run));
        }
    }
    if data["restartProxies"] == true {
        out.push(
            "  \u{2022} restart proxies to run the pinned build: toolportctl compression proxy \
             restart (close attached sessions first)"
                .into(),
        );
    }
    if req.dry_run {
        out.push("  (dry run: nothing was written)".into());
    }
    Ok(render(data, out))
}

pub fn pin(rest: &[String]) -> Result<Output, CtlError> {
    with_system(|cx| pin_with(cx, rest))
}

pub(super) fn seal_with(cx: &mut Ctx, rest: &[String]) -> Result<Output, CtlError> {
    let flags = SEAL.parse(rest)?;
    let preset = match flags.operands() {
        [] => None,
        [one] => Some(one.clone()),
        [_, extra, ..] => return Err(CtlError::usage(format!("unexpected argument: {extra}"))),
    };
    let req = SealReq {
        preset,
        apply: flags.on("--apply"),
        dry_run: flags.on("--dry-run"),
    };
    let data = manage::seal(cx, &req)?;
    let name = text(&data, "preset");
    if data["complete"] == true {
        let human = format!("  \u{2713} preset '{name}' already declares its whole posture");
        return Ok(Output::new(data, human));
    }
    let version = data["version"].as_str().unwrap_or("unknown");
    let mut declarable: Vec<(String, String)> = data["declarable"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|d| (text(d, "knob"), text(d, "value")))
        .collect();
    declarable.sort();
    let mut unset = lines(&data, "unset");
    unset.sort();
    let mut out = vec![format!(
        "Posture of '{name}' on :{} (headroom {version})",
        data["port"]
    )];
    for (k, v) in &declarable {
        out.push(format!(
            "  + {k}={v}   vendor default \u{2192} would become policy"
        ));
    }
    for k in &unset {
        out.push(format!(
            "  \u{b7} {k}  unset \u{2014} headroom's internal default; not expressible as env, \
             stable only because the build is pinned"
        ));
    }
    if !req.apply {
        out.push(String::new());
        out.push(format!(
            "  {} declarable, {} unsealable. Apply with toolportctl compression seal {name} --apply",
            declarable.len(),
            unset.len()
        ));
        return Ok(render(data, out));
    }
    out.push(String::new());
    if req.dry_run {
        out.push(format!(
            "  would seal {} knob(s) into '{name}' as policy (dry run: nothing was written)",
            declarable.len()
        ));
    } else {
        out.push(format!(
            "  \u{2713} sealed {} knob(s) into '{name}' as policy (runtime unchanged \u{2014} these \
             are the values already in force)",
            data["sealed"]
        ));
    }
    if !unset.is_empty() {
        out.push(format!(
            "  {} setting(s) remain at headroom's internal default; a pin bump will diff them.",
            unset.len()
        ));
    }
    Ok(render(data, out))
}

pub fn seal(rest: &[String]) -> Result<Output, CtlError> {
    with_system(|cx| seal_with(cx, rest))
}

pub(super) fn doctor_with(
    cx: &mut Ctx,
    rest: &[String],
    probe: &dyn HealthProbe,
) -> Result<Output, CtlError> {
    super::output::no_args(rest)?;
    let data = manage::doctor(cx, probe)?;
    let mut out = Vec::new();
    for note in lines(&data, "migrated") {
        out.push(format!("  migrated {note}"));
    }
    let mut failed = false;
    for c in data["checks"].as_array().into_iter().flatten() {
        let ok = c["ok"] == true;
        failed |= !ok;
        out.push(format!(
            "  {} {:<20} {}",
            if ok { "ok  " } else { "FAIL" },
            text(c, "name"),
            text(c, "detail")
        ));
    }
    Ok(Output {
        data,
        human: out.join("\n"),
        failed,
    })
}

pub fn doctor(rest: &[String]) -> Result<Output, CtlError> {
    with_system(|cx| {
        doctor_with(
            cx,
            rest,
            &crate::plus::compression::launch::SystemOps::new(),
        )
    })
}
