//! `skills tap link|unlink` and the refresh behind `skills tap update`: a tap whose clone is a
//! plugin marketplace is declared to Claude Code as a directory marketplace
//! (`extraKnownMarketplaces`) and its plugins are switched on (`enabledPlugins`) in the user
//! settings, written through `SettingsStore::write_key` so comments and every other key stay.
//! Claude Code installs what is switched on when a session starts, and copies a plugin into a
//! cache keyed by its version, so a pull in the clone reaches a session only after
//! `claude plugin update`, which the refresh runs when the versions differ.

use super::claude::{ClaudeRunner, NOT_FOUND};
use super::installed::{self, Installed};
use super::marketplace::{self, Marketplace, Plugin};
use super::{read_json, Env};
use crate::context::fsx;
use crate::context::plan::{Diff, Effects, PlanV1, ResultV1, Step};
use crate::context::plugin_switch::RESTART;
use crate::op::OpError;
use crate::skills::taps::{load_taps, tap_dir, taps_root, valid_tap_name};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;
use toolport_plus_api::SettingsStore;

const CLAUDE_TIMEOUT: Duration = Duration::from_secs(120);
const SYNCED: &str = "synced";
pub(crate) const USER: &str = "user";
pub(crate) const MARKETS_KEY: &str = "extraKnownMarketplaces";
pub(crate) const ENABLED_KEY: &str = "enabledPlugins";

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

/// The user settings file as the host reads it.
pub(crate) struct Settings {
    pub(crate) path: PathBuf,
    pub(crate) doc: Value,
}

impl Settings {
    pub(crate) fn load(store: &dyn SettingsStore, env: &Env) -> Result<Self, OpError> {
        let path = env.user_settings();
        let (doc, _) = store.read_settings(&path).map_err(|e| {
            OpError::conflict(format!(
                "{} cannot be read: {e}; nothing was changed",
                path.display()
            ))
        })?;
        Ok(Self { path, doc })
    }

    /// For listings: a settings file that cannot be read counts as one that declares nothing.
    fn lenient(store: &dyn SettingsStore, env: &Env) -> Self {
        Self::load(store, env).unwrap_or_else(|_| Self {
            path: env.user_settings(),
            doc: Value::Object(Map::new()),
        })
    }

    fn entries(&self, key: &str) -> Vec<(&String, &Value)> {
        self.doc
            .get(key)
            .and_then(Value::as_object)
            .map(|object| object.iter().collect())
            .unwrap_or_default()
    }

    fn enabled_ids(&self) -> Vec<String> {
        self.entries(ENABLED_KEY)
            .into_iter()
            .filter(|(_, on)| on.as_bool() == Some(true))
            .map(|(id, _)| id.clone())
            .collect()
    }
}

/// One entry of a settings key: set to `value`, or removed when `None`.
pub(crate) struct Edit {
    pub(crate) key: &'static str,
    pub(crate) entry: String,
    pub(crate) action: &'static str,
    pub(crate) value: Option<Value>,
}

impl Edit {
    pub(crate) fn path(&self) -> String {
        format!("{}.{}", self.key, self.entry)
    }

    pub(crate) fn change(&self, settings: &Path) -> Value {
        json!({
            "scope": USER,
            "action": self.action,
            "path": fsx::display(settings),
            "key": self.path(),
        })
    }
}

fn pretty(value: Option<&Value>) -> String {
    value
        .map(|v| serde_json::to_string_pretty(v).unwrap_or_default())
        .unwrap_or_default()
}

fn with_edits(current: Option<&Value>, edits: &[&Edit]) -> Value {
    let mut object = current
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    for edit in edits {
        match &edit.value {
            Some(value) => object.insert(edit.entry.clone(), value.clone()),
            None => object.remove(&edit.entry),
        };
    }
    Value::Object(object)
}

/// One merge step per settings key the edits touch, with the key as it is now and as it will be.
pub(crate) fn edit_steps(settings: &Settings, edits: &[Edit]) -> Vec<Step> {
    let mut steps = Vec::new();
    for key in [MARKETS_KEY, ENABLED_KEY] {
        let mine: Vec<&Edit> = edits.iter().filter(|e| e.key == key).collect();
        if mine.is_empty() {
            continue;
        }
        let before = settings.doc.get(key);
        let after = with_edits(before, &mine);
        let verbs: Vec<String> = mine
            .iter()
            .map(|e| format!("{} {}", e.action, e.entry))
            .collect();
        steps.push(Step::merge(
            &fsx::display(&settings.path),
            verbs.join(", "),
            &[key],
            Diff {
                before: pretty(before),
                after: pretty(Some(&after)),
            },
        ));
    }
    steps
}

/// Writes the edits one key at a time, each from a fresh read so a change made in between is
/// refused by the writer instead of overwritten.
pub(crate) fn write_edits(
    store: &dyn SettingsStore,
    settings: &Settings,
    edits: &[Edit],
) -> Result<(), OpError> {
    for key in [MARKETS_KEY, ENABLED_KEY] {
        let mine: Vec<&Edit> = edits.iter().filter(|e| e.key == key).collect();
        if mine.is_empty() {
            continue;
        }
        let (mut root, text) = store
            .read_settings(&settings.path)
            .map_err(OpError::conflict)?;
        root[key] = with_edits(root.get(key), &mine);
        store
            .write_key(&settings.path, text.as_deref(), &root, key)
            .map_err(OpError::conflict)?;
    }
    Ok(())
}

/// One `claude` invocation: what it does, and the arguments after `claude`.
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
        format!("claude {}", self.args.join(" "))
    }

    fn step(&self) -> Step {
        Step {
            op: "exec",
            path: None,
            detail: self.shown(),
            keys: None,
            diff: None,
        }
    }

    fn change(&self) -> Value {
        let mut argv = vec!["claude".to_string()];
        argv.extend(self.args.iter().cloned());
        json!({"scope": USER, "action": self.action, "command": argv})
    }
}

fn clone_of(env: &Env, tap: &str) -> Result<PathBuf, OpError> {
    valid_tap_name(tap).map_err(OpError::usage)?;
    let entry = load_taps(&env.data_dir)
        .into_iter()
        .find(|t| t.name == tap)
        .ok_or_else(|| OpError::not_found(format!("Tap '{tap}' not found.")))?;
    Ok(tap_dir(&taps_root(&env.data_dir), &entry))
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

fn is_directory_of(source: Option<&Value>, clone: &Path) -> bool {
    source.and_then(|s| s.get("source")).and_then(Value::as_str) == Some("directory")
        && source
            .and_then(|s| s.get("path"))
            .and_then(Value::as_str)
            .is_some_and(|path| fsx::canonical(Path::new(path)) == clone)
}

/// The marketplaces with this clone as their directory: what Claude Code registered
/// (`known_marketplaces.json`) and what the user settings declare.
struct Here {
    known: Vec<String>,
    declared: Vec<String>,
}

impl Here {
    fn names(&self) -> Vec<String> {
        let mut all = self.known.clone();
        for name in &self.declared {
            if !all.contains(name) {
                all.push(name.clone());
            }
        }
        all
    }
}

fn declared_here(env: &Env, settings: &Settings, clone: &Path) -> Here {
    let clone = fsx::canonical(clone);
    let known = match read_json(&env.claude_home.join("plugins/known_marketplaces.json")) {
        Some(Value::Object(known)) => known
            .iter()
            .filter(|(_, meta)| is_directory_of(meta.get("source"), &clone))
            .map(|(name, _)| name.clone())
            .collect(),
        _ => Vec::new(),
    };
    let declared = settings
        .entries(MARKETS_KEY)
        .into_iter()
        .filter(|(_, meta)| is_directory_of(meta.get("source"), &clone))
        .map(|(name, _)| name.clone())
        .collect();
    Here { known, declared }
}

fn declared(env: &Env, settings: &Settings, target: &Target) -> Declared {
    let name = &target.market.name;
    if declared_here(env, settings, &target.clone)
        .names()
        .contains(name)
    {
        return Declared::Here;
    }
    let elsewhere = installed::marketplace_source(&env.claude_home, name).or_else(|| {
        settings
            .entries(MARKETS_KEY)
            .into_iter()
            .find(|(entry, _)| *entry == name)
            .and_then(|(_, meta)| meta.get("source"))
            .and_then(installed::source_text)
    });
    match elsewhere {
        Some(source) => Declared::Elsewhere(source),
        None => Declared::No,
    }
}

fn at_user_scope(plugin: &Installed) -> bool {
    plugin.scope.as_deref().is_none_or(|scope| scope == USER)
}

/// The plugin ids of `marketplace` that are installed at user scope or switched on in the user
/// settings.
fn linked_ids(env: &Env, settings: &Settings, marketplace: &str) -> Vec<String> {
    let suffix = format!("@{marketplace}");
    let mut ids: Vec<String> = installed::file_list(&env.claude_home)
        .into_iter()
        .filter(|p| at_user_scope(p) && p.marketplace.as_deref() == Some(marketplace))
        .map(|p| p.id)
        .collect();
    for id in settings.enabled_ids() {
        if id.ends_with(&suffix) && !ids.contains(&id) {
            ids.push(id);
        }
    }
    ids
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
    Err(OpError::conflict(installed::failure(&out, &shown)))
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

pub(crate) fn note(detail: String) -> Step {
    Step {
        op: "note",
        path: None,
        detail,
        keys: None,
        diff: None,
    }
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

/// The plugins of `chosen` that the organization sync also provides. `claude plugin list` does not
/// show them, so the folders the sync writes (`plugins/synced/<org>_<user>/<plugin>`) are read too.
fn synced_twins(env: &Env, runner: &dyn ClaudeRunner, chosen: &[&Plugin]) -> Vec<String> {
    let mut twins: Vec<String> = installed::list(Some(runner), &env.claude_home)
        .plugins
        .into_iter()
        .filter(|p| p.marketplace.as_deref() == Some(SYNCED))
        .filter(|p| chosen.iter().any(|c| c.name == p.name))
        .map(|p| p.id)
        .collect();
    for account in fsx::list_dir(&env.claude_home.join("plugins").join(SYNCED)) {
        for plugin in chosen {
            let id = format!("{}@{SYNCED}", plugin.name);
            if fsx::is_dir(&account.path.join(&plugin.name)) && !twins.contains(&id) {
                twins.push(id);
            }
        }
    }
    twins
}

fn envelope(
    tap: &str,
    id: &str,
    dry_run: bool,
    plan: PlanV1,
    changed: Vec<String>,
    changes: Vec<Value>,
    extra: Value,
) -> Value {
    let result = (!dry_run).then(|| ResultV1 {
        applied: true,
        changed,
        undo: plan.undo.clone(),
        backups: Vec::new(),
    });
    let mut data = json!({
        "id": id,
        "scope": USER,
        "cwd": null,
        "dryRun": dry_run,
        "plan": plan,
        "result": result,
        "changes": changes,
        "conflicts": [],
        "tap": tap,
    });
    if let (Some(data), Some(extra)) = (data.as_object_mut(), extra.as_object()) {
        data.extend(extra.clone());
    }
    data
}

/// `skills tap link <tap> [--plugin <name>...]`.
pub fn link(
    store: &dyn SettingsStore,
    env: &Env,
    runner: &dyn ClaudeRunner,
    tap: &str,
    only: &[String],
    dry_run: bool,
) -> Result<Value, OpError> {
    let target = resolve(env, tap)?;
    let market = &target.market;
    let chosen = choose(market, only)?;
    let settings = Settings::load(store, env)?;
    let state = declared(env, &settings, &target);
    if let Declared::Elsewhere(source) = &state {
        return Err(OpError::conflict(format!(
            "marketplace '{}' is already declared from {source}; \
             `claude plugin marketplace remove {}` first",
            market.name, market.name
        )));
    }
    let present = linked_ids(env, &settings, &market.name);

    let mut edits: Vec<Edit> = Vec::new();
    if state == Declared::No {
        edits.push(Edit {
            key: MARKETS_KEY,
            entry: market.name.clone(),
            action: "declare",
            value: Some(json!({
                "source": {"source": "directory", "path": fsx::display(&target.clone)}
            })),
        });
    }
    let enabled = settings.doc.get(ENABLED_KEY);
    let mut rows = Vec::new();
    for plugin in &chosen {
        let id = market.id_of(&plugin.name);
        if enabled.and_then(|e| e.get(&id)).and_then(Value::as_bool) != Some(true) {
            edits.push(Edit {
                key: ENABLED_KEY,
                entry: id.clone(),
                action: "enable",
                value: Some(json!(true)),
            });
        }
        rows.push(row(market, plugin, present.contains(&id)));
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

    let mut steps = edit_steps(&settings, &edits);
    if edits.is_empty() {
        steps.push(note(format!(
            "already linked: {} is declared and its plugins are switched on",
            market.name
        )));
    } else {
        steps.push(note(
            "Claude Code installs the switched-on plugins of a declared marketplace when a session starts"
                .to_string(),
        ));
    }
    steps.push(note(
        "updates: `toolportctl skills tap update` pulls the clone and runs `claude plugin update` \
         for what is linked"
            .to_string(),
    ));

    let undo = format!("toolportctl skills tap unlink {tap}");
    let plan = PlanV1 {
        summary: format!(
            "Link tap {tap} to Claude Code as marketplace {}",
            market.name
        ),
        steps,
        effects: Effects::default(),
        warnings,
        undo,
    };
    if !dry_run {
        write_edits(store, &settings, &edits)?;
    }
    let changed = edits
        .iter()
        .map(|e| format!("{} in {}", e.path(), fsx::display(&settings.path)))
        .collect();
    let changes = edits.iter().map(|e| e.change(&settings.path)).collect();
    Ok(envelope(
        tap,
        &market.name,
        dry_run,
        plan,
        changed,
        changes,
        json!({"marketplace": market.name, "plugins": rows}),
    ))
}

/// `skills tap unlink <tap>`: works from what Claude Code and the settings recorded, so it also
/// cleans up after a clone that is gone or a marketplace file that no longer parses.
pub fn unlink(
    store: &dyn SettingsStore,
    env: &Env,
    runner: &dyn ClaudeRunner,
    tap: &str,
    dry_run: bool,
) -> Result<Value, OpError> {
    let clone = clone_of(env, tap)?;
    let settings = Settings::load(store, env)?;
    let here = declared_here(env, &settings, &clone);
    let names = here.names();
    let mut edits: Vec<Edit> = Vec::new();
    for name in &names {
        if here.declared.contains(name) {
            edits.push(Edit {
                key: MARKETS_KEY,
                entry: name.clone(),
                action: "remove",
                value: None,
            });
        }
        let suffix = format!("@{name}");
        for id in settings
            .entries(ENABLED_KEY)
            .into_iter()
            .map(|(id, _)| id)
            .filter(|id| id.ends_with(&suffix))
        {
            edits.push(Edit {
                key: ENABLED_KEY,
                entry: id.clone(),
                action: "remove",
                value: None,
            });
        }
    }
    let commands: Vec<Command> = here
        .known
        .iter()
        .map(|name| {
            Command::new(
                "marketplace remove",
                &["plugin", "marketplace", "remove", name],
            )
        })
        .collect();

    let mut warnings = Vec::new();
    if !commands.is_empty() && !runner.present() {
        warnings.push(format!(
            "{NOT_FOUND}: Claude Code still lists {}; run `{}` when it is installed",
            here.known.join(", "),
            commands[0].shown()
        ));
    }
    warnings.push(RESTART.to_string());
    let mut steps = edit_steps(&settings, &edits);
    steps.extend(commands.iter().map(Command::step));
    if edits.is_empty() && commands.is_empty() {
        steps.push(note(format!(
            "tap {tap} is not linked to Claude Code; nothing to remove"
        )));
    }
    let undo = format!("toolportctl skills tap link {tap}");
    let plan = PlanV1 {
        summary: format!("Unlink tap {tap} from Claude Code"),
        steps,
        effects: Effects::default(),
        warnings,
        undo,
    };
    let mut changed: Vec<String> = edits
        .iter()
        .map(|e| format!("{} in {}", e.path(), fsx::display(&settings.path)))
        .collect();
    if !dry_run {
        write_edits(store, &settings, &edits)?;
        if runner.present() {
            for command in &commands {
                exec(runner, command).map_err(|e| {
                    OpError::conflict(format!(
                        "{} (the settings were already changed; `toolportctl skills tap link {tap}` puts them back)",
                        e.message
                    ))
                })?;
                changed.push(command.shown());
            }
        }
    }
    let mut changes: Vec<Value> = edits.iter().map(|e| e.change(&settings.path)).collect();
    changes.extend(commands.iter().map(Command::change));
    Ok(envelope(
        tap,
        tap,
        dry_run,
        plan,
        changed,
        changes,
        json!({"marketplaces": names}),
    ))
}

/// The taps among `taps` that are linked to Claude Code.
pub fn linked_taps(store: &dyn SettingsStore, env: &Env, taps: &[String]) -> Vec<String> {
    let settings = Settings::lenient(store, env);
    taps.iter()
        .filter(|tap| {
            clone_of(env, tap)
                .is_ok_and(|clone| !declared_here(env, &settings, &clone).names().is_empty())
        })
        .cloned()
        .collect()
}

/// `plugin update` for every linked plugin of `taps` whose clone carries another version than
/// the installed copy. A failure is a row, never an error: the pull already happened.
pub fn refresh(
    store: &dyn SettingsStore,
    env: &Env,
    runner: &dyn ClaudeRunner,
    taps: &[String],
    dry_run: bool,
) -> Vec<Value> {
    let settings = Settings::lenient(store, env);
    let linked: Vec<(String, Vec<String>)> = taps
        .iter()
        .filter_map(|tap| {
            let names = declared_here(env, &settings, &clone_of(env, tap).ok()?).names();
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
                let command =
                    Command::new("update", &["plugin", "update", &plugin.id, "--scope", USER]);
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
pub fn describe(store: &dyn SettingsStore, env: &Env, tap: &str) -> Option<Value> {
    let clone = clone_of(env, tap).ok()?;
    let market = match marketplace::read(&clone)? {
        Ok(market) => market,
        Err(error) => return Some(json!({"error": error})),
    };
    let settings = Settings::lenient(store, env);
    let target = Target { clone, market };
    let present = linked_ids(env, &settings, &target.market.name);
    let plugins: Vec<Value> = target
        .market
        .plugins
        .iter()
        .map(|p| {
            row(
                &target.market,
                p,
                present.contains(&target.market.id_of(&p.name)),
            )
        })
        .collect();
    let declared = match declared(env, &settings, &target) {
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
