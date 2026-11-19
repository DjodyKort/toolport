//! Moving a server to another version of the package or release it runs: a plan first, then the
//! one registry change (the pin in the arguments, or the version and file of a GitHub release),
//! a restart through the gateway, and an undo that puts back exactly that. Nothing here edits
//! anything but the pin, and a restart is a separate step the person can skip.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{json, Map, Value};
use toolport_plus_api::{Host, McpmSource, ServerRecord};

use super::journal::{Applied, ArgChange, FileChange, Journal};
use super::release::{self, Asset, Checksum, Platform};
use super::{lookup, recorded_docker, rewrite, version, Kind, Pinned, Rewrite};
use crate::attention::store::Store;
use crate::attention::updates::record_lookup;
use crate::hashing::lock_hash;
use crate::op::OpError;
use crate::servers::repo::screen::shown;
use crate::servers::repo::update::{self as repo_update, RestartChoice, UpdateArgs as RepoArgs};
use crate::servers::restart::{self, Timing};
use crate::servers::{self, find, read_servers, ServerError};
use crate::tasks::cron::rfc3339;

const VERIFY_TIMEOUT: Duration = Duration::from_secs(10);

pub struct UpdateArgs {
    pub key: String,
    /// The version to move to; without it the newest one the last check found.
    pub to: Option<String>,
    pub dry_run: bool,
    /// Ask the registry first, so the plan sees what was published since the last check.
    pub fetch: bool,
    /// The hash of the plan the person reviewed; the update stops when the plan has changed.
    pub plan_hash: Option<String>,
    /// Install a release file the publisher gives no checksum for.
    pub allow_unverified: bool,
    pub restart: RestartChoice,
    pub timing: Timing,
    pub restart_wait: Duration,
    pub platform: Platform,
}

impl UpdateArgs {
    pub fn new(key: &str) -> Self {
        Self {
            key: key.to_string(),
            to: None,
            dry_run: false,
            fetch: false,
            plan_hash: None,
            allow_unverified: false,
            restart: RestartChoice::IfIdle,
            timing: Timing::default(),
            restart_wait: restart::DEFAULT_WAIT,
            platform: Platform::current(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refusal {
    pub code: &'static str,
    pub message: String,
}

/// One field of the registry entry that the update writes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub field: String,
    pub from: String,
    pub to: String,
}

impl Change {
    fn to_value(&self) -> Value {
        json!({"field": self.field, "from": self.from, "to": self.to})
    }
}

#[derive(Clone, Debug)]
pub struct ReleasePlan {
    pub tag: String,
    pub asset: Asset,
    /// `None` when the release publishes no checksum for the file.
    pub checksum: Option<Checksum>,
    pub path: PathBuf,
    pub binary: String,
    pub verify: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Plan {
    pub server_id: String,
    pub server_name: String,
    pub kind: Kind,
    pub package: String,
    pub from: String,
    pub to: String,
    pub downgrade: bool,
    /// The registry was asked in this plan whether the version exists.
    pub verified: bool,
    pub changes: Vec<Change>,
    pub arg: Option<Rewrite>,
    /// The tag the server's source records for its image, which moves with the pin.
    pub source_tag: Option<String>,
    pub release: Option<ReleasePlan>,
    pub refusal: Option<Refusal>,
    /// `None` when no gateway answered, so nothing is running that a restart could interrupt.
    pub in_flight: Option<usize>,
    pub hash: String,
}

fn base_of(host: &dyn Host) -> Result<PathBuf, OpError> {
    host.data_dir()
        .ok_or_else(|| OpError::failed("no_data_dir", "the data directory could not be resolved"))
}

fn journal_error(error: String) -> OpError {
    OpError::failed("server_update_store", error)
}

fn lookup_failed(error: String) -> OpError {
    OpError::failed("lookup_failed", error)
}

fn undo_line(name: &str) -> String {
    format!("toolportctl server update {name} --undo")
}

/// The version text a release source records: with the `v` of the tag only when the source
/// already spelled its versions that way.
fn recorded_version(old: &str, tag: &str) -> String {
    let bare = tag.trim_start_matches(['v', 'V']);
    if old.starts_with(['v', 'V']) {
        format!("v{bare}")
    } else {
        bare.to_string()
    }
}

fn pin_of(servers: &[ServerRecord], key: &str) -> Result<(ServerRecord, Pinned), OpError> {
    let server = find(servers, key)
        .ok_or_else(|| OpError::not_found(format!("no server '{key}'")))?
        .clone();
    let pin = super::pinned(&server).ok_or_else(|| {
        OpError::not_found(format!(
            "{} pins no package version and was not installed from a GitHub release",
            server.name
        ))
    })?;
    Ok((server, pin))
}

fn refuse(code: &'static str, message: impl Into<String>) -> Option<Refusal> {
    Some(Refusal {
        code,
        message: message.into(),
    })
}

struct Wanted {
    to: String,
    explicit: bool,
    verified: bool,
    refusal: Option<Refusal>,
}

fn wanted(
    host: &dyn Host,
    base: &Path,
    pin: &Pinned,
    args: &UpdateArgs,
) -> Result<Wanted, OpError> {
    let target = &pin.target;
    let mut verified = false;
    let explicit = args
        .to
        .as_deref()
        .map(str::trim)
        .filter(|to| !to.is_empty());
    if let Some(to) = explicit {
        if !version::exact(to) {
            return Err(OpError::usage(format!(
                "'{to}' is not one version; give an exact one such as 1.4.2"
            )));
        }
        if args.fetch {
            match lookup::exists(host, target, to).map_err(lookup_failed)? {
                true => verified = true,
                false => {
                    return Ok(Wanted {
                        to: to.to_string(),
                        explicit: true,
                        verified: false,
                        refusal: refuse(
                            "unknown_version",
                            format!("{} has no version {to}", target.name),
                        ),
                    })
                }
            }
        }
        return Ok(Wanted {
            to: to.to_string(),
            explicit: true,
            verified,
            refusal: None,
        });
    }
    let latest = if args.fetch {
        let found = lookup::latest(host, target);
        record_lookup(base, host.now_unix(), target, &found).map_err(journal_error)?;
        verified = found.is_ok();
        found.map_err(lookup_failed)?
    } else {
        Store::read(base)
            .map_err(journal_error)?
            .updates
            .sources
            .get(&target.source())
            .and_then(|check| check.latest.clone())
    };
    Ok(match latest {
        Some(to) => Wanted {
            to,
            explicit: false,
            verified,
            refusal: None,
        },
        None => Wanted {
            to: String::new(),
            explicit: false,
            verified,
            refusal: refuse(
                "not_checked",
                if args.fetch {
                    format!("{} is not published under that name", target.name)
                } else {
                    format!(
                        "the last update check found no version of {}; run the check, pass --fetch to ask the registry now, or give --to",
                        target.name
                    )
                },
            ),
        },
    })
}

fn plan_hash(plan: &Plan) -> String {
    let (index, before, after) = plan
        .arg
        .as_ref()
        .map(|rw| (rw.index.to_string(), rw.before.as_str(), rw.after.as_str()))
        .unwrap_or_default();
    let (asset, digest) = plan
        .release
        .as_ref()
        .map(|r| {
            (
                r.asset.name.as_str(),
                r.checksum.as_ref().map_or("", |c| c.sha256.as_str()),
            )
        })
        .unwrap_or_default();
    lock_hash(format!(
        "{}\n{}\n{}\n{}\n{index}\n{before}\n{after}\n{asset}\n{digest}",
        plan.server_id,
        plan.kind.word(),
        plan.from,
        plan.to
    ))
}

/// What an update would do right now.
pub fn plan(host: &dyn Host, args: &UpdateArgs, ask_gateways: bool) -> Result<Plan, OpError> {
    let servers = read_servers(host)?;
    let (server, pin) = pin_of(&servers, &args.key)?;
    let base = base_of(host)?;
    let want = wanted(host, &base, &pin, args)?;
    let mut plan = Plan {
        server_id: server.id.clone(),
        server_name: server.name.clone(),
        kind: pin.target.kind,
        package: pin.target.name.clone(),
        from: pin.version.clone(),
        to: want.to.clone(),
        downgrade: want.explicit && version::newer(&pin.version, &want.to),
        verified: want.verified,
        changes: Vec::new(),
        arg: None,
        source_tag: None,
        release: None,
        refusal: want.refusal,
        in_flight: None,
        hash: String::new(),
    };
    if plan.refusal.is_none() {
        let same = !version::newer(&plan.to, &plan.from) && !version::newer(&plan.from, &plan.to);
        if same || (!want.explicit && !version::newer(&plan.to, &plan.from)) {
            plan.refusal = refuse(
                "up_to_date",
                format!("{} is already on {}", plan.package, plan.from),
            );
        }
    }
    if plan.refusal.is_none() {
        match pin.target.kind {
            Kind::Github => plan_release(host, &server, &pin, args, &mut plan)?,
            _ => plan_pin(&server, &pin, &mut plan),
        }
    }
    if plan.refusal.is_none() && ask_gateways {
        plan.in_flight =
            restart::calls_in_flight(host, &server.id, args.timing, args.restart_wait)?;
    }
    plan.hash = plan_hash(&plan);
    Ok(plan)
}

fn plan_pin(server: &ServerRecord, pin: &Pinned, plan: &mut Plan) {
    match rewrite(server, &plan.to) {
        Err(reason) => {
            plan.refusal = refuse(
                "cannot_rewrite",
                format!("Toolport cannot change this pin itself: {reason}"),
            )
        }
        Ok(rw) => {
            plan.changes.push(Change {
                field: format!("args[{}]", rw.index),
                from: rw.before.clone(),
                to: rw.after.clone(),
            });
            if recorded_docker(server).is_some() {
                plan.source_tag = Some(plan.to.clone());
                plan.changes.push(Change {
                    field: "mcpmSource.tag".into(),
                    from: pin.version.clone(),
                    to: plan.to.clone(),
                });
            }
            plan.arg = Some(rw);
        }
    }
}

fn plan_release(
    host: &dyn Host,
    server: &ServerRecord,
    pin: &Pinned,
    args: &UpdateArgs,
    plan: &mut Plan,
) -> Result<(), OpError> {
    let method = match release::method(server) {
        Ok(method) => method,
        Err(reason) => {
            plan.refusal = refuse(
                "cannot_rewrite",
                format!("Toolport cannot update this install itself: {reason}"),
            );
            return Ok(());
        }
    };
    let Some(found) =
        release::release_for(host, &pin.target.name, &plan.to).map_err(lookup_failed)?
    else {
        plan.refusal = refuse(
            "unknown_version",
            format!("{} has no release {}", pin.target.name, plan.to),
        );
        return Ok(());
    };
    plan.verified = true;
    let asset = match release::pick_asset(&found, &method.pattern, args.platform) {
        Ok(asset) => asset.clone(),
        Err(message) => {
            plan.refusal = refuse("no_asset", message);
            return Ok(());
        }
    };
    if asset.size > release::MAX_ASSET_BYTES {
        plan.refusal = refuse(
            "too_large",
            format!(
                "{} is {} MB; Toolport takes files up to {} MB",
                asset.name,
                asset.size / 1_000_000,
                release::MAX_ASSET_BYTES / 1_000_000
            ),
        );
        return Ok(());
    }
    let checksum = release::checksum_for(host, &found, &asset).map_err(lookup_failed)?;
    let after = recorded_version(&pin.version, &found.tag);
    plan.to = found.tag.clone();
    plan.changes.push(Change {
        field: "mcpmSource.current_version".into(),
        from: pin.version.clone(),
        to: after,
    });
    plan.changes.push(Change {
        field: format!("file {}", method.path.display()),
        from: "the installed binary".into(),
        to: asset.name.clone(),
    });
    plan.release = Some(ReleasePlan {
        tag: found.tag,
        asset,
        checksum,
        path: method.path,
        binary: method.binary,
        verify: method.verify,
    });
    Ok(())
}

impl Plan {
    pub fn to_value(&self) -> Value {
        let apply = format!(
            "toolportctl server update {} --to {} --plan-hash {}",
            self.server_name, self.to, self.hash
        );
        json!({
            "server": self.server_name,
            "id": self.server_id,
            "kind": self.kind.word(),
            "package": self.package,
            "from": self.from,
            "to": self.to,
            "downgrade": self.downgrade,
            "verified": self.verified,
            "changes": self.changes.iter().map(Change::to_value).collect::<Vec<_>>(),
            "release": self.release.as_ref().map(|r| json!({
                "tag": r.tag,
                "asset": {"name": r.asset.name, "size": r.asset.size},
                "checksum": r.checksum.as_ref().map(|c| json!({"source": c.source})),
                "file": r.path.to_string_lossy(),
                "verifyCommand": r.verify.is_some(),
            })),
            "refusal": self.refusal.as_ref().map(|r| json!({"code": r.code, "message": r.message})),
            "inFlight": self.in_flight,
            "planHash": self.hash,
            "apply": self.refusal.is_none().then_some(apply),
            "undo": undo_line(&self.server_name),
        })
    }

    pub fn human(&self) -> String {
        let mut out = format!(
            "{} runs {} {} ({})\n",
            self.server_name,
            self.package,
            self.from,
            self.kind.word()
        );
        if let Some(refusal) = &self.refusal {
            out.push_str(&format!("Nothing to apply: {}.\n", refusal.message));
            return out;
        }
        out.push_str(&format!(
            "{} {} to {}{}\n",
            if self.downgrade {
                "Go back from"
            } else {
                "Update"
            },
            self.from,
            self.to,
            if self.verified {
                ""
            } else {
                " (the registry was not asked whether it exists; add --fetch)"
            }
        ));
        out.push_str("The registry entry changes:\n");
        for change in &self.changes {
            out.push_str(&format!(
                "  {}: {} -> {}\n",
                change.field, change.from, change.to
            ));
        }
        if let Some(release) = &self.release {
            match &release.checksum {
                Some(checksum) => out.push_str(&format!(
                    "The file is checked against {} (sha256) before it replaces the binary.\n",
                    checksum.source
                )),
                None => out.push_str(
                    "The release publishes no checksum for this file, so it cannot be verified; pass --allow-unverified to install it anyway.\n",
                ),
            }
            out.push_str(match release.verify {
                Some(_) => "The new binary must pass the source's verify command, or the previous one is put back.\n",
                None => "The new binary must be an executable file, or the previous one is put back.\n",
            });
        }
        match self.in_flight {
            Some(0) => out.push_str("Then restart it; no call is running.\n"),
            Some(calls) => out.push_str(&format!(
                "{calls} call(s) are running; a restart waits unless you pass --restart-now.\n"
            )),
            None => out.push_str("Then restart it through the gateway if one is running.\n"),
        }
        out.push_str(&format!(
            "Apply with: toolportctl server update {} --to {} --plan-hash {}\n",
            self.server_name, self.to, self.hash
        ));
        out
    }
}

#[derive(Clone, Debug)]
pub struct Done {
    pub server_id: String,
    pub server_name: String,
    pub kind: Kind,
    pub package: String,
    pub from: String,
    pub to: String,
    pub changed: bool,
    pub changes: Vec<Change>,
    pub restart: Option<(&'static str, String)>,
    pub warnings: Vec<String>,
    pub undo: String,
}

impl Done {
    pub fn failed(&self) -> bool {
        self.restart.as_ref().is_some_and(|(s, _)| *s == "failed")
    }

    pub fn to_value(&self) -> Value {
        json!({
            "applied": self.changed,
            "server": self.server_name,
            "id": self.server_id,
            "kind": self.kind.word(),
            "package": self.package,
            "from": self.from,
            "to": self.to,
            "changes": self.changes.iter().map(Change::to_value).collect::<Vec<_>>(),
            "restart": self.restart.as_ref().map(|(status, line)| json!({"status": status, "detail": line})),
            "warnings": self.warnings,
            "undo": self.undo,
        })
    }

    pub fn human(&self, verb: &str) -> String {
        let mut out = if self.changed {
            format!(
                "{verb} {}: {} {} to {}.\n",
                self.server_name, self.package, self.from, self.to
            )
        } else {
            format!("{} is already on {}.\n", self.server_name, self.from)
        };
        if let Some((status, detail)) = &self.restart {
            out.push_str(&format!("Restart ({status}): {detail}\n"));
        }
        for warning in &self.warnings {
            out.push_str(&format!("Warning: {warning}\n"));
        }
        if self.changed {
            out.push_str(&format!("To go back: {}\n", self.undo));
        }
        out
    }
}

fn restart_after(
    host: &dyn Host,
    args: &UpdateArgs,
    server_id: &str,
) -> Option<(&'static str, String)> {
    let mut ask = RepoArgs::new(&args.key);
    ask.restart = args.restart;
    ask.restart_wait = args.restart_wait;
    ask.timing = args.timing;
    repo_update::restart_after(host, &ask, server_id)
}

fn server_error(error: ServerError) -> OpError {
    match error {
        ServerError::NotFound(message) => OpError::not_found(message),
        ServerError::Exists => OpError::conflict("a server with that name exists"),
        ServerError::Refused(refusal) => OpError::failed("update_failed", refusal.message),
        ServerError::Rejected(message) => OpError::conflict(message),
    }
}

fn source_of(server: &ServerRecord) -> McpmSource {
    server.fork.mcpm_source.clone().unwrap_or_default()
}

fn merge(host: &dyn Host, server_id: &str, key: &str, value: Value) -> Result<(), String> {
    let mut fields = Map::new();
    fields.insert(key.into(), value);
    host.merge_server_mcpm_source(server_id, fields)
}

/// Writes the update into the registry (and puts the new binary in place), records it for undo
/// and restarts the server.
pub fn apply(host: &dyn Host, args: &UpdateArgs) -> Result<Done, OpError> {
    let plan = plan(host, args, false)?;
    let done = |changed: bool, warnings: Vec<String>, restart| Done {
        server_id: plan.server_id.clone(),
        server_name: plan.server_name.clone(),
        kind: plan.kind,
        package: plan.package.clone(),
        from: plan.from.clone(),
        to: plan.to.clone(),
        changed,
        changes: plan.changes.clone(),
        restart,
        warnings,
        undo: undo_line(&plan.server_name),
    };
    if let Some(refusal) = &plan.refusal {
        if refusal.code == "up_to_date" {
            return Ok(done(false, Vec::new(), None));
        }
        return Err(OpError::failed("update_refused", refusal.message.clone())
            .with_details(json!({"reason": refusal.code})));
    }
    if args.plan_hash.as_deref().is_some_and(|h| h != plan.hash) {
        return Err(OpError::conflict(
            "the plan has changed since it was reviewed; review it again",
        )
        .with_details(json!({"planHash": plan.hash})));
    }
    let base = base_of(host)?;
    let servers = read_servers(host)?;
    let (server, _) = pin_of(&servers, &args.key)?;
    let before = source_of(&server);
    let mut warnings = Vec::new();
    let mut applied = Applied {
        kind: plan.kind.word().into(),
        package: plan.package.clone(),
        from: plan.from.clone(),
        to: plan.to.clone(),
        at: host.now_unix(),
        arg: None,
        source_before: None,
        source_after: None,
        file: None,
    };
    if let Some(rw) = &plan.arg {
        servers::replace_arg(host, &plan.server_id, rw.index, &rw.before, &rw.after)
            .map_err(server_error)?;
        applied.arg = Some(ArgChange {
            index: rw.index,
            before: rw.before.clone(),
            after: rw.after.clone(),
        });
        if let Some(tag) = &plan.source_tag {
            match merge(host, &plan.server_id, "tag", json!(tag)) {
                Ok(()) => {
                    applied.source_before = Some(before.clone());
                    applied.source_after = Some(tag.clone());
                }
                Err(error) => warnings.push(format!(
                    "the arguments were changed, but the image tag the source records could not be: {error}"
                )),
            }
        }
    }
    if let Some(release) = &plan.release {
        install_release(host, &base, &server, &plan, release, args, &mut applied)?;
        applied.source_before = Some(before);
    }
    record(host, &base, &plan.server_id, applied, &mut warnings);
    let restart = restart_after(host, args, &plan.server_id);
    Ok(done(true, warnings, restart))
}

fn record(
    host: &dyn Host,
    base: &Path,
    server_id: &str,
    applied: Applied,
    warnings: &mut Vec<String>,
) {
    let written = Journal::read(base).and_then(|mut journal| {
        if let Some(old) = journal.applied.insert(server_id.to_string(), applied) {
            if let Some(file) = old.file {
                let _ = std::fs::remove_file(file.backup);
            }
        }
        journal.write(base)
    });
    if let Err(error) = written {
        warnings.push(shown(
            host,
            &format!("the update was applied but could not be recorded for undo: {error}"),
        ));
    }
}

fn install_release(
    host: &dyn Host,
    base: &Path,
    server: &ServerRecord,
    plan: &Plan,
    release: &ReleasePlan,
    args: &UpdateArgs,
    applied: &mut Applied,
) -> Result<(), OpError> {
    let failed = |message: String| OpError::failed("update_failed", shown(host, &message));
    if release.checksum.is_none() && !args.allow_unverified {
        return Err(OpError::failed(
            "update_refused",
            "the release publishes no checksum for this file; pass --allow-unverified to install it anyway",
        )
        .with_details(json!({"reason": "unverified"})));
    }
    let bytes = release::download(host, &release.asset).map_err(failed)?;
    if let Some(checksum) = &release.checksum {
        if release::sha256_hex(&bytes) != checksum.sha256 {
            return Err(failed(format!(
                "{} does not match the checksum in {}; nothing was changed",
                release.asset.name, checksum.source
            )));
        }
    }
    let kept = super::journal::kept_dir(base, &plan.server_id);
    let work = kept.join(format!("unpack-{}", std::process::id()));
    let unpacked = release::binary_from(&release.asset.name, &bytes, &release.binary, &work);
    let _ = std::fs::remove_dir_all(&work);
    let binary = unpacked.map_err(|e| failed(format!("{e}; nothing was changed")))?;
    let backup = kept.join("previous");
    release::replace_file(&release.path, &binary, &backup)
        .map_err(|e| failed(format!("{e}; nothing was changed")))?;
    let checked = release::runnable(&release.path).and_then(|()| match &release.verify {
        Some(command) => {
            let dir = release.path.parent().unwrap_or(Path::new("."));
            let run = repo_update::run_build(host, dir, command, VERIFY_TIMEOUT);
            if run.ok {
                Ok(())
            } else {
                Err(format!("{} failed: {}", run.command, run.output))
            }
        }
        None => Ok(()),
    });
    if let Err(reason) = checked {
        let back = release::restore_file(&release.path, &backup);
        let _ = std::fs::remove_file(&backup);
        return Err(failed(match back {
            Ok(()) => format!(
                "the new binary did not pass its check ({reason}); the previous one is back and nothing else changed"
            ),
            Err(error) => format!(
                "the new binary did not pass its check ({reason}) and the previous one could not be put back: {error}"
            ),
        })
        .with_details(json!({"putBack": true})));
    }
    let old = server
        .fork
        .mcpm_source
        .as_ref()
        .and_then(|source| source.extra.get("current_version"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let new = recorded_version(&old, &release.tag);
    let mut fields = Map::new();
    fields.insert("current_version".into(), json!(new));
    fields.insert("last_updated".into(), json!(rfc3339(host.now_unix())));
    host.merge_server_mcpm_source(&plan.server_id, fields)
        .map_err(|error| {
            let _ = release::restore_file(&release.path, &backup);
            failed(format!(
                "the version could not be recorded ({error}); the previous binary is back"
            ))
        })?;
    applied.source_after = Some(new);
    applied.file = Some(FileChange {
        path: release.path.to_string_lossy().into_owned(),
        backup: backup.to_string_lossy().into_owned(),
        installed_sha256: release::sha256_hex(&binary),
    });
    Ok(())
}

#[derive(Clone, Debug)]
pub struct UndoPlan {
    pub server_id: String,
    pub server_name: String,
    pub applied: Applied,
    pub refusal: Option<Refusal>,
}

impl UndoPlan {
    pub fn to_value(&self) -> Value {
        json!({
            "server": self.server_name,
            "id": self.server_id,
            "kind": self.applied.kind,
            "package": self.applied.package,
            "from": self.applied.to,
            "to": self.applied.from,
            "refusal": self.refusal.as_ref().map(|r| json!({"code": r.code, "message": r.message})),
        })
    }

    pub fn human(&self) -> String {
        match &self.refusal {
            Some(refusal) => format!("Cannot go back: {}.\n", refusal.message),
            None => format!(
                "Would put {} back from {} to {}, then restart it.\n",
                self.server_name, self.applied.to, self.applied.from
            ),
        }
    }
}

fn source_value(server: &ServerRecord, kind: &str) -> Option<String> {
    let key = if kind == Kind::Github.word() {
        "current_version"
    } else {
        "tag"
    };
    server
        .fork
        .mcpm_source
        .as_ref()?
        .extra
        .get(key)?
        .as_str()
        .map(String::from)
}

/// Whether the one update on record can be taken back: what it wrote must still be what is there.
pub fn undo_plan(host: &dyn Host, key: &str) -> Result<UndoPlan, OpError> {
    let servers = read_servers(host)?;
    let server = find(&servers, key)
        .ok_or_else(|| OpError::not_found(format!("no server '{key}'")))?
        .clone();
    let journal = Journal::read(&base_of(host)?).map_err(journal_error)?;
    let applied =
        journal.applied.get(&server.id).cloned().ok_or_else(|| {
            OpError::not_found(format!("no update of {} to take back", server.name))
        })?;
    let args = server.strings("args");
    let moved = |what: &str| {
        refuse(
            "moved",
            format!("{what} has changed since the update, so nothing was changed"),
        )
    };
    let refusal = if applied
        .arg
        .as_ref()
        .is_some_and(|arg| args.get(arg.index) != Some(&arg.after))
    {
        moved("the server's argument")
    } else if applied.source_after.is_some()
        && source_value(&server, &applied.kind) != applied.source_after
    {
        moved("the server's source")
    } else if let Some(file) = &applied.file {
        let current = std::fs::read(&file.path).map(|bytes| release::sha256_hex(&bytes));
        if !Path::new(&file.backup).is_file() {
            refuse("gone", "the previous binary is no longer kept")
        } else if current.as_deref().ok() != Some(file.installed_sha256.as_str()) {
            moved(&file.path)
        } else {
            None
        }
    } else {
        None
    };
    Ok(UndoPlan {
        server_id: server.id,
        server_name: server.name,
        applied,
        refusal,
    })
}

/// Puts back what the one update on record changed, and restarts the server.
pub fn undo(host: &dyn Host, args: &UpdateArgs) -> Result<Done, OpError> {
    let plan = undo_plan(host, &args.key)?;
    if let Some(refusal) = &plan.refusal {
        return Err(OpError::failed("undo_refused", refusal.message.clone())
            .with_details(json!({"reason": refusal.code})));
    }
    let base = base_of(host)?;
    let applied = plan.applied.clone();
    let mut warnings = Vec::new();
    if let Some(arg) = &applied.arg {
        servers::replace_arg(host, &plan.server_id, arg.index, &arg.after, &arg.before)
            .map_err(server_error)?;
    }
    if let Some(file) = &applied.file {
        release::restore_file(Path::new(&file.path), Path::new(&file.backup)).map_err(|e| {
            OpError::failed(
                "undo_failed",
                shown(host, &format!("{e}; the file was not put back")),
            )
        })?;
    }
    if let Some(source) = &applied.source_before {
        if let Err(error) = host.set_server_mcpm_source(&plan.server_id, Some(source.clone())) {
            warnings.push(shown(
                host,
                &format!("the server's source could not be put back: {error}"),
            ));
        }
    }
    let forgotten = Journal::read(&base).and_then(|mut journal| {
        journal.applied.remove(&plan.server_id);
        journal.write(&base)
    });
    if let Err(error) = forgotten {
        warnings.push(shown(
            host,
            &format!("the undo record could not be cleared: {error}"),
        ));
    }
    if let Some(file) = &applied.file {
        let _ = std::fs::remove_file(&file.backup);
    }
    let restart = restart_after(host, args, &plan.server_id);
    Ok(Done {
        server_id: plan.server_id,
        server_name: plan.server_name.clone(),
        kind: Kind::from_word(&applied.kind).unwrap_or(Kind::Npm),
        package: applied.package,
        from: applied.to,
        to: applied.from,
        changed: true,
        changes: Vec::new(),
        restart,
        warnings,
        undo: format!("toolportctl server update {}", plan.server_name),
    })
}
