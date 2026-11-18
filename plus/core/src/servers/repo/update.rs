//! Installing the new commits of a server's checkout: a plan first, then a fast-forward of the
//! tracked branch only, the build command the person agreed to, and a restart through the
//! gateway. Undo moves the branch back when nothing else has moved it. Nothing here fetches,
//! stashes, forces or touches another branch.

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use toolport_plus_api::{Host, ServerRecord};

use super::discover::discover;
use super::state::{self, RepoState, Verdict};
use super::store::{Applied, Build, RepoStore};
use crate::hashing::lock_hash;
use crate::library::remote::git_out;
use crate::op::OpError;
use crate::redact::scrub_text;
use crate::servers::restart::{self, RestartArgs, Status, Timing};
use crate::servers::{find, read_servers};
use crate::skills::gitexec::{git_stdout, run_command};
use crate::skills::taps::redact;

pub const BUILD_TIMEOUT: Duration = Duration::from_secs(120);
const GIT_TIMEOUT: Duration = Duration::from_secs(30);
const LIST_LIMIT: usize = 50;
const FILE_LIMIT: usize = 200;
const OUTPUT_LINES: usize = 20;
const OUTPUT_LINE_CHARS: usize = 300;
const PROCESS_SECRETS: [&str; 3] = [
    "TOOLPORT_SECRET_KEY",
    "CONDUIT_SECRET_KEY",
    "TOOLPORT_HTTP_TOKEN",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestartChoice {
    /// Restart unless a call is running; say so when one is.
    IfIdle,
    /// Restart now; calls in flight end.
    Now,
    Skip,
}

pub struct UpdateArgs {
    pub key: String,
    /// The build command to run; without it the remembered one runs.
    pub build: Option<String>,
    pub no_build: bool,
    pub dry_run: bool,
    /// The hash of the plan the person reviewed; the update stops when the plan has changed.
    pub plan_hash: Option<String>,
    pub restart: RestartChoice,
    pub timing: Timing,
    pub restart_wait: Duration,
    pub build_timeout: Duration,
}

impl UpdateArgs {
    pub fn new(key: &str) -> Self {
        Self {
            key: key.to_string(),
            build: None,
            no_build: false,
            dry_run: false,
            plan_hash: None,
            restart: RestartChoice::IfIdle,
            timing: Timing::default(),
            restart_wait: restart::DEFAULT_WAIT,
            build_timeout: BUILD_TIMEOUT,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refusal {
    pub code: &'static str,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildPlan {
    pub command: Option<String>,
    /// The command is the one agreed to earlier for this server.
    pub remembered: bool,
}

#[derive(Clone, Debug)]
pub struct Plan {
    pub server_id: String,
    pub server_name: String,
    pub state: RepoState,
    /// Other servers that run from the same checkout and get the new files too.
    pub shared_with: Vec<String>,
    pub commits: Vec<(String, String)>,
    pub commits_total: u64,
    pub files: Vec<(String, String)>,
    pub files_total: usize,
    pub refusal: Option<Refusal>,
    pub build: BuildPlan,
    /// `None` when no gateway answered, so nothing is running that a restart could interrupt.
    pub in_flight: Option<usize>,
    pub hash: String,
}

fn text(value: &Option<String>) -> &str {
    value.as_deref().unwrap_or("")
}

fn base_of(host: &dyn Host) -> Result<std::path::PathBuf, OpError> {
    host.data_dir()
        .ok_or_else(|| OpError::failed("no_data_dir", "the data directory could not be resolved"))
}

fn store_error(error: String) -> OpError {
    OpError::failed("server_repo_store", error)
}

pub fn locate(
    servers: &[ServerRecord],
    key: &str,
) -> Result<(ServerRecord, std::path::PathBuf), OpError> {
    let server = find(servers, key)
        .ok_or_else(|| OpError::not_found(format!("no server '{key}'")))?
        .clone();
    let home = crate::fsx::home();
    let found = discover(std::slice::from_ref(&server), home.as_deref());
    match found.into_iter().next() {
        Some(found) => Ok((server, found.root)),
        None => Err(OpError::not_found(format!(
            "{} does not run from a git checkout on this machine",
            server.name
        ))),
    }
}

fn refusal_of(state: &RepoState) -> Option<Refusal> {
    let branch = text(&state.branch);
    let upstream = text(&state.upstream);
    let refuse = |code, message: String| Some(Refusal { code, message });
    match state.verdict() {
        Verdict::Behind => None,
        Verdict::UpToDate => refuse("up_to_date", format!("{branch} is up to date with {upstream}")),
        Verdict::Detached => refuse(
            "detached",
            "HEAD is detached; check out the branch first, Toolport only updates a branch".into(),
        ),
        Verdict::NoUpstream => refuse(
            "no_upstream",
            format!("{branch} tracks no remote branch, so there is nothing to update from"),
        ),
        Verdict::Busy => refuse(
            "busy",
            format!(
                "{} is open in this checkout; finish or abort it first",
                state.busy.unwrap_or("an operation")
            ),
        ),
        Verdict::Diverged => refuse(
            "diverged",
            format!(
                "your branch diverged: {} local commit(s) and {} new commit(s) on {upstream}; Toolport only fast-forwards, so rebase or merge it yourself",
                state.ahead, state.behind
            ),
        ),
        Verdict::Dirty => refuse(
            "dirty",
            format!(
                "local changes in {} tracked file(s) block the update; commit or stash them yourself",
                state.dirty
            ),
        ),
    }
}

fn commits_between(root: &Path, to: &str) -> Vec<(String, String)> {
    git_out(
        root,
        &[
            "log",
            "--format=%H%x1f%s",
            "-n",
            &LIST_LIMIT.to_string(),
            &format!("HEAD..{to}"),
        ],
    )
    .unwrap_or_default()
    .lines()
    .filter_map(|line| {
        let (id, subject) = line.split_once('\u{1f}')?;
        Some((id.to_string(), subject.to_string()))
    })
    .collect()
}

fn files_between(root: &Path, to: &str) -> Vec<(String, String)> {
    git_out(root, &["diff", "--name-status", "--no-renames", "HEAD", to])
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let (status, path) = line.split_once('\t')?;
            Some((status.to_string(), path.to_string()))
        })
        .collect()
}

fn hash_of(server_id: &str, state: &RepoState, build: &BuildPlan) -> String {
    lock_hash(format!(
        "{}\n{}\n{}\n{}\n{}",
        server_id,
        text(&state.branch),
        text(&state.head),
        text(&state.upstream_head),
        text(&build.command)
    ))
}

fn build_plan(args: &UpdateArgs, store: &RepoStore, server_id: &str) -> BuildPlan {
    let remembered = store.remembered(server_id);
    let command = if args.no_build {
        None
    } else {
        args.build
            .as_deref()
            .or(remembered)
            .map(|c| c.trim().to_string())
            .filter(|c| !c.is_empty())
    };
    BuildPlan {
        remembered: command.is_some() && command.as_deref() == remembered,
        command,
    }
}

/// What an update would do right now, from the state the last check left on disk.
pub fn plan(host: &dyn Host, args: &UpdateArgs, ask_gateways: bool) -> Result<Plan, OpError> {
    let servers = read_servers(host)?;
    let (server, root) = locate(&servers, &args.key)?;
    let store = RepoStore::read(&base_of(host)?).map_err(store_error)?;
    let state = state::read(&root);
    let refusal = refusal_of(&state);
    let to = text(&state.upstream_head).to_string();
    let (commits, files) = if refusal.is_none() {
        (commits_between(&root, &to), files_between(&root, &to))
    } else {
        (Vec::new(), Vec::new())
    };
    let files_total = files.len();
    let shared_with = discover(&servers, crate::fsx::home().as_deref())
        .into_iter()
        .filter(|f| f.root == state.root && f.server_id != server.id)
        .map(|f| f.server_name)
        .collect();
    let build = build_plan(args, &store, &server.id);
    let in_flight = if ask_gateways && refusal.is_none() {
        restart::calls_in_flight(host, &server.id, args.timing, args.restart_wait)?
    } else {
        None
    };
    Ok(Plan {
        hash: hash_of(&server.id, &state, &build),
        server_id: server.id,
        server_name: server.name,
        commits_total: state.behind,
        commits,
        files: files.into_iter().take(FILE_LIMIT).collect(),
        files_total,
        refusal,
        build,
        in_flight,
        shared_with,
        state,
    })
}

fn short(id: &str) -> String {
    RepoState::short(Some(id))
}

impl Plan {
    pub fn to_value(&self) -> Value {
        let state = &self.state;
        json!({
            "server": self.server_name,
            "id": self.server_id,
            "root": state.root.to_string_lossy(),
            "branch": state.branch,
            "upstream": state.upstream,
            "from": state.head,
            "to": state.upstream_head,
            "commits": self.commits.iter().map(|(id, subject)| json!({"id": short(id), "subject": subject})).collect::<Vec<_>>(),
            "commitsTotal": self.commits_total,
            "files": self.files.iter().map(|(status, path)| json!({"status": status, "path": path})).collect::<Vec<_>>(),
            "filesTotal": self.files_total,
            "sharedWith": self.shared_with,
            "refusal": self.refusal.as_ref().map(|r| json!({"code": r.code, "message": r.message})),
            "build": {
                "command": self.build.command,
                "remembered": self.build.remembered,
                "timeoutSeconds": BUILD_TIMEOUT.as_secs(),
            },
            "inFlight": self.in_flight,
            "planHash": self.hash,
            "undo": format!("toolportctl server repo undo {}", self.server_name),
        })
    }

    pub fn human(&self) -> String {
        let state = &self.state;
        let mut out = format!("{} runs from {}\n", self.server_name, state.root.display());
        if let Some(refusal) = &self.refusal {
            out.push_str(&format!("Nothing to apply: {}.\n", refusal.message));
            return out;
        }
        out.push_str(&format!(
            "Fast-forward {} {} to {}: {} commit(s)\n",
            text(&state.branch),
            short(text(&state.head)),
            short(text(&state.upstream_head)),
            self.commits_total
        ));
        for (id, subject) in &self.commits {
            out.push_str(&format!("  {} {subject}\n", short(id)));
        }
        out.push_str(&format!("{} file(s) change\n", self.files_total));
        for (status, path) in self.files.iter().take(20) {
            out.push_str(&format!("  {status} {path}\n"));
        }
        if !self.shared_with.is_empty() {
            out.push_str(&format!("Also used by {}.\n", self.shared_with.join(", ")));
        }
        match &self.build.command {
            Some(command) => out.push_str(&format!(
                "Then run in the checkout (stops after {} s): {command}{}\n",
                BUILD_TIMEOUT.as_secs(),
                if self.build.remembered {
                    " (agreed to earlier)"
                } else {
                    ""
                }
            )),
            None => out.push_str("No build command: only the files change.\n"),
        }
        match self.in_flight {
            Some(0) => out.push_str("Then restart it; no call is running.\n"),
            Some(calls) => out.push_str(&format!(
                "{calls} call(s) are running; a restart waits unless you pass --restart-now.\n"
            )),
            None => out.push_str("Then restart it through the gateway if one is running.\n"),
        }
        out.push_str(&format!(
            "Apply with: toolportctl server repo update {} --plan-hash {}\n",
            self.server_name, self.hash
        ));
        out
    }
}

#[derive(Clone, Debug)]
pub struct BuildRun {
    pub command: String,
    pub ok: bool,
    pub code: i32,
    pub elapsed_ms: u64,
    pub output: String,
}

impl BuildRun {
    fn to_value(&self) -> Value {
        json!({
            "command": self.command, "ok": self.ok, "exitCode": self.code,
            "elapsedMs": self.elapsed_ms, "output": self.output,
        })
    }
}

fn shell(command: &str) -> Command {
    let mut cmd;
    if cfg!(windows) {
        cmd = Command::new("cmd");
        cmd.args(["/C", command]);
    } else {
        cmd = Command::new("sh");
        cmd.args(["-c", command]);
    }
    for name in PROCESS_SECRETS {
        cmd.env_remove(name);
    }
    cmd
}

fn tail(stdout: &str, stderr: &str) -> String {
    let lines: Vec<String> = stdout
        .lines()
        .chain(stderr.lines())
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.chars().take(OUTPUT_LINE_CHARS).collect())
        .collect();
    let from = lines.len().saturating_sub(OUTPUT_LINES);
    scrub_text(redact(&lines[from..].join("\n")))
}

pub fn run_build(root: &Path, command: &str, timeout: Duration) -> BuildRun {
    let started = Instant::now();
    let mut cmd = shell(command);
    cmd.current_dir(root);
    let (ok, code, output) = match run_command(cmd, timeout) {
        Ok(out) => (out.ok(), out.code, tail(&out.stdout, &out.stderr)),
        Err(error) => (false, -1, error),
    };
    BuildRun {
        command: command.to_string(),
        ok,
        code,
        elapsed_ms: started.elapsed().as_millis() as u64,
        output,
    }
}

#[derive(Clone, Debug)]
pub struct Done {
    pub server_id: String,
    pub server_name: String,
    pub root: String,
    pub branch: String,
    pub from: String,
    pub to: String,
    pub moved: bool,
    pub build: Option<BuildRun>,
    pub restart: Option<(&'static str, String)>,
    pub warnings: Vec<String>,
    pub undo: String,
}

impl Done {
    pub fn failed(&self) -> bool {
        self.build.as_ref().is_some_and(|b| !b.ok)
            || self.restart.as_ref().is_some_and(|(s, _)| *s == "failed")
    }

    pub fn to_value(&self) -> Value {
        json!({
            "applied": self.moved,
            "server": self.server_name,
            "id": self.server_id,
            "root": self.root,
            "branch": self.branch,
            "from": self.from,
            "to": self.to,
            "build": self.build.as_ref().map(BuildRun::to_value),
            "restart": self.restart.as_ref().map(|(status, line)| json!({"status": status, "detail": line})),
            "warnings": self.warnings,
            "undo": self.undo,
        })
    }

    pub fn human(&self, verb: &str) -> String {
        let mut out = if self.moved {
            format!(
                "{verb} {} on {}: {} to {}.\n",
                self.server_name,
                self.branch,
                short(&self.from),
                short(&self.to)
            )
        } else {
            format!("{} is already up to date.\n", self.server_name)
        };
        if let Some(build) = &self.build {
            out.push_str(&format!(
                "Build {} in {} s: {}\n",
                if build.ok { "finished" } else { "FAILED" },
                build.elapsed_ms / 1000,
                build.command
            ));
            if !build.ok && !build.output.is_empty() {
                out.push_str(&format!("{}\n", build.output));
            }
        }
        if let Some((status, detail)) = &self.restart {
            out.push_str(&format!("Restart ({status}): {detail}\n"));
        }
        for warning in &self.warnings {
            out.push_str(&format!("Warning: {warning}\n"));
        }
        if self.moved {
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
    if args.restart == RestartChoice::Skip {
        return None;
    }
    let mut ask = RestartArgs::new(server_id);
    ask.if_idle = args.restart == RestartChoice::IfIdle;
    ask.wait = args.restart_wait;
    ask.timing = args.timing;
    Some(match restart::restart(host, &ask) {
        Ok(done) => {
            let status = done.status.as_str();
            let detail = if done.status == Status::Busy {
                format!(
                    "a call is running; restart when it ends with: toolportctl server restart {}",
                    done.name
                )
            } else {
                done.line()
            };
            (status, detail)
        }
        Err(error) => ("failed", error.message),
    })
}

fn nothing(plan: &Plan, undo: String) -> Done {
    Done {
        server_id: plan.server_id.clone(),
        server_name: plan.server_name.clone(),
        root: plan.state.root.to_string_lossy().into_owned(),
        branch: text(&plan.state.branch).to_string(),
        from: text(&plan.state.head).to_string(),
        to: text(&plan.state.head).to_string(),
        moved: false,
        build: None,
        restart: None,
        warnings: Vec::new(),
        undo,
    }
}

fn undo_line(name: &str) -> String {
    format!("toolportctl server repo undo {name}")
}

/// Fast-forwards the tracked branch to the commit the last check saw, builds when a command was
/// agreed to, and restarts the server.
pub fn apply(host: &dyn Host, args: &UpdateArgs) -> Result<Done, OpError> {
    let plan = plan(host, args, false)?;
    if let Some(refusal) = &plan.refusal {
        if refusal.code == "up_to_date" {
            return Ok(nothing(&plan, undo_line(&plan.server_name)));
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
    let state = &plan.state;
    let (root, branch) = (&state.root, text(&state.branch).to_string());
    let (from, to) = (
        text(&state.head).to_string(),
        text(&state.upstream_head).to_string(),
    );
    git_stdout(Some(root), &["merge", "--ff-only", &to], GIT_TIMEOUT)
        .map_err(|e| OpError::failed("update_failed", format!("{e}; nothing was changed")))?;
    if git_out(root, &["rev-parse", "--verify", "-q", "HEAD"]).as_deref() != Some(to.as_str()) {
        return Err(OpError::failed(
            "update_failed",
            "the branch did not end on the expected commit",
        ));
    }
    let mut warnings = Vec::new();
    let build = plan
        .build
        .command
        .as_deref()
        .map(|command| run_build(root, command, args.build_timeout));
    let now = host.now_unix();
    let recorded = RepoStore::read(&base).and_then(|mut store| {
        store.applied.insert(
            plan.server_id.clone(),
            Applied {
                root: root.to_string_lossy().into_owned(),
                branch: branch.clone(),
                from: from.clone(),
                to: to.clone(),
                at: now,
                build: plan.build.command.clone(),
            },
        );
        if let Some(run) = build.as_ref().filter(|run| run.ok) {
            store.builds.insert(
                plan.server_id.clone(),
                Build {
                    command: run.command.clone(),
                    at: now,
                },
            );
        }
        store.write(&base)
    });
    if let Err(error) = recorded {
        warnings.push(format!(
            "the update was applied but could not be recorded for undo: {error}; go back with git -C {} reset --keep {}",
            root.display(),
            short(&from)
        ));
    }
    let built_ok = build.as_ref().is_none_or(|run| run.ok);
    let restart = if built_ok {
        restart_after(host, args, &plan.server_id)
    } else {
        Some((
            "skipped",
            "the build failed, so the server was not restarted".to_string(),
        ))
    };
    Ok(Done {
        moved: true,
        build,
        restart,
        warnings,
        from,
        to,
        branch,
        root: root.to_string_lossy().into_owned(),
        undo: undo_line(&plan.server_name),
        server_id: plan.server_id,
        server_name: plan.server_name,
    })
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
            "root": self.applied.root,
            "branch": self.applied.branch,
            "from": self.applied.to,
            "to": self.applied.from,
            "build": self.applied.build,
            "refusal": self.refusal.as_ref().map(|r| json!({"code": r.code, "message": r.message})),
        })
    }

    pub fn human(&self) -> String {
        match &self.refusal {
            Some(refusal) => format!("Cannot go back: {}.\n", refusal.message),
            None => format!(
                "Would move {} on {} back from {} to {}{}, then restart it.\n",
                self.server_name,
                self.applied.branch,
                short(&self.applied.to),
                short(&self.applied.from),
                self.applied
                    .build
                    .as_ref()
                    .map(|c| format!(", rebuild with {c}"))
                    .unwrap_or_default()
            ),
        }
    }
}

/// Whether the one update on record can be taken back: the branch must still be where the update
/// left it, with nothing open and no local changes.
pub fn undo_plan(host: &dyn Host, key: &str) -> Result<UndoPlan, OpError> {
    let servers = read_servers(host)?;
    let server = find(&servers, key)
        .ok_or_else(|| OpError::not_found(format!("no server '{key}'")))?
        .clone();
    let store = RepoStore::read(&base_of(host)?).map_err(store_error)?;
    let applied =
        store.applied.get(&server.id).cloned().ok_or_else(|| {
            OpError::not_found(format!("no update of {} to take back", server.name))
        })?;
    let root = Path::new(&applied.root);
    let state = state::read(root);
    let refuse = |code, message: String| Some(Refusal { code, message });
    let refusal = if !root.is_dir() {
        refuse("gone", format!("{} no longer exists", applied.root))
    } else if state.branch.as_deref() != Some(applied.branch.as_str()) {
        refuse(
            "moved",
            format!("{} is no longer the checked-out branch", applied.branch),
        )
    } else if state.head.as_deref() != Some(applied.to.as_str()) {
        refuse(
            "moved",
            format!(
                "{} has moved on since the update, so nothing was changed",
                applied.branch
            ),
        )
    } else if state.busy.is_some() {
        refuse("busy", "a merge or rebase is open in this checkout".into())
    } else if state.dirty > 0 {
        refuse(
            "dirty",
            format!(
                "local changes in {} tracked file(s) block going back",
                state.dirty
            ),
        )
    } else if git_stdout(
        Some(root),
        &["cat-file", "-e", &format!("{}^{{commit}}", applied.from)],
        GIT_TIMEOUT,
    )
    .is_err()
    {
        refuse(
            "gone",
            format!(
                "commit {} is no longer in the checkout",
                short(&applied.from)
            ),
        )
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

/// Moves the branch back to where it was before the update and runs the same build again.
pub fn undo(host: &dyn Host, args: &UpdateArgs) -> Result<Done, OpError> {
    let plan = undo_plan(host, &args.key)?;
    if let Some(refusal) = &plan.refusal {
        return Err(OpError::failed("undo_refused", refusal.message.clone())
            .with_details(json!({"reason": refusal.code})));
    }
    let base = base_of(host)?;
    let applied = plan.applied.clone();
    let root = Path::new(&applied.root);
    git_stdout(Some(root), &["reset", "--keep", &applied.from], GIT_TIMEOUT)
        .map_err(|e| OpError::failed("undo_failed", format!("{e}; nothing was changed")))?;
    let mut warnings = Vec::new();
    let forgotten = RepoStore::read(&base).and_then(|mut store| {
        store.applied.remove(&plan.server_id);
        store.write(&base)
    });
    if let Err(error) = forgotten {
        warnings.push(format!("the undo record could not be cleared: {error}"));
    }
    let build = applied
        .build
        .as_deref()
        .filter(|_| !args.no_build)
        .map(|command| run_build(root, command, args.build_timeout));
    let restart = if build.as_ref().is_none_or(|run| run.ok) {
        restart_after(host, args, &plan.server_id)
    } else {
        Some((
            "skipped",
            "the build failed, so the server was not restarted".to_string(),
        ))
    };
    Ok(Done {
        server_id: plan.server_id,
        server_name: plan.server_name.clone(),
        root: applied.root.clone(),
        branch: applied.branch,
        from: applied.to,
        to: applied.from,
        moved: true,
        build,
        restart,
        warnings,
        undo: format!("toolportctl server repo update {}", plan.server_name),
    })
}
