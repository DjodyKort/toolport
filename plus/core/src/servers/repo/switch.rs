//! Moving a server's checkout to the branch another machine is on: a plan first, then one
//! `git switch` when the work tree is clean, the agreed build command and a restart. Nothing
//! here stashes, forces, merges or pushes; the branch the person leaves keeps every commit, and
//! undo goes back to it when nothing has moved.

use std::path::Path;

use serde_json::{json, Value};
use toolport_plus_api::Host;

use super::peers::{views, PeerView};
use super::state::{self, RepoState};
use super::store::{Build, RepoStore, Switched};
use super::update::{
    base_of, build_plan, restart_after, run_build, short, store_error, text, BuildPlan, Done,
    Refusal, UpdateArgs, GIT_TIMEOUT,
};
use crate::hashing::lock_hash;
use crate::library::remote::git_out;
use crate::op::OpError;
use crate::servers::read_servers;
use crate::skills::gitexec::git_stdout;
use crate::sync::peers::Peers;

#[derive(Clone, Debug)]
pub struct Plan {
    pub server_id: String,
    pub server_name: String,
    pub state: RepoState,
    pub machine: String,
    pub to_branch: String,
    pub to_head: String,
    /// The branch exists only on the remote: the switch makes a local one that tracks it.
    pub creates: bool,
    pub refusal: Option<Refusal>,
    pub build: BuildPlan,
    pub hash: String,
}

impl Plan {
    pub fn to_value(&self) -> Value {
        json!({
            "server": self.server_name,
            "id": self.server_id,
            "root": self.state.root.to_string_lossy(),
            "fromBranch": self.state.branch,
            "fromHead": self.state.head,
            "toBranch": self.to_branch,
            "toHead": self.to_head,
            "machine": self.machine,
            "createsBranch": self.creates,
            "build": {"command": self.build.command, "remembered": self.build.remembered},
            "refusal": self.refusal.as_ref().map(|r| json!({"code": r.code, "message": r.message})),
            "planHash": self.hash,
        })
    }

    pub fn human(&self) -> String {
        let mut out = format!(
            "{} is on {}; {} is on {} at {}.\n",
            self.server_name,
            text(&self.state.branch),
            self.machine,
            self.to_branch,
            short(&self.to_head)
        );
        match &self.refusal {
            Some(refusal) => out.push_str(&format!("Cannot switch: {}\n", refusal.message)),
            None => {
                out.push_str(&format!(
                    "Switching checks out {}{}. Nothing is stashed or forced.\n",
                    self.to_branch,
                    if self.creates {
                        ", as a new local branch that tracks the remote one"
                    } else {
                        ""
                    }
                ));
                if let Some(command) = &self.build.command {
                    out.push_str(&format!("Then it runs: {command}\n"));
                }
                out.push_str(&format!(
                    "Apply with: toolportctl server repo switch {} --plan-hash {}\n",
                    self.server_name, self.hash
                ));
            }
        }
        out
    }
}

fn target(views: Vec<PeerView>, args: &UpdateArgs, server_id: &str) -> Option<PeerView> {
    views
        .into_iter()
        .filter(|v| v.server_id == server_id && v.mine_branch != v.theirs.branch)
        .filter(|v| args.machine.as_deref().is_none_or(|m| m == v.machine))
        .max_by(|a, b| a.made_at.cmp(&b.made_at))
}

fn refusal_of(view: &PeerView, state: &RepoState) -> (Option<Refusal>, bool) {
    let root = &state.root;
    let branch = &view.theirs.branch;
    let refuse = |code, message: String| (Some(Refusal { code, message }), false);
    if !view.theirs.pushed {
        return refuse(
            "not_pushed",
            format!(
                "{}'s commits are on no remote, so this machine cannot install them; push them from there first",
                view.machine
            ),
        );
    }
    if state.branch.is_none() {
        return refuse(
            "detached",
            "HEAD is detached; check out a branch first".into(),
        );
    }
    if let Some(open) = state.busy {
        return refuse(
            "busy",
            format!("{open} is open in this checkout; finish or abort it first"),
        );
    }
    if state.dirty > 0 {
        return refuse(
            "dirty",
            format!(
                "local changes in {} tracked file(s) block switching; commit or stash them yourself",
                state.dirty
            ),
        );
    }
    if git_out(root, &["check-ref-format", "--branch", branch]).is_none() {
        return refuse(
            "bad_branch",
            format!("{branch} is not a usable branch name"),
        );
    }
    if git_out(
        root,
        &[
            "rev-parse",
            "--verify",
            "-q",
            &format!("refs/heads/{branch}"),
        ],
    )
    .is_some()
    {
        return (None, false);
    }
    let remote_ref = state
        .remote
        .as_ref()
        .map(|remote| format!("refs/remotes/{remote}/{branch}"));
    if remote_ref
        .as_deref()
        .is_some_and(|r| git_out(root, &["rev-parse", "--verify", "-q", r]).is_some())
    {
        return (None, true);
    }
    refuse(
        "not_fetched",
        format!("{branch} is not in this checkout yet; plan again with --fetch"),
    )
}

pub fn plan(host: &dyn Host, args: &UpdateArgs) -> Result<Plan, OpError> {
    let servers = read_servers(host)?;
    let (server, root) = super::update::locate(&servers, &args.key)?;
    if args.fetch {
        super::fetch_tracked(&root).map_err(|e| OpError::failed("fetch_failed", e))?;
    }
    let base = base_of(host)?;
    let repos = super::repos_for(host, Some(&server.id))?;
    let peers = Peers::read(&base);
    let view = target(views(&repos, &peers), args, &server.id).ok_or_else(|| {
        OpError::not_found(format!(
            "no other machine has published another branch of {}",
            server.name
        ))
    })?;
    let state = state::read(&root);
    let (refusal, creates) = refusal_of(&view, &state);
    let store = RepoStore::read(&base).map_err(store_error)?;
    let build = build_plan(host, args, &store, &server.id)?;
    let hash = lock_hash(format!(
        "switch\n{}\n{}\n{}\n{}\n{}\n{}",
        server.id,
        text(&state.branch),
        text(&state.head),
        view.theirs.branch,
        view.theirs.head,
        text(&build.command)
    ));
    Ok(Plan {
        server_id: server.id,
        server_name: server.name,
        state,
        machine: view.machine,
        to_branch: view.theirs.branch,
        to_head: view.theirs.head,
        creates,
        refusal,
        build,
        hash,
    })
}

pub fn apply(host: &dyn Host, args: &UpdateArgs) -> Result<(Done, String), OpError> {
    let plan = plan(host, args)?;
    if let Some(refusal) = &plan.refusal {
        return Err(OpError::failed("switch_refused", refusal.message.clone())
            .with_details(json!({"reason": refusal.code})));
    }
    if args.plan_hash.as_deref().is_some_and(|h| h != plan.hash) {
        return Err(OpError::conflict(
            "the plan has changed since it was reviewed; review it again",
        )
        .with_details(json!({"planHash": plan.hash})));
    }
    let base = base_of(host)?;
    let root = plan.state.root.clone();
    let (from_branch, from_head) = (
        text(&plan.state.branch).to_string(),
        text(&plan.state.head).to_string(),
    );
    let branch = plan.to_branch.clone();
    let switched = if plan.creates {
        let tracked = format!("{}/{branch}", text(&plan.state.remote));
        git_stdout(
            Some(&root),
            &["switch", "-c", &branch, "--track", &tracked],
            GIT_TIMEOUT,
        )
    } else {
        git_stdout(Some(&root), &["switch", &branch], GIT_TIMEOUT)
    };
    switched.map_err(|e| OpError::failed("switch_failed", format!("{e}; nothing was changed")))?;
    let landed = git_out(&root, &["rev-parse", "--verify", "-q", "HEAD"]).unwrap_or_default();
    let mut warnings = Vec::new();
    let build = plan
        .build
        .command
        .as_deref()
        .map(|command| run_build(host, &root, command, args.build_timeout));
    let now = host.now_unix();
    let recorded = RepoStore::read(&base).and_then(|mut store| {
        store.switched.insert(
            plan.server_id.clone(),
            Switched {
                root: root.to_string_lossy().into_owned(),
                from_branch: from_branch.clone(),
                from_head: from_head.clone(),
                to_branch: branch.clone(),
                to_head: landed.clone(),
                created: plan.creates,
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
            "the switch was made but could not be recorded for undo: {error}; go back with git -C {} switch {from_branch}",
            root.display()
        ));
    }
    let restart = if build.as_ref().is_none_or(|run| run.ok) {
        restart_after(host, args, &plan.server_id)
    } else {
        Some((
            "skipped",
            "the build failed, so the server was not restarted".to_string(),
        ))
    };
    let done = Done {
        server_id: plan.server_id,
        undo: format!("toolportctl server repo switch {} --undo", plan.server_name),
        server_name: plan.server_name,
        root: root.to_string_lossy().into_owned(),
        branch,
        from: from_head,
        to: landed,
        moved: true,
        build,
        restart,
        warnings,
    };
    Ok((done, from_branch))
}

#[derive(Clone, Debug)]
pub struct BackPlan {
    pub server_id: String,
    pub server_name: String,
    pub switched: Switched,
    pub refusal: Option<Refusal>,
}

impl BackPlan {
    pub fn to_value(&self) -> Value {
        json!({
            "server": self.server_name,
            "id": self.server_id,
            "root": self.switched.root,
            "fromBranch": self.switched.to_branch,
            "toBranch": self.switched.from_branch,
            "removesBranch": self.switched.created,
            "refusal": self.refusal.as_ref().map(|r| json!({"code": r.code, "message": r.message})),
        })
    }

    pub fn human(&self) -> String {
        match &self.refusal {
            Some(refusal) => format!("Cannot go back: {}\n", refusal.message),
            None => format!(
                "{} goes back from {} to {}.\n",
                self.server_name, self.switched.to_branch, self.switched.from_branch
            ),
        }
    }
}

pub fn back_plan(host: &dyn Host, key: &str) -> Result<BackPlan, OpError> {
    let servers = read_servers(host)?;
    let server = crate::servers::find(&servers, key)
        .ok_or_else(|| OpError::not_found(format!("no server '{key}'")))?
        .clone();
    let store = RepoStore::read(&base_of(host)?).map_err(store_error)?;
    let switched = store.switched.get(&server.id).cloned().ok_or_else(|| {
        OpError::not_found(format!("no branch switch of {} to take back", server.name))
    })?;
    let root = Path::new(&switched.root);
    let state = state::read(root);
    let refuse = |code, message: String| Some(Refusal { code, message });
    let refusal = if !root.is_dir() {
        refuse("gone", format!("{} no longer exists", switched.root))
    } else if state.branch.as_deref() != Some(switched.to_branch.as_str())
        || state.head.as_deref() != Some(switched.to_head.as_str())
    {
        refuse(
            "moved",
            format!(
                "{} has moved on since the switch, so nothing was changed",
                switched.to_branch
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
    } else if git_out(
        root,
        &[
            "rev-parse",
            "--verify",
            "-q",
            &format!("refs/heads/{}", switched.from_branch),
        ],
    )
    .is_none()
    {
        refuse(
            "gone",
            format!("branch {} no longer exists here", switched.from_branch),
        )
    } else {
        None
    };
    Ok(BackPlan {
        server_id: server.id,
        server_name: server.name,
        switched,
        refusal,
    })
}

pub fn back(host: &dyn Host, args: &UpdateArgs) -> Result<Done, OpError> {
    let plan = back_plan(host, &args.key)?;
    if let Some(refusal) = &plan.refusal {
        return Err(OpError::failed("undo_refused", refusal.message.clone())
            .with_details(json!({"reason": refusal.code})));
    }
    let base = base_of(host)?;
    let switched = plan.switched.clone();
    let root = Path::new(&switched.root);
    git_stdout(Some(root), &["switch", &switched.from_branch], GIT_TIMEOUT)
        .map_err(|e| OpError::failed("undo_failed", format!("{e}; nothing was changed")))?;
    let mut warnings = Vec::new();
    if switched.created {
        if let Err(error) = git_stdout(
            Some(root),
            &["branch", "-d", &switched.to_branch],
            GIT_TIMEOUT,
        ) {
            warnings.push(format!(
                "the branch {} was kept: {error}",
                switched.to_branch
            ));
        }
    }
    let forgotten = RepoStore::read(&base).and_then(|mut store| {
        store.switched.remove(&plan.server_id);
        store.write(&base)
    });
    if let Err(error) = forgotten {
        warnings.push(format!("the undo record could not be cleared: {error}"));
    }
    let build = switched
        .build
        .as_deref()
        .filter(|_| !args.no_build)
        .map(|command| run_build(host, root, command, args.build_timeout));
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
        undo: format!("toolportctl server repo switch {}", plan.server_name),
        server_name: plan.server_name,
        root: switched.root.clone(),
        branch: switched.from_branch,
        from: switched.to_head,
        to: switched.from_head,
        moved: true,
        build,
        restart,
        warnings,
    })
}
