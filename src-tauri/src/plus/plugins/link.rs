//! `skills tap link|unlink` and the refresh behind `skills tap update`: a tap whose clone is a
//! plugin marketplace is declared to Claude Code as a directory marketplace and its plugins are
//! installed at user scope, always through Claude Code's own `plugin` commands (as in D-097).
//! Claude copies an installed plugin into a cache keyed by its version, so a pull in the clone
//! reaches a session only after `plugin update`, which the refresh runs when the versions differ.

use super::claude::{ClaudeRunner, NOT_FOUND};
use super::config::{envelope, quote};
use super::installed::{self, Installed};
use super::marketplace::{self, Marketplace, Plugin};
use super::switch::RESTART;
use super::Env;
use crate::plus::jsonfs::read_json;
use crate::plus::op::OpError;
use crate::plus::plan::{Effects, PlanV1, ResultV1, Step};
use crate::plus::skills::taps::{load_taps, tap_dir, taps_root, valid_tap_name};
use crate::plus::sources::fsx;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;

const CLAUDE_TIMEOUT: Duration = Duration::from_secs(120);
const SYNCED: &str = "synced";
const USER: &str = "user";

#[derive(Debug, PartialEq, Eq)]
enum Declared {
    No,
    Here,
    Elsewhere(String),
}

struct Target {
    clone: PathBuf,
    market: Marketplace,
}

/// One `claude` invocation of a plan: what it does, and the arguments after `claude`.
struct Command {
    action: &'static str,
    args: Vec<String>,
}

impl Command {
    fn new(action: &'static str, args: &[&str]) -> Self {
        Self {
            action,
            args: args.iter().map(|a| a.to_string()).collect(),
        }
    }

    fn shown(&self) -> String {
        let words: Vec<String> = self.args.iter().map(|a| quote(a)).collect();
        format!("claude {}", words.join(" "))
    }

    fn step(&self) -> Step {
        Step { op: "exec", path: None, detail: self.shown(), keys: None, diff: None }
    }

    fn change(&self) -> Value {
        let mut argv = vec!["claude".to_string()];
        argv.extend(self.args.iter().cloned());
        json!({"scope": USER, "action": self.action, "command": argv})
    }
}

fn clone_of(env: &Env, tap: &str) -> Result<PathBuf, OpError> {
    valid_tap_name(tap).map_err(OpError::usage)?;
    let data = env
        .data_dir
        .as_deref()
        .ok_or_else(|| OpError::failed("no_data_dir", "no data directory"))?;
    let entry = load_taps(data)
        .into_iter()
        .find(|t| t.name == tap)
        .ok_or_else(|| OpError::not_found(format!("Tap '{tap}' not found.")))?;
    Ok(tap_dir(&taps_root(data), &entry))
}

fn resolve(env: &Env, tap: &str) -> Result<Target, OpError> {
    let clone = clone_of(env, tap)?;
    if !fsx::is_dir(&clone) {
        return Err(OpError::not_found(format!(
            "the clone of tap '{tap}' is missing; run `toolportctl skills tap update`"
        )));
    }
    match marketplace::read(&clone) {
        None => Err(OpError::usage(format!(
            "tap '{tap}' is not a plugin marketplace: it has no {}",
            marketplace::FILE
        ))),
        Some(Err(message)) => Err(OpError::conflict(message)),
        Some(Ok(market)) => Ok(Target { clone, market }),
    }
}

/// The marketplaces Claude Code knows with this clone as their directory.
fn declared_here(claude_home: &Path, clone: &Path) -> Vec<String> {
    let known: Option<Value> = read_json(&claude_home.join("plugins/known_marketplaces.json"));
    let Some(Value::Object(known)) = known else {
        return Vec::new();
    };
    let clone = fsx::canonical(clone);
    known
        .iter()
        .filter(|(_, meta)| {
            let source = meta.get("source");
            source.and_then(|s| s.get("source")).and_then(Value::as_str) == Some("directory")
                && source
                    .and_then(|s| s.get("path"))
                    .and_then(Value::as_str)
                    .is_some_and(|path| fsx::canonical(Path::new(path)) == clone)
        })
        .map(|(name, _)| name.clone())
        .collect()
}

fn declared(claude_home: &Path, target: &Target) -> Declared {
    if declared_here(claude_home, &target.clone).contains(&target.market.name) {
        return Declared::Here;
    }
    match installed::marketplace_source(claude_home, &target.market.name) {
        Some(source) => Declared::Elsewhere(source),
        None => Declared::No,
    }
}

fn at_user_scope(plugin: &Installed) -> bool {
    plugin.scope.as_deref().map_or(true, |scope| scope == USER)
}

fn installed_ids(claude_home: &Path, marketplace: &str) -> Vec<String> {
    installed::file_list(claude_home)
        .into_iter()
        .filter(|p| at_user_scope(p) && p.marketplace.as_deref() == Some(marketplace))
        .map(|p| p.id)
        .collect()
}

fn exec(runner: &dyn ClaudeRunner, command: &Command) -> Result<(), OpError> {
    let shown = command.shown();
    let args: Vec<&str> = command.args.iter().map(String::as_str).collect();
    let out = runner.run(&args, CLAUDE_TIMEOUT).map_err(|e| {
        OpError::conflict(if e == NOT_FOUND {
            format!("{NOT_FOUND}: `{shown}` needs it")
        } else {
            e
        })
    })?;
    if out.ok() {
        return Ok(());
    }
    let line = out.first_error_line();
    Err(OpError::conflict(if line.is_empty() {
        format!("{shown} exited {}", out.code)
    } else {
        line
    }))
}

fn exec_all(runner: &dyn ClaudeRunner, commands: &[Command], undo: &str) -> Result<(), OpError> {
    for (done, command) in commands.iter().enumerate() {
        if let Err(error) = exec(runner, command) {
            let note = if done == 0 {
                String::new()
            } else {
                format!(" ({done} of {} steps ran; {undo} puts it back)", commands.len())
            };
            return Err(OpError::conflict(format!("{}{note}", error.message)));
        }
    }
    Ok(())
}

fn require_claude(runner: &dyn ClaudeRunner, what: &str) -> Result<(), OpError> {
    if runner.present() {
        Ok(())
    } else {
        Err(OpError::conflict(format!("{NOT_FOUND}: `{what}` needs it")))
    }
}

fn choose<'a>(market: &'a Marketplace, only: &[String]) -> Result<Vec<&'a Plugin>, OpError> {
    if market.plugins.is_empty() {
        return Err(OpError::conflict(format!(
            "{} lists no plugin that can be linked from the clone",
            market.name
        )));
    }
    if only.is_empty() {
        return Ok(market.plugins.iter().collect());
    }
    let mut chosen: Vec<&Plugin> = Vec::new();
    for name in only {
        let plugin = market.plugin(name).ok_or_else(|| {
            let listed: Vec<&str> = market.plugins.iter().map(|p| p.name.as_str()).collect();
            OpError::usage(format!(
                "{name} is not a plugin of {}; it has: {}",
                market.name,
                listed.join(", ")
            ))
        })?;
        if !chosen.iter().any(|p| p.name == plugin.name) {
            chosen.push(plugin);
        }
    }
    Ok(chosen)
}

fn note(detail: String) -> Step {
    Step { op: "note", path: None, detail, keys: None, diff: None }
}

fn row(market: &Marketplace, plugin: &Plugin, linked: bool) -> Value {
    json!({
        "name": plugin.name,
        "id": market.id_of(&plugin.name),
        "version": plugin.version,
        "skills": plugin.skills.len(),
        "agents": plugin.agents.len(),
        "hooks": plugin.hooks,
        "linked": linked,
    })
}

fn synced_twins(env: &Env, runner: &dyn ClaudeRunner, chosen: &[&Plugin]) -> Vec<String> {
    installed::list(Some(runner), &env.claude_home)
        .plugins
        .into_iter()
        .filter(|p| p.marketplace.as_deref() == Some(SYNCED))
        .filter(|p| chosen.iter().any(|c| c.name == p.name))
        .map(|p| p.id)
        .collect()
}

fn applied(commands: &[Command], undo: String) -> ResultV1 {
    ResultV1 {
        applied: true,
        changed: commands.iter().map(Command::shown).collect(),
        undo,
        backups: Vec::new(),
    }
}

/// `skills tap link <tap> [--plugin <name>...]`.
pub fn link(
    env: &Env,
    runner: &dyn ClaudeRunner,
    tap: &str,
    only: &[String],
    dry_run: bool,
) -> Result<Value, OpError> {
    require_claude(runner, "claude plugin marketplace add")?;
    let target = resolve(env, tap)?;
    let market = &target.market;
    let chosen = choose(market, only)?;
    let state = declared(&env.claude_home, &target);
    if let Declared::Elsewhere(source) = &state {
        return Err(OpError::conflict(format!(
            "marketplace '{}' is already declared from {source}; \
             `claude plugin marketplace remove {}` first",
            market.name, market.name
        )));
    }
    let present = installed_ids(&env.claude_home, &market.name);

    let mut commands: Vec<Command> = Vec::new();
    if state == Declared::No {
        let clone = fsx::display(&target.clone);
        commands.push(Command::new(
            "marketplace add",
            &["plugin", "marketplace", "add", &clone, "--scope", USER],
        ));
    }
    let mut rows = Vec::new();
    for plugin in &chosen {
        let id = market.id_of(&plugin.name);
        let linked = present.contains(&id);
        if !linked {
            commands.push(Command::new(
                "install",
                &["plugin", "install", &id, "--scope", USER],
            ));
        }
        rows.push(row(market, plugin, linked));
    }

    let mut warnings = market.warnings.clone();
    for twin in synced_twins(env, runner, &chosen) {
        let name = twin.split('@').next().unwrap_or(&twin);
        warnings.push(format!(
            "{twin} is the same plugin as {name}@{}: Claude Code loads the marketplace copy and \
             reports the synced one as not loaded, so on a login that syncs it the clone replaces \
             the organization's copy",
            market.name
        ));
    }
    let hooked: Vec<&str> = chosen
        .iter()
        .filter(|p| p.hooks)
        .map(|p| p.name.as_str())
        .collect();
    if !hooked.is_empty() {
        warnings.push(format!(
            "{} ship hooks, which run in every session on this machine once linked",
            hooked.join(", ")
        ));
    }
    warnings.push(RESTART.to_string());

    let mut steps: Vec<Step> = commands.iter().map(Command::step).collect();
    if commands.is_empty() {
        steps.push(note(format!(
            "already linked: {} is declared and its plugins are installed",
            market.name
        )));
    }
    steps.push(note(
        "updates: `toolportctl skills tap update` pulls the clone and runs `claude plugin update` \
         for what is linked"
            .to_string(),
    ));

    let undo = format!("toolportctl skills tap unlink {tap}");
    let plan = PlanV1 {
        summary: format!("Link tap {tap} to Claude Code as marketplace {}", market.name),
        steps,
        effects: Effects::default(),
        warnings,
        undo: undo.clone(),
    };
    let result = if dry_run {
        None
    } else {
        exec_all(runner, &commands, &undo)?;
        Some(applied(&commands, undo))
    };
    let changes: Vec<Value> = commands.iter().map(Command::change).collect();
    Ok(envelope(
        &market.name,
        USER,
        None,
        dry_run,
        &plan,
        result,
        json!({
            "tap": tap,
            "marketplace": market.name,
            "plugins": rows,
            "changes": changes,
            "conflicts": [],
        }),
    ))
}

/// `skills tap unlink <tap>`: works from what Claude Code recorded, so it also cleans up after a
/// clone that is gone or a marketplace file that no longer parses.
pub fn unlink(env: &Env, runner: &dyn ClaudeRunner, tap: &str, dry_run: bool) -> Result<Value, OpError> {
    require_claude(runner, "claude plugin uninstall")?;
    let clone = clone_of(env, tap)?;
    let names = declared_here(&env.claude_home, &clone);
    let mut commands: Vec<Command> = Vec::new();
    for name in &names {
        for id in installed_ids(&env.claude_home, name) {
            commands.push(Command::new(
                "uninstall",
                &["plugin", "uninstall", &id, "--scope", USER],
            ));
        }
        commands.push(Command::new(
            "marketplace remove",
            &["plugin", "marketplace", "remove", name],
        ));
    }

    let mut steps: Vec<Step> = commands.iter().map(Command::step).collect();
    if commands.is_empty() {
        steps.push(note(format!("tap {tap} is not linked to Claude Code; nothing to remove")));
    }
    let undo = format!("toolportctl skills tap link {tap}");
    let plan = PlanV1 {
        summary: format!("Unlink tap {tap} from Claude Code"),
        steps,
        effects: Effects::default(),
        warnings: vec![RESTART.to_string()],
        undo: undo.clone(),
    };
    let result = if dry_run {
        None
    } else {
        exec_all(runner, &commands, &undo)?;
        Some(applied(&commands, undo))
    };
    let changes: Vec<Value> = commands.iter().map(Command::change).collect();
    Ok(envelope(
        tap,
        USER,
        None,
        dry_run,
        &plan,
        result,
        json!({"tap": tap, "marketplaces": names, "changes": changes, "conflicts": []}),
    ))
}

/// The taps among `taps` that are linked to Claude Code.
pub fn linked_taps(env: &Env, taps: &[String]) -> Vec<String> {
    taps.iter()
        .filter(|tap| {
            clone_of(env, tap)
                .is_ok_and(|clone| !declared_here(&env.claude_home, &clone).is_empty())
        })
        .cloned()
        .collect()
}

/// `plugin update` for every linked plugin of `taps` whose clone carries another version than
/// the installed copy. A failure is a row, never an error: the pull already happened.
pub fn refresh(env: &Env, runner: &dyn ClaudeRunner, taps: &[String], dry_run: bool) -> Vec<Value> {
    let linked: Vec<(String, Vec<String>)> = taps
        .iter()
        .filter_map(|tap| {
            let names = declared_here(&env.claude_home, &clone_of(env, tap).ok()?);
            (!names.is_empty()).then(|| (tap.clone(), names))
        })
        .collect();
    if linked.is_empty() {
        return Vec::new();
    }
    let listed = match installed::cli_list(runner) {
        Ok(listed) => listed,
        Err(error) => {
            return linked
                .iter()
                .map(|(tap, _)| json!({"tap": tap, "error": error}))
                .collect()
        }
    };
    let mut rows = Vec::new();
    for (tap, names) in &linked {
        let drifted = listed.iter().filter(|p| {
            at_user_scope(p)
                && p.marketplace.as_ref().is_some_and(|m| names.contains(m))
                && p.folder_version.is_some()
                && p.version != p.folder_version
        });
        for plugin in drifted {
            let mut out = json!({
                "tap": tap,
                "id": plugin.id,
                "from": plugin.version,
                "to": plugin.folder_version,
            });
            if dry_run {
                out["outcome"] = json!("would update");
            } else {
                let command = Command::new("update", &["plugin", "update", &plugin.id, "--scope", USER]);
                match exec(runner, &command) {
                    Ok(()) => out["outcome"] = json!("updated"),
                    Err(error) => out["error"] = json!(error.message),
                }
            }
            rows.push(out);
        }
    }
    rows
}

/// What `skills tap ls` shows for a tap that is a marketplace; `None` for any other tap.
pub fn describe(env: &Env, tap: &str) -> Option<Value> {
    let clone = clone_of(env, tap).ok()?;
    let market = match marketplace::read(&clone)? {
        Ok(market) => market,
        Err(error) => return Some(json!({"error": error})),
    };
    let target = Target { clone, market };
    let present = installed_ids(&env.claude_home, &target.market.name);
    let plugins: Vec<Value> = target
        .market
        .plugins
        .iter()
        .map(|p| row(&target.market, p, present.contains(&target.market.id_of(&p.name))))
        .collect();
    let declared = match declared(&env.claude_home, &target) {
        Declared::No => "no".to_string(),
        Declared::Here => "here".to_string(),
        Declared::Elsewhere(source) => format!("elsewhere: {source}"),
    };
    Some(json!({
        "name": target.market.name,
        "declared": declared,
        "plugins": plugins,
        "warnings": target.market.warnings,
    }))
}
