//! Update checks and apply for registry servers (MIG-UPD-1): git ff-only,
//! GitHub release binaries with checksum + atomic replace, npx/uvx pins.
//! Commands that come from configuration (`post_update`, `verify_command`)
//! only run when the caller passes `allow_commands`.

pub mod exec;
pub mod gitops;
pub mod net;
pub mod pins;
pub mod release;
pub mod source;
#[cfg(test)]
mod tests;

use crate::registry::{self, ServerEntry};
use exec::{GitRunner, ShellRunner};
use net::HttpClient;
use serde::Serialize;
use serde_json::{json, Map, Value};
use source::Source;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

pub const GITHUB_API: &str = "https://api.github.com";
pub const NPM_REGISTRY: &str = "https://registry.npmjs.org";
pub const PYPI_INDEX: &str = "https://pypi.org";
const POST_UPDATE_TIMEOUT: Duration = Duration::from_secs(120);

pub struct Env {
    pub git: Box<dyn GitRunner>,
    pub shell: Box<dyn ShellRunner>,
    pub http: Box<dyn HttpClient>,
    pub github_api: String,
    pub npm_registry: String,
    pub pypi_index: String,
    pub platform: (String, String),
    pub home: Option<PathBuf>,
    pub github_token: Option<String>,
    pub now: fn() -> String,
}

impl Env {
    pub fn system() -> Self {
        Env {
            git: Box::new(exec::SystemGit),
            shell: Box::new(exec::SystemShell),
            http: Box::new(net::UreqHttp),
            github_api: GITHUB_API.into(),
            npm_registry: NPM_REGISTRY.into(),
            pypi_index: PYPI_INDEX.into(),
            platform: release::platform(),
            home: dirs::home_dir(),
            github_token: std::env::var("GITHUB_TOKEN").ok().filter(|t| !t.is_empty()),
            now: now_iso,
        }
    }
}

pub fn now_iso() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    Check,
    DryRun,
    Apply,
    Init,
}

#[derive(Debug, Clone)]
pub struct Options {
    pub mode: Mode,
    pub server: Option<String>,
    pub allow_commands: bool,
    pub allow_unverified: bool,
    pub force: bool,
    pub dry_run: bool,
    pub repo: Option<String>,
}

impl Options {
    pub fn new(mode: Mode) -> Self {
        Self {
            mode,
            server: None,
            allow_commands: false,
            allow_unverified: false,
            force: false,
            dry_run: false,
            repo: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    UpToDate,
    UpdateAvailable,
    Updated,
    Skipped,
    Error,
    Auto,
    Configured,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerReport {
    pub id: String,
    pub kind: String,
    pub status: Status,
    pub message: String,
    pub detected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub behind: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ahead: Option<u32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub plan: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<Step>,
    #[serde(skip)]
    pub changed: bool,
}

impl ServerReport {
    fn new(id: &str, kind: &str, status: Status, message: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            kind: kind.into(),
            status,
            message: message.into(),
            detected: false,
            current: None,
            latest: None,
            behind: None,
            ahead: None,
            plan: Vec::new(),
            steps: Vec::new(),
            changed: false,
        }
    }

    fn set(&mut self, status: Status, message: impl Into<String>) {
        self.status = status;
        self.message = message.into();
    }

    fn step(&mut self, name: &str, ok: bool, detail: impl Into<String>) {
        self.steps.push(Step {
            name: name.into(),
            ok,
            detail: detail.into(),
        });
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub mode: Mode,
    pub servers: Vec<ServerReport>,
    pub counts: BTreeMap<String, usize>,
}

impl Report {
    fn new(mode: Mode, servers: Vec<ServerReport>) -> Self {
        let mut counts = BTreeMap::new();
        for s in &servers {
            let key = serde_json::to_value(s.status)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_default();
            *counts.entry(key).or_insert(0) += 1;
        }
        Report {
            mode,
            servers,
            counts,
        }
    }

    pub fn count(&self, key: &str) -> usize {
        self.counts.get(key).copied().unwrap_or(0)
    }

    pub fn has_errors(&self) -> bool {
        self.count("error") > 0
    }

    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }

    pub fn render(&self) -> String {
        if self.servers.is_empty() {
            return "No servers.".into();
        }
        let mut lines: Vec<String> = Vec::new();
        for s in &self.servers {
            lines.push(format!("  {:<22} {:<15} {}", s.id, s.kind, s.message));
            for p in &s.plan {
                lines.push(format!("      plan: {p}"));
            }
            for st in &s.steps {
                let mark = if st.ok { "ok" } else { "!!" };
                lines.push(format!("      [{mark}] {}: {}", st.name, st.detail));
            }
        }
        let mut summary: Vec<String> = self
            .counts
            .iter()
            .map(|(k, v)| format!("{v} {k}"))
            .collect();
        summary.sort();
        lines.push(String::new());
        lines.push(format!("{}: {}", mode_label(self.mode), summary.join(", ")));
        lines.join("\n")
    }
}

fn mode_label(mode: Mode) -> &'static str {
    match mode {
        Mode::Check => "check",
        Mode::DryRun => "dry run",
        Mode::Apply => "apply",
        Mode::Init => "init",
    }
}

fn describe_post_update(cmd: &str, allowed: bool) -> String {
    if allowed {
        format!("post_update: {cmd}")
    } else {
        format!("post_update: {cmd} (not run without --allow-commands)")
    }
}

fn check_git(
    env: &Env,
    opts: &Options,
    entry: &mut ServerEntry,
    src: &Source,
    rep: &mut ServerReport,
) {
    let Source::Git {
        path,
        branch,
        post_update,
        ..
    } = src
    else {
        return;
    };
    let repo = source::expand_path(path, env.home.as_deref());
    let status = match gitops::check(env.git.as_ref(), &repo, branch.as_deref()) {
        Ok(s) => s,
        Err(e) => return rep.set(Status::Error, e),
    };
    rep.behind = Some(status.behind);
    rep.ahead = Some(status.ahead);
    rep.current = Some(status.branch.clone());
    rep.latest = Some(status.remote_ref.clone());
    if status.behind == 0 {
        return rep.set(
            Status::UpToDate,
            format!("up to date with {}", status.remote_ref),
        );
    }
    if status.dirty {
        return rep.set(Status::Skipped, "uncommitted changes");
    }
    if status.ahead > 0 {
        return rep.set(
            Status::Skipped,
            "cannot fast-forward: local and remote have diverged",
        );
    }
    let message = format!("{} commit(s) behind {}", status.behind, status.remote_ref);
    rep.plan
        .push(format!("git merge --ff-only {}", status.remote_ref));
    if let Some(cmd) = post_update {
        rep.plan
            .push(describe_post_update(cmd, opts.allow_commands));
    }
    rep.set(Status::UpdateAvailable, message);
    if opts.mode != Mode::Apply {
        return;
    }
    if let Err(e) = gitops::fast_forward(env.git.as_ref(), &repo, &status.remote_ref) {
        rep.step("git merge --ff-only", false, e.clone());
        return rep.set(Status::Skipped, e);
    }
    rep.step(
        "git merge --ff-only",
        true,
        format!("{} new commit(s)", status.behind),
    );
    rep.changed = true;
    let mut fields = Map::new();
    fields.insert("last_updated".into(), json!((env.now)()));
    source::set_meta_fields(entry, fields);
    let mut message = format!("updated ({} new commit(s))", status.behind);
    if let Some(cmd) = post_update {
        if opts.allow_commands {
            match env.shell.run(cmd, &repo, POST_UPDATE_TIMEOUT) {
                Ok(out) if out.ok() => rep.step("post_update", true, cmd.clone()),
                Ok(out) => {
                    rep.step(
                        "post_update",
                        false,
                        format!(
                            "`{cmd}` failed (exit {}): {}",
                            out.code,
                            out.first_error_line()
                        ),
                    );
                    rep.set(
                        Status::Error,
                        format!(
                            "git updated but post_update failed; run manually: cd {path} && {cmd}"
                        ),
                    );
                    return;
                }
                Err(e) => {
                    rep.step("post_update", false, format!("`{cmd}` failed: {e}"));
                    rep.set(
                        Status::Error,
                        format!(
                            "git updated but post_update failed; run manually: cd {path} && {cmd}"
                        ),
                    );
                    return;
                }
            }
        } else {
            rep.step(
                "post_update",
                false,
                format!("not run: pass --allow-commands to run `{cmd}`"),
            );
            message.push_str("; post_update not run");
        }
    }
    rep.set(Status::Updated, message);
}

fn check_release(
    env: &Env,
    opts: &Options,
    entry: &mut ServerEntry,
    src: &Source,
    rep: &mut ServerReport,
) {
    let Source::GithubRelease {
        path,
        repo,
        current_version,
        asset_pattern,
        verify_command,
    } = src
    else {
        return;
    };
    let Some(repo) = repo else {
        return rep.set(
            Status::Skipped,
            "no repo configured; run update --init --repo owner/repo",
        );
    };
    rep.current = current_version.clone();
    let checked = match release::check(
        env.http.as_ref(),
        &env.github_api,
        env.github_token.as_deref(),
        repo,
        current_version.as_deref(),
        asset_pattern.as_deref(),
        &env.platform,
    ) {
        Ok(c) => c,
        Err(e) => return rep.set(Status::Error, e),
    };
    let found = match checked {
        release::Checked::UpToDate { latest } => {
            rep.latest = Some(latest.clone());
            return rep.set(Status::UpToDate, format!("up to date ({latest})"));
        }
        release::Checked::Available(c) => c,
    };
    rep.latest = Some(found.version.clone());
    rep.plan.push(format!("download {}", found.asset_name));
    rep.plan.push(match &found.checksum_name {
        Some(n) => format!("verify sha256 against {n}"),
        None if opts.allow_unverified => "no checksum published (installing unverified)".into(),
        None => "no checksum published (apply refuses without --allow-unverified)".into(),
    });
    rep.plan.push(format!("atomic replace {path}"));
    rep.set(
        Status::UpdateAvailable,
        format!(
            "{} available (installed {})",
            found.version,
            current_version.as_deref().unwrap_or("unknown")
        ),
    );
    if opts.mode != Mode::Apply {
        return;
    }
    let target = source::expand_path(path, env.home.as_deref());
    let apply_opts = release::ApplyOptions {
        allow_commands: opts.allow_commands,
        allow_unverified: opts.allow_unverified,
        verify_command: verify_command.clone(),
    };
    match release::apply(
        env.http.as_ref(),
        env.shell.as_ref(),
        &target,
        repo,
        &found,
        &apply_opts,
    ) {
        Ok(done) => {
            rep.step("download", true, found.asset_name.clone());
            rep.step(
                "checksum",
                done.checksum_verified,
                if done.checksum_verified {
                    format!("sha256 {}", done.sha256)
                } else {
                    "not verified".into()
                },
            );
            for note in &done.notes {
                rep.step("note", true, note.clone());
            }
            let mut fields = Map::new();
            fields.insert("current_version".into(), json!(done.version));
            fields.insert("last_updated".into(), json!((env.now)()));
            source::set_meta_fields(entry, fields);
            rep.changed = true;
            rep.set(Status::Updated, format!("updated to {}", done.version));
        }
        Err(e) => {
            rep.step("apply", false, e.clone());
            rep.set(Status::Error, e);
        }
    }
}

fn check_pin(
    env: &Env,
    opts: &Options,
    entry: &mut ServerEntry,
    src: &Source,
    rep: &mut ServerReport,
) {
    let (uvx, package) = match src {
        Source::Npx { package } => (false, package),
        Source::Uvx { package } => (true, package),
        _ => return,
    };
    let spec = pins::parse_spec(&entry.args, uvx).filter(|s| &s.name == package);
    let Some(spec) = spec else {
        return rep.set(
            Status::Skipped,
            format!("{package} not found in the launch arguments"),
        );
    };
    let pinned = match spec.version.as_deref() {
        Some(v) if !pins::is_floating(v) => v.to_string(),
        _ => {
            return rep.set(
                Status::Auto,
                format!("unpinned: {} resolves at runtime", spec.name),
            )
        }
    };
    rep.current = Some(pinned.clone());
    let latest = if uvx {
        pins::latest_pypi(env.http.as_ref(), &env.pypi_index, &spec.name)
    } else {
        pins::latest_npm(env.http.as_ref(), &env.npm_registry, &spec.name)
    };
    let latest = match latest {
        Ok(v) => v,
        Err(e) => return rep.set(Status::Error, e),
    };
    rep.latest = Some(latest.clone());
    match pins::compare_versions(&pinned, &latest) {
        None => rep.set(
            Status::Error,
            format!("could not compare versions '{pinned}' and '{latest}'"),
        ),
        Some(Ordering::Less) => {
            rep.plan.push(format!(
                "rewrite arg {}{}{} -> {}{}{}",
                spec.name, spec.separator, pinned, spec.name, spec.separator, latest
            ));
            rep.set(
                Status::UpdateAvailable,
                format!("pinned {pinned}, latest {latest}"),
            );
            if opts.mode == Mode::Apply && pins::rewrite(&mut entry.args, &spec, &latest) {
                let mut fields = Map::new();
                fields.insert("last_updated".into(), json!((env.now)()));
                source::set_meta_fields(entry, fields);
                rep.step("pin", true, format!("{pinned} -> {latest}"));
                rep.changed = true;
                rep.set(Status::Updated, format!("pin moved {pinned} -> {latest}"));
            }
        }
        Some(_) => rep.set(
            Status::UpToDate,
            format!("pinned at {pinned} (latest {latest})"),
        ),
    }
}

fn init_one(env: &Env, opts: &Options, entry: &mut ServerEntry, rep: &mut ServerReport) {
    let existing = source::stored(entry);
    if existing.is_some() && !opts.force {
        rep.kind = existing.map(|s| s.kind().to_string()).unwrap_or_default();
        return rep.set(Status::Skipped, "already configured");
    }
    let mut detected = source::detect(entry, env.home.as_deref());
    match &mut detected {
        Source::Git {
            path,
            remote_url,
            branch,
            post_update,
        } => {
            let repo = source::expand_path(path, env.home.as_deref());
            if gitops::is_repo(env.git.as_ref(), &repo) {
                *remote_url = gitops::remote_url(env.git.as_ref(), &repo);
                *branch = gitops::default_branch(env.git.as_ref(), &repo);
                *post_update = source::suggest_post_update(&repo);
            }
        }
        Source::GithubRelease { repo, .. } => {
            if let Some(r) = &opts.repo {
                if release::valid_repo(r) {
                    *repo = Some(r.clone());
                }
            }
        }
        _ => {}
    }
    if let Some(Source::Git {
        post_update: old, ..
    }) = &existing
    {
        if let Source::Git { post_update, .. } = &mut detected {
            if post_update.is_none() {
                *post_update = old.clone();
            }
        }
    }
    rep.kind = detected.kind().to_string();
    let message = match &detected {
        Source::Git { path, .. } => format!("git {path}"),
        Source::GithubRelease { path, repo, .. } => format!(
            "release {path}{}",
            repo.as_deref()
                .map(|r| format!(" ({r})"))
                .unwrap_or_default()
        ),
        Source::Npx { package } | Source::Uvx { package } => format!("package {package}"),
        Source::Remote => "remote".into(),
        Source::Unknown { reason } => reason.clone(),
    };
    if !opts.dry_run {
        let mut meta = detected.to_meta();
        meta.insert("last_checked".into(), json!((env.now)()));
        if let Some(Value::Object(old)) = entry.unknown_fields.get(source::META_KEY).cloned() {
            for (k, v) in old {
                meta.entry(k).or_insert(v);
            }
        }
        entry
            .unknown_fields
            .insert(source::META_KEY.into(), Value::Object(meta));
        rep.changed = true;
        rep.set(Status::Configured, message);
    } else {
        rep.set(Status::Skipped, format!("would configure: {message}"));
    }
}

pub fn run(env: &Env, entries: &mut [ServerEntry], opts: &Options) -> Result<Report, String> {
    let mut reports = Vec::new();
    let mut matched = false;
    for entry in entries.iter_mut() {
        if let Some(wanted) = &opts.server {
            if &entry.id != wanted && &entry.name != wanted {
                continue;
            }
        }
        matched = true;
        let id = entry.id.clone();
        let mut rep = ServerReport::new(&id, "", Status::Skipped, "");
        if opts.mode == Mode::Init {
            init_one(env, opts, entry, &mut rep);
            reports.push(rep);
            continue;
        }
        let (src, from_meta) = source::effective(entry, env.home.as_deref());
        rep.kind = src.kind().to_string();
        rep.detected = !from_meta;
        match &src {
            Source::Git { .. } => check_git(env, opts, entry, &src, &mut rep),
            Source::GithubRelease { .. } => check_release(env, opts, entry, &src, &mut rep),
            Source::Npx { .. } | Source::Uvx { .. } => check_pin(env, opts, entry, &src, &mut rep),
            Source::Remote => rep.set(Status::Skipped, "remote server, nothing to update"),
            Source::Unknown { reason } => rep.set(
                Status::Skipped,
                format!("unknown source ({reason}); run update --init"),
            ),
        }
        if !from_meta && rep.status != Status::Error {
            rep.message
                .push_str(" [no stored source; run update --init]");
        }
        reports.push(rep);
    }
    if let Some(wanted) = &opts.server {
        if !matched {
            return Err(format!("server not found: {wanted}"));
        }
    }
    Ok(Report::new(opts.mode, reports))
}

fn changed_ids(report: &Report) -> Vec<String> {
    report
        .servers
        .iter()
        .filter(|s| s.changed)
        .map(|s| s.id.clone())
        .collect()
}

pub fn execute_with(env: &Env, opts: &Options) -> Result<Report, String> {
    let mut registry = registry::load_resolved()?;
    let report = run(env, &mut registry.servers, opts)?;
    let ids = changed_ids(&report);
    if !ids.is_empty() && matches!(opts.mode, Mode::Apply | Mode::Init) && !opts.dry_run {
        let updated: Vec<ServerEntry> = registry
            .servers
            .iter()
            .filter(|s| ids.contains(&s.id))
            .cloned()
            .collect();
        registry::update(|reg| {
            for fresh in &updated {
                if let Some(slot) = reg.servers.iter_mut().find(|s| s.id == fresh.id) {
                    slot.args = fresh.args.clone();
                    if let Some(meta) = fresh.unknown_fields.get(source::META_KEY) {
                        slot.unknown_fields
                            .insert(source::META_KEY.into(), meta.clone());
                    }
                }
            }
            Ok(())
        })?;
    }
    Ok(report)
}

pub fn execute(opts: &Options) -> Result<Report, String> {
    execute_with(&Env::system(), opts)
}

fn flag(args: &Value, key: &str) -> bool {
    args.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn options_from(args: &Value, mode: Mode) -> Options {
    let mut opts = Options::new(mode);
    opts.server = args
        .get("server")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(String::from);
    opts.allow_commands = flag(args, "allowCommands");
    opts.allow_unverified = flag(args, "allowUnverified");
    opts.force = flag(args, "force");
    opts.repo = args.get("repo").and_then(Value::as_str).map(String::from);
    opts
}

pub fn check_handler(args: Value) -> Result<Value, String> {
    let mode = if flag(&args, "dryRun") {
        Mode::DryRun
    } else {
        Mode::Check
    };
    execute(&options_from(&args, mode)).map(|r| r.to_value())
}

pub fn apply_handler(args: Value) -> Result<Value, String> {
    let dry = flag(&args, "dryRun");
    let mode = if flag(&args, "init") {
        Mode::Init
    } else if dry {
        Mode::DryRun
    } else {
        Mode::Apply
    };
    let mut opts = options_from(&args, mode);
    opts.dry_run = dry;
    execute(&opts).map(|r| r.to_value())
}
