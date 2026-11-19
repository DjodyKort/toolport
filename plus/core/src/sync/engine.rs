//! `sync status|setup|push|pull|undo`. A push builds the setup file in its sealed form and puts it
//! on the transport; a pull plans the file on the transport against this machine as a restore
//! does, with the baseline telling a change of the other machine from one of both. Nothing is
//! applied without the plan hash of a dry run, and nothing is merged.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::config::{
    self, Config, LastApply, LastRun, State, TransportCfg, DEFAULT_BRANCH, FILE_NAME, SYNC_PARTS,
};
use super::peers::{self, Peers};
use super::transport::{self, Transport};
use crate::op::OpError;
use crate::setup::crypto::Passphrase;
use crate::setup::export::{self, Options};
use crate::setup::restore::{self, RestoreOptions};
use crate::setup::threeway::Baseline;
use crate::setup::{crypto, inspect, parts as setup_parts, Ctx, PartId};
use crate::skills::git::GitRunner;
use crate::tasks::cron::rfc3339;

pub const TASK_ID: &str = crate::tasks::builtin::SYNC_PULL;
const UPSTREAM_REASON: &str = "handled by upstream Sync: one writer per part";

fn failed(why: impl std::fmt::Display) -> OpError {
    OpError::failed("sync", why.to_string())
}

struct Scratch(PathBuf);

impl Scratch {
    fn new(ctx: &Ctx<'_>) -> Result<Self, OpError> {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let dir = config::dir(ctx).join("tmp").join(format!(
            "run-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).map_err(|e| failed(format!("{}: {e}", dir.display())))?;
        Ok(Self(dir))
    }

    fn file(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
        if let Some(parent) = self.0.parent() {
            let _ = std::fs::remove_dir(parent);
        }
    }
}

fn require_config(ctx: &Ctx<'_>) -> Result<Config, OpError> {
    config::read_config(ctx)?
        .ok_or_else(|| OpError::not_found("sync is not set up: run `toolportctl sync setup`"))
}

fn open_transport<'a>(
    ctx: &Ctx<'_>,
    cfg: &Config,
    git: &'a dyn GitRunner,
) -> Box<dyn Transport + 'a> {
    transport::open(&cfg.transport, config::dir(ctx).join("repo"), git)
}

/// The parts this machine syncs now and the ones left out with the reason.
fn effective_parts(ctx: &Ctx<'_>, cfg: &Config) -> Result<(BTreeSet<PartId>, Vec<Value>), OpError> {
    let mut parts = cfg.parts();
    let mut skipped = Vec::new();
    if parts.contains(&PartId::Servers) && ctx.host.upstream_sync_active().map_err(failed)? {
        parts.remove(&PartId::Servers);
        skipped.push(json!({"part": "servers", "reason": UPSTREAM_REASON}));
    }
    Ok((parts, skipped))
}

fn passphrase_of(ctx: &Ctx<'_>, unattended: bool) -> Result<Passphrase, OpError> {
    match ctx.host.sync_passphrase(unattended).map_err(|why| {
        OpError::failed(
            "keyring",
            format!("the passphrase could not be read from the keychain: {why}"),
        )
    })? {
        Some(pass) => Ok(Passphrase::from(pass)),
        None => Err(OpError::failed(
            "passphrase",
            "no passphrase is stored on this machine: run `toolportctl sync setup --passphrase-stdin`",
        )),
    }
}

fn unseal(sealed: &Path, passphrase: &str, out: &Path) -> Result<(), OpError> {
    let mut input =
        std::fs::File::open(sealed).map_err(|e| failed(format!("{}: {e}", sealed.display())))?;
    let mut plain = setup_parts::create_private(out).map_err(failed)?;
    crypto::open(passphrase, &mut input, &mut plain).map_err(|e| match e {
        crypto::OpenError::Wrong => OpError::failed(
            "passphrase",
            "that passphrase does not open the sync file: nothing was changed",
        ),
        other => failed(other),
    })
}

fn now_text(ctx: &Ctx<'_>) -> String {
    rfc3339(ctx.host.now_unix())
}

fn schedule_json(ctx: &Ctx<'_>) -> Value {
    let Some(base) = ctx.host.tasks_base() else {
        return Value::Null;
    };
    match crate::tasks::store::load_task(&base, TASK_ID) {
        Ok(task) => json!({
            "task": TASK_ID,
            "present": true,
            "enabled": task.enabled,
            "cron": task.triggers.schedule.as_ref().map(|s| s.cron.as_str()),
        }),
        Err(_) => json!({"task": TASK_ID, "present": false, "enabled": false, "cron": null}),
    }
}

fn parts_json(ctx: &Ctx<'_>, cfg: Option<&Config>) -> Result<Vec<Value>, OpError> {
    let upstream = ctx.host.upstream_sync_active().unwrap_or(false);
    let on = cfg.map(Config::parts).unwrap_or_default();
    Ok(SYNC_PARTS
        .iter()
        .map(|part| {
            let handled = upstream && *part == PartId::Servers;
            json!({
                "id": part.id(),
                "label": part.label(),
                "on": on.contains(part) && !handled,
                "default": config::DEFAULT_PARTS.contains(part),
                "handledBy": handled.then_some("upstream"),
            })
        })
        .collect())
}

fn machines_json(ctx: &Ctx<'_>, state: &State) -> Vec<Value> {
    let this = export::machine_name();
    let mut machines: Vec<Value> = state
        .machines
        .iter()
        .map(|(name, at)| json!({"machine": name, "at": at, "thisMachine": *name == this}))
        .collect();
    if !state.machines.contains_key(&this) {
        machines.push(json!({"machine": this, "at": null, "thisMachine": true}));
    }
    let _ = ctx;
    machines
}

/// `sync status`: what is set up and when it last ran. Reads local files and the keychain entry
/// only; the network is not touched.
pub fn status(ctx: &Ctx<'_>) -> Result<Value, OpError> {
    let cfg = config::read_config(ctx)?;
    let state = config::read_state(ctx)?;
    let locked = ctx.host.keyring_locked();
    let passphrase = if locked {
        "locked"
    } else {
        match ctx.host.sync_passphrase(true) {
            Ok(Some(pass)) => {
                let _wiped = Passphrase::from(pass);
                "stored"
            }
            Ok(None) => "missing",
            Err(_) => "locked",
        }
    };
    Ok(json!({
        "configured": cfg.is_some(),
        "enabled": cfg.as_ref().is_some_and(|c| c.enabled),
        "transport": cfg.as_ref().map(|c| config::transport_json(&c.transport)),
        "parts": parts_json(ctx, cfg.as_ref())?,
        "passphrase": passphrase,
        "keyringLocked": locked,
        "paused": locked && cfg.as_ref().is_some_and(|c| c.enabled),
        "upstream": {"active": ctx.host.upstream_sync_active().unwrap_or(false)},
        "lastSync": state.last_sync,
        "lastCheck": state.last_check,
        "pending": state.pending,
        "remoteSeen": state.remote_seen,
        "machines": machines_json(ctx, &state),
        "schedule": schedule_json(ctx),
        "undoable": state.last_apply.as_ref().map(|a| a.run.clone()),
    }))
}

#[derive(Clone, Debug, Default)]
pub struct SetupOptions {
    pub git_url: Option<String>,
    pub branch: Option<String>,
    pub folder: Option<PathBuf>,
    pub parts: Option<BTreeSet<PartId>>,
    pub passphrase: Option<Passphrase>,
    pub enabled: Option<bool>,
    pub schedule: Option<bool>,
    pub forget_passphrase: bool,
    pub dry_run: bool,
}

fn transport_of(opts: &SetupOptions) -> Result<Option<TransportCfg>, OpError> {
    match (&opts.git_url, &opts.folder) {
        (Some(_), Some(_)) => Err(OpError::usage("name a git URL or a folder, not both")),
        (Some(url), None) => {
            crate::skills::taps::valid_source_url(url).map_err(|e| {
                OpError::usage(
                    e.replace("tap url", "sync url")
                        .replace("tap source", "sync source"),
                )
            })?;
            if let Some(host) = transport::url_host(url) {
                crate::egress::check(&host).map_err(|e| OpError::failed("egress", e))?;
            }
            Ok(Some(TransportCfg::Git {
                url: url.clone(),
                branch: opts
                    .branch
                    .clone()
                    .unwrap_or_else(|| DEFAULT_BRANCH.to_string()),
            }))
        }
        (None, Some(path)) => {
            if !path.is_dir() {
                return Err(OpError::usage(format!(
                    "{} is not a folder",
                    path.display()
                )));
            }
            Ok(Some(TransportCfg::Folder {
                path: path.to_string_lossy().into_owned(),
            }))
        }
        (None, None) => Ok(None),
    }
}

/// `sync setup`: where the file goes, which parts travel, the passphrase for this machine's
/// keychain and the schedule. Changing the transport starts over: the baseline belongs to it.
pub fn setup(ctx: &Ctx<'_>, opts: &SetupOptions) -> Result<Value, OpError> {
    let found = transport_of(opts)?;
    let existing = config::read_config(ctx)?;
    let mut cfg = match (&existing, found.clone()) {
        (Some(cfg), _) => cfg.clone(),
        (None, Some(transport)) => Config::new(transport),
        (None, None) => return Err(OpError::usage(
            "sync needs a place to keep the file: --git <url> [--branch <name>] or --folder <path>",
        )),
    };
    if let Some(transport) = found {
        cfg.transport = transport;
    } else if let (
        Some(branch),
        TransportCfg::Git {
            branch: current, ..
        },
    ) = (&opts.branch, &mut cfg.transport)
    {
        current.clone_from(branch);
    }
    let reset = existing
        .as_ref()
        .is_some_and(|e| e.transport != cfg.transport);
    if let Some(parts) = &opts.parts {
        if let Some(bad) = parts.iter().find(|p| !SYNC_PARTS.contains(p)) {
            return Err(OpError::usage(format!(
                "{} does not sync: use {}",
                bad.id(),
                SYNC_PARTS.map(PartId::id).join(", ")
            )));
        }
        cfg.parts = parts.iter().map(|p| p.id().to_string()).collect();
    }
    if let Some(enabled) = opts.enabled {
        cfg.enabled = enabled;
    }
    if let Some(pass) = &opts.passphrase {
        crypto::check_passphrase(pass).map_err(OpError::usage)?;
    }
    if opts.dry_run {
        let mut data = status(ctx)?;
        data["dryRun"] = json!(true);
        data["transport"] = json!(config::transport_json(&cfg.transport));
        return Ok(data);
    }
    config::write_config(ctx, &cfg)?;
    if reset {
        config::write_state(ctx, &State::default())?;
        let _ = std::fs::remove_dir_all(config::dir(ctx).join("repo"));
    }
    if let Some(pass) = &opts.passphrase {
        ctx.host.sync_store_passphrase(pass).map_err(|why| {
            OpError::failed("keyring", format!("the passphrase was not stored: {why}"))
        })?;
    }
    if opts.forget_passphrase {
        ctx.host.sync_clear_passphrase().map_err(failed)?;
    }
    let mut data = status(ctx)?;
    if let Some(on) = opts.schedule {
        data["schedule"] = crate::tasks::api::set_enabled(ctx.host, TASK_ID, on, false)?;
    }
    data["dryRun"] = json!(false);
    Ok(data)
}

fn record_machine(ctx: &Ctx<'_>, state: &mut State, manifest: &Value) {
    if let Some(machine) = manifest["machine"].as_str() {
        let at = manifest["madeAt"]
            .as_str()
            .map_or_else(|| now_text(ctx), String::from);
        state.machines.insert(machine.to_string(), at);
    }
}

#[derive(Clone, Debug, Default)]
pub struct PushOptions {
    pub dry_run: bool,
    /// Push although the last pull left rows of both machines unsettled: the other machine's
    /// version of them is replaced (git keeps it in the history).
    pub keep_mine: bool,
}

fn moved_error() -> OpError {
    OpError::conflict(transport::MOVED)
}

/// What the plan of the pushed file against this machine says is agreed: every row that is the
/// same on both sides, by its hash.
fn agreed_rows(
    ctx: &Ctx<'_>,
    plain: &Path,
    passphrase: &str,
    parts: &BTreeSet<PartId>,
) -> Result<BTreeMap<String, String>, OpError> {
    let plan = restore::run(
        ctx,
        &RestoreOptions {
            file: plain.to_path_buf(),
            passphrase: Some(passphrase.into()),
            parts: Some(parts.clone()),
            baseline: Some(Baseline::default()),
            ..Default::default()
        },
    )?;
    Ok(plan["rows"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|r| r["state"] == restore::STATE_HERE)
        .filter_map(|r| {
            Some((
                r["id"].as_str()?.to_string(),
                r["hash"].as_str()?.to_string(),
            ))
        })
        .collect())
}

/// The records other machines published, from the file on the remote, so this push keeps them.
/// A file this passphrase does not open carries nothing: the other machines publish again.
fn carried_records(remote_file: &Path, pass: &str, scratch: &Scratch) -> Vec<(String, Value)> {
    let plain = scratch.file("carry.zip");
    if unseal(remote_file, pass, &plain).is_err() {
        return Vec::new();
    }
    inspect::open(&plain)
        .map(|mut opened| peers::records_of(&mut opened, &export::machine_name()))
        .unwrap_or_default()
}

/// `sync push`: seals this machine's parts as a whole and puts the file on the transport,
/// provided nobody else pushed since this machine last pulled.
pub fn push(ctx: &Ctx<'_>, opts: &PushOptions, git: &dyn GitRunner) -> Result<Value, OpError> {
    let cfg = require_config(ctx)?;
    if !cfg.enabled {
        return Err(failed(
            "sync is switched off: `toolportctl sync setup --enable`",
        ));
    }
    let mut state = config::read_state(ctx)?;
    if !state.pending.is_empty() && !opts.keep_mine {
        return Err(OpError::conflict(format!(
            "the last pull left rows both machines changed: {}. Settle them with `sync pull --take <row>`, or push with --keep-mine to keep this machine's",
            state.pending.join(", ")
        )));
    }
    let (parts, skipped) = effective_parts(ctx, &cfg)?;
    if parts.is_empty() {
        return Err(failed("no part is switched on for sync"));
    }
    let scratch = Scratch::new(ctx)?;
    let link = open_transport(ctx, &cfg, git);
    let remote_file = scratch.file("remote.tpsync");
    let remote = link.fetch(&remote_file).map_err(failed)?;
    if remote.is_some() && remote != state.remote_seen {
        return Err(moved_error());
    }
    let mut export_opts = Options {
        parts: parts.clone(),
        seal_file: true,
        dry_run: opts.dry_run,
        out: Some(scratch.file("push.tpsync")),
        ..Options::default()
    };
    export_opts.library_zip = false;
    if opts.dry_run {
        let mut data = export::run(ctx, &export_opts, None)?;
        data["skipped"] = json!(skipped);
        data["wouldReplace"] = json!(remote);
        data["file"] = json!(FILE_NAME);
        data["name"] = json!(FILE_NAME);
        return Ok(data);
    }
    let pass = passphrase_of(ctx, false)?;
    if parts.contains(&PartId::ServerRepos) && remote.is_some() {
        export_opts.peer_records = carried_records(&remote_file, &pass, &scratch);
    }
    let mut data = export::run(ctx, &export_opts, Some(&pass))?;
    let sealed = scratch.file("push.tpsync");
    let plain = scratch.file("push.zip");
    unseal(&sealed, &pass, &plain)?;
    let agreed = agreed_rows(ctx, &plain, &pass, &parts)?;
    let manifest = inspect::open(&plain)?.manifest;
    let revision = link
        .publish(&sealed, remote.as_deref(), "toolport sync")
        .map_err(|why| {
            if why == transport::MOVED {
                moved_error()
            } else {
                failed(why)
            }
        })?;
    let machine = export::machine_name();
    let at = now_text(ctx);
    record_machine(ctx, &mut state, &manifest);
    state.baseline = agreed;
    state.pending.clear();
    state.remote_seen = Some(revision.clone());
    state.last_apply = None;
    state.last_sync = Some(LastRun {
        direction: "push".into(),
        at: at.clone(),
        revision: revision.clone(),
        machine,
    });
    config::write_state(ctx, &state)?;
    data["pushed"] = json!(true);
    data["revision"] = json!(revision);
    data["skipped"] = json!(skipped);
    data["file"] = json!(FILE_NAME);
    data["result"] = json!({"undo": "put the previous version back from the remote's history"});
    if let Some(map) = data.as_object_mut() {
        map.remove("tree");
    }
    Ok(data)
}

#[derive(Clone, Debug, Default)]
pub struct PullOptions {
    pub take: BTreeSet<String>,
    /// Conflict rows settled as this machine's version: the baseline moves to the other
    /// machine's, so the row counts as changed only here and goes out with the next push.
    pub keep: BTreeSet<String>,
    pub apply: bool,
    pub plan: Option<String>,
    /// A scheduled check: plans only, skips quietly while the keyring is locked and does
    /// nothing when the remote is as this machine last saw it.
    pub unattended: bool,
    /// Plans without ever asking the person to unlock the keychain.
    pub no_prompt: bool,
}

fn skipped_run(ctx: &Ctx<'_>, reason: &str) -> Value {
    json!({"skipped": true, "reason": reason, "at": now_text(ctx), "applied": false})
}

fn advance(
    baseline: &mut BTreeMap<String, String>,
    data: &Value,
    taken: &BTreeSet<String>,
    kept: &BTreeSet<String>,
) {
    let steps = data["steps"].as_array().cloned().unwrap_or_default();
    let ok = |id: &str| steps.iter().any(|s| s["id"] == id && s["ok"] == true);
    let servers_ok = steps
        .iter()
        .any(|s| s["part"] == "servers" && s["ok"] == true);
    for row in data["rows"].as_array().into_iter().flatten() {
        let (Some(id), Some(hash)) = (row["id"].as_str(), row["hash"].as_str()) else {
            continue;
        };
        let settled = match row["state"].as_str().unwrap_or_default() {
            restore::STATE_HERE => true,
            restore::STATE_ADD if row["part"] == "servers" => servers_ok,
            restore::STATE_ADD | restore::STATE_INCOMING => ok(id),
            restore::STATE_CONFLICT => (taken.contains(id) && ok(id)) || kept.contains(id),
            _ => false,
        };
        if settled {
            baseline.insert(id.to_string(), hash.to_string());
        }
    }
}

/// `sync pull` (and `sync plan`): the file on the transport planned against this machine; with
/// `apply` and the plan hash it is applied the way a restore is, and the baseline moves for the
/// rows that were settled.
pub fn pull(ctx: &Ctx<'_>, opts: &PullOptions, git: &dyn GitRunner) -> Result<Value, OpError> {
    if (opts.unattended || opts.no_prompt) && opts.apply {
        return Err(OpError::usage("an unattended pull only plans"));
    }
    let quiet = opts.unattended || opts.no_prompt;
    let cfg = require_config(ctx)?;
    if !cfg.enabled {
        return Ok(skipped_run(ctx, "sync is switched off"));
    }
    if quiet && ctx.host.keyring_locked() {
        return Ok(skipped_run(ctx, "the keyring is locked: sync is paused"));
    }
    let mut state = config::read_state(ctx)?;
    let pass = match passphrase_of(ctx, quiet) {
        Ok(pass) => pass,
        Err(e) if quiet => return Ok(skipped_run(ctx, &e.message)),
        Err(e) => return Err(e),
    };
    let (parts, skipped) = effective_parts(ctx, &cfg)?;
    let scratch = Scratch::new(ctx)?;
    let link = open_transport(ctx, &cfg, git);
    let sealed = scratch.file("remote.tpsync");
    let Some(revision) = link.fetch(&sealed).map_err(failed)? else {
        return Ok(json!({
            "empty": true,
            "applied": false,
            "dryRun": true,
            "note": "nothing is on the remote yet: push from the machine that has the setup",
            "skipped": skipped,
            "rows": [],
        }));
    };
    if opts.unattended && state.remote_seen.as_deref() == Some(revision.as_str()) {
        state.last_check = Some(json!({"at": now_text(ctx), "revision": revision, "changes": 0}));
        config::write_state(ctx, &state)?;
        return Ok(json!({"upToDate": true, "revision": revision, "applied": false}));
    }
    let plain = scratch.file("remote.zip");
    unseal(&sealed, &pass, &plain)?;
    let mut opened = inspect::open(&plain)?;
    let manifest = opened.manifest.clone();
    let records = parts
        .contains(&PartId::ServerRepos)
        .then(|| peers::records_of(&mut opened, &export::machine_name()));
    drop(opened);
    let mut data = restore::run(
        ctx,
        &RestoreOptions {
            file: plain.clone(),
            passphrase: Some(pass),
            parts: Some(parts),
            take: opts.take.clone(),
            apply: opts.apply,
            plan: opts.plan.clone(),
            baseline: Some(state.baseline()),
        },
    )?;
    if let Some(records) = records {
        Peers {
            revision: Some(revision.clone()),
            seen_at: ctx.host.now_unix(),
            machines: peers::typed(&records),
        }
        .write(&ctx.data_dir)
        .map_err(failed)?;
    }
    data["revision"] = json!(revision);
    data["remote"] = json!({
        "machine": manifest["machine"],
        "madeAt": manifest["madeAt"],
    });
    data["skipped"] = json!(skipped);
    data["file"] = json!(FILE_NAME);
    record_machine(ctx, &mut state, &manifest);
    if !opts.apply {
        let summary = &data["summary"];
        let changes = ["add", "incoming", "conflict"]
            .iter()
            .map(|k| summary[*k].as_u64().unwrap_or(0))
            .sum::<u64>();
        state.last_check = Some(json!({
            "at": now_text(ctx),
            "revision": revision,
            "changes": changes,
            "summary": summary,
        }));
        config::write_state(ctx, &state)?;
        return Ok(data);
    }
    let before = state.baseline.clone();
    advance(&mut state.baseline, &data, &opts.take, &opts.keep);
    state.pending = data["rows"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|r| r["state"] == restore::STATE_CONFLICT)
        .filter_map(|r| r["id"].as_str().map(String::from))
        .filter(|id| {
            !opts.keep.contains(id)
                && !(opts.take.contains(id)
                    && data["steps"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .any(|s| s["id"] == id.as_str() && s["ok"] == true))
        })
        .collect();
    state.last_apply = Some(LastApply {
        run: data["run"].as_str().unwrap_or_default().to_string(),
        before,
        seen_before: state.remote_seen.clone(),
    });
    state.remote_seen = Some(revision.clone());
    state.last_sync = Some(LastRun {
        direction: "pull".into(),
        at: now_text(ctx),
        revision,
        machine: export::machine_name(),
    });
    config::write_state(ctx, &state)?;
    data["pending"] = json!(state.pending);
    Ok(data)
}

/// `sync undo`: reverses the last applied pull as `setup undo` does, and puts the baseline back
/// so the next pull offers the same changes again.
pub fn undo(ctx: &Ctx<'_>, dry_run: bool) -> Result<Value, OpError> {
    let mut state = config::read_state(ctx)?;
    let apply = state
        .last_apply
        .clone()
        .ok_or_else(|| OpError::not_found("no applied sync pull to undo"))?;
    let data = restore::undo(ctx, Some(&apply.run), dry_run)?;
    if !dry_run {
        state.baseline = apply.before;
        state.remote_seen = apply.seen_before;
        state.last_apply = None;
        config::write_state(ctx, &state)?;
    }
    Ok(data)
}
