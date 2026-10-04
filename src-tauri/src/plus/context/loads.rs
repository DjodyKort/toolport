//! "What loads": which memory layers, rules, settings, MCP servers, skills, commands, agents and
//! plugins a `claude` session gets for a launch profile and working directory, with a token
//! estimate, provenance and the override ("clobber") relations between layers. Pure: it only
//! reads files under [`Roots`]. Every number is `bytes / 4` (`basis: "estimate"`): good for
//! ordering, not a saving; only `context measure` produces a number a screen may call one.

use super::config::{ContextConfig, ProfileSpec};
use super::globs::glob_match;
use super::launch::{parse_selection, read_json_object as read_json, Selection};
use super::layers::{body_of, frontmatter_of, is_managed_local, list_layers, yaml_text};
use super::loads_extra as extra;
use super::measure::{MeasureRun, MeasuredInfo};
use super::roots::Roots;
use crate::plus::sources::model::Origin;
use crate::savings::estimated_tokens;
use serde::Serialize;
use serde_json::{json, Map, Value};
use serde_yaml::Value as Yaml;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct LoadItem {
    pub kind: &'static str,
    pub name: String,
    pub path: Option<String>,
    pub source: &'static str,
    pub loaded: bool,
    pub reason: String,
    pub tokens: u64,
    pub basis: &'static str,
    pub origin: Origin,
    pub writable: bool,
    pub lazy: bool,
    pub via: Vec<String>,
    pub scope: &'static str,
    pub visible: Option<bool>,
}

impl LoadItem {
    pub(super) fn new(
        kind: &'static str,
        name: impl Into<String>,
        path: String,
        source: &'static str,
        reason: impl Into<String>,
        tokens: u64,
    ) -> Self {
        LoadItem {
            kind,
            name: name.into(),
            path: Some(path),
            source,
            loaded: true,
            reason: reason.into(),
            tokens,
            basis: "estimate",
            origin: Origin::new("user", ""),
            writable: false,
            lazy: false,
            via: Vec::new(),
            scope: "always",
            visible: None,
        }
    }

    pub(super) fn from(mut self, origin: Origin, writable: bool) -> Self {
        self.origin = origin;
        self.writable = writable;
        self
    }

    pub(super) fn via(mut self, chain: Vec<String>) -> Self {
        self.via = chain;
        self
    }

    pub(super) fn visible(mut self, accepted: Option<bool>) -> Self {
        self.visible = accepted;
        self
    }

    fn unless(mut self, skip: bool, reason: &str) -> Self {
        if skip {
            self.loaded = false;
            self.reason = reason.into();
        }
        self
    }

    /// Loads only when Claude reads a file the row belongs to: never in the total.
    pub(super) fn on_demand(mut self, reason: impl Into<String>) -> Self {
        self.loaded = false;
        self.lazy = true;
        self.reason = reason.into();
        self
    }

    fn path_scoped(mut self) -> Self {
        self.scope = "paths";
        self.on_demand("path-scoped: loads when a matching file is read")
    }
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Clobber {
    pub kind: &'static str,
    pub key: String,
    pub winner: String,
    pub overridden: Vec<String>,
    pub relation: &'static str,
}

/// Claude Code lists skills up to 1% of the context window; the rest keep only their name.
pub const SKILL_BUDGET_FRACTION: f64 = 0.01;
pub const DEFAULT_CONTEXT_WINDOW: u64 = 200_000;
/// The name of the origin of everything in the user's own Claude folder, as `sources` spells it.
pub(super) const USER_DIR: &str = "~/.claude";

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct SkillBudget {
    pub fraction: f64,
    pub context_window: u64,
    pub limit_tokens: u64,
    pub used_tokens: u64,
    pub capped: Vec<String>,
}

/// Entries in listing order; the first one that does not fit and every one after it is capped.
fn skill_budget(window: u64, listed: &[(String, u64)]) -> SkillBudget {
    let limit = (window as f64 * SKILL_BUDGET_FRACTION) as u64;
    let mut used = 0;
    let mut capped = Vec::new();
    for (name, tokens) in listed {
        if capped.is_empty() && used + tokens <= limit {
            used += tokens;
        } else {
            capped.push(name.clone());
        }
    }
    SkillBudget {
        fraction: SKILL_BUDGET_FRACTION,
        context_window: window,
        limit_tokens: limit,
        used_tokens: used,
        capped,
    }
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct WhatLoads {
    pub profile: Option<String>,
    pub cwd: String,
    pub items: Vec<LoadItem>,
    pub clobbers: Vec<Clobber>,
    pub tokens_by_kind: BTreeMap<String, u64>,
    pub total_tokens: u64,
    pub tokens_lazy: u64,
    pub basis: &'static str,
    pub skill_budget: SkillBudget,
    /// The cached `context measure` as-is run, only when the caller asked for it (`--measured`).
    pub measured: Option<MeasureRun>,
    pub measured_info: Option<MeasuredInfo>,
    pub partial: bool,
    pub notes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compact: Option<super::compact::CompactInfo>,
}

#[derive(Clone, Debug, Default)]
pub struct LoadsOptions {
    pub no_lazy: bool,
    pub context_window: Option<u64>,
}

pub(super) fn text_tokens(text: &str) -> u64 {
    estimated_tokens(text.len() as u64)
}

pub(super) fn show(path: &Path) -> String {
    path.display().to_string()
}

fn memory_source(text: &str) -> &'static str {
    if is_managed_local(text) {
        "client-layer"
    } else {
        "project-local"
    }
}

/// Directories from the filesystem root (or `home`'s parent chain) down to `cwd`, outermost first.
fn ancestors(cwd: &Path) -> Vec<PathBuf> {
    let mut chain: Vec<PathBuf> = cwd.ancestors().map(Path::to_path_buf).collect();
    chain.reverse();
    chain
}

pub(super) fn excluded(settings: &Map<String, Value>, path: &Path) -> bool {
    let target = path.display().to_string();
    settings
        .get("claudeMdExcludes")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .any(|pattern| pattern == target || glob_match(pattern, &target))
        })
        .unwrap_or(false)
}

pub(super) struct Ctx<'a> {
    pub roots: &'a Roots,
    pub config: &'a ContextConfig,
    pub cwd: &'a Path,
    pub spec: Option<&'a ProfileSpec>,
    pub items: Vec<LoadItem>,
    pub clobbers: Vec<Clobber>,
    pub notes: Vec<String>,
    pub partial: bool,
    pub winners: BTreeMap<(&'static str, String), (String, usize)>,
    pub plugin_switches: BTreeMap<String, (bool, String)>,
    pub clients_root: PathBuf,
    pub corp_name: String,
    pub library_name: String,
}

impl Ctx<'_> {
    pub(super) fn push(&mut self, item: LoadItem) {
        self.items.push(item);
    }

    fn clobber(
        &mut self,
        kind: &'static str,
        key: &str,
        winner: &str,
        overridden: &str,
        relation: &'static str,
    ) {
        self.clobbers.push(Clobber {
            kind,
            key: key.to_string(),
            winner: winner.to_string(),
            overridden: vec![overridden.to_string()],
            relation,
        });
    }

    fn override_item(&mut self, index: usize, winner: &str) {
        self.items[index].loaded = false;
        self.items[index].reason = format!("overridden by {winner}");
    }

    /// Call right before pushing the item `name` of `kind` read from `label`: a later scope wins
    /// over the earlier one, which stays listed but unloaded.
    fn claim(&mut self, kind: &'static str, name: &str, label: &str) {
        let key = (kind, name.to_string());
        if let Some((prev, i)) = self.winners.get(&key).cloned() {
            self.clobber(kind, name, label, &prev, "overrides");
            self.override_item(i, label);
        }
        self.winners
            .insert(key, (label.to_string(), self.items.len()));
    }

    fn org_enabled(&self) -> bool {
        self.spec.map(|s| s.org).unwrap_or(true)
    }

    /// Where a file below the working tree comes from: the repository it sits in, a client
    /// repository under the clients root, or nobody Toolport knows (`loose`).
    pub(super) fn project_origin(&self, path: &Path) -> (Origin, bool) {
        let start = path.parent().unwrap_or(path);
        match extra::git_root(start) {
            Some(root) => {
                let name = root
                    .file_name()
                    .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                let kind = if root.starts_with(&self.clients_root) {
                    "client"
                } else {
                    "repo"
                };
                (Origin::new(kind, name), false)
            }
            None => (Origin::new("loose", show(start)), true),
        }
    }
}

fn user_origin() -> Origin {
    Origin::new("user", USER_DIR)
}

fn managed_origin() -> Origin {
    Origin::new("managed", "toolportctl")
}

fn memory(ctx: &mut Ctx, effective_settings: &Map<String, Value>) {
    let user_md = ctx.roots.claude_home.join("CLAUDE.md");
    if let Ok(text) = fs::read_to_string(&user_md) {
        let off = !ctx.org_enabled() || excluded(effective_settings, &user_md);
        let owner = if extra::org_provides_claude_md(ctx) {
            (Origin::new("org", ctx.corp_name.clone()), false)
        } else {
            (Origin::new("loose", USER_DIR), true)
        };
        ctx.push(
            LoadItem::new(
                "memory",
                "CLAUDE.md",
                show(&user_md),
                "org",
                "user memory",
                text_tokens(&text),
            )
            .from(owner.0.clone(), owner.1)
            .unless(off, "excluded by claudeMdExcludes"),
        );
        if !off {
            extra::follow_imports(ctx, &user_md, &text, &[], "org", &owner);
        }
    }
    for dir in ancestors(ctx.cwd) {
        for (rel, local) in [
            ("CLAUDE.md", false),
            (".claude/CLAUDE.md", false),
            ("CLAUDE.local.md", true),
        ] {
            let path = dir.join(rel);
            if path == user_md || path.starts_with(&ctx.roots.claude_home) {
                continue;
            }
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            let off = excluded(effective_settings, &path);
            let source = if local {
                memory_source(&text)
            } else {
                "project"
            };
            let owner = if source == "client-layer" {
                (managed_origin(), false)
            } else {
                ctx.project_origin(&path)
            };
            ctx.push(
                LoadItem::new(
                    "memory",
                    rel,
                    show(&path),
                    source,
                    "directory walk to cwd",
                    text_tokens(&text),
                )
                .from(owner.0.clone(), owner.1)
                .unless(off, "excluded by claudeMdExcludes"),
            );
            if !off {
                extra::follow_imports(ctx, &path, &text, &[], source, &owner);
            }
        }
    }
}

fn rule_files(dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(read) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<(String, PathBuf)> = read
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().map(|e| e == "md").unwrap_or(false))
        .filter_map(|p| {
            let name = p.file_stem()?.to_string_lossy().into_owned();
            Some((name, p))
        })
        .collect();
    files.sort();
    files
}

fn has_paths(text: &str) -> bool {
    let fm = frontmatter_of(text);
    fm.contains_key(Yaml::String("paths".into())) || fm.contains_key(Yaml::String("globs".into()))
}

fn rules(ctx: &mut Ctx) {
    let layers = list_layers(ctx.roots);
    let mut seen: BTreeMap<String, String> = BTreeMap::new();
    let mut scopes: Vec<(&'static str, PathBuf)> =
        vec![("user", ctx.roots.claude_home.join("rules"))];
    for dir in ancestors(ctx.cwd) {
        if dir != ctx.roots.home {
            scopes.push(("project", dir.join(".claude/rules")));
        }
    }
    for (scope, dir) in scopes {
        for (name, path) in rule_files(&dir) {
            let text = fs::read_to_string(&path).unwrap_or_default();
            let scoped = has_paths(&text);
            let layer = layers.iter().any(|l| l.name == name);
            let source = if scope == "project" {
                "project"
            } else if layer {
                if name.starts_with("client-") {
                    "client-layer"
                } else {
                    "personal"
                }
            } else {
                "loose"
            };
            let (origin, writable) = if scope == "project" {
                ctx.project_origin(&path)
            } else if layer {
                (managed_origin(), false)
            } else {
                (Origin::new("loose", USER_DIR), true)
            };
            let label = path.display().to_string();
            if let Some(prev) = seen.get(&name) {
                ctx.clobber("rule", &name, &label, prev, "overrides");
            }
            seen.insert(name.clone(), label);
            let mut row = LoadItem::new(
                "rule",
                name,
                show(&path),
                source,
                "always on",
                text_tokens(&text),
            )
            .from(origin, writable);
            if scoped {
                row = row.path_scoped();
            }
            ctx.push(row);
        }
    }
    if let Some(spec) = ctx.spec {
        if let Ok(Selection::Named(names)) = parse_selection(&spec.rules, "rules") {
            for rule in names {
                let Some(layer) = layers.iter().find(|l| l.name == rule) else {
                    ctx.notes
                        .push(format!("profile rule layer '{rule}' not found"));
                    continue;
                };
                let body = body_of(&layer.path);
                let source = if rule.starts_with("client-") {
                    "client-layer"
                } else {
                    "personal"
                };
                ctx.push(
                    LoadItem::new(
                        "rule",
                        format!("{rule} (append-system-prompt)"),
                        show(&layer.path),
                        source,
                        "profile --append-system-prompt-file",
                        text_tokens(body.trim()),
                    )
                    .from(managed_origin(), false),
                );
            }
        }
    }
}

/// The folders whose `.claude/settings*.json` Claude Code reads for a session started in `cwd`:
/// that folder and the root of the repository around it. Folders above the repository root are
/// not merged (proof F0, run R4), so they are not read here either.
fn project_dirs(cwd: &Path, home: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(root) = extra::git_root(cwd) {
        if root != cwd {
            dirs.push(root);
        }
    }
    dirs.push(cwd.to_path_buf());
    dirs.retain(|d| d != home);
    dirs
}

fn settings_layers(ctx: &Ctx) -> Vec<(String, &'static str, Map<String, Value>)> {
    let mut layers = vec![(
        ctx.roots
            .claude_home
            .join("settings.json")
            .display()
            .to_string(),
        "user",
        read_json(&ctx.roots.claude_home.join("settings.json")),
    )];
    for dir in project_dirs(ctx.cwd, &ctx.roots.home) {
        for (file, source) in [
            ("settings.json", "project"),
            ("settings.local.json", "project-local"),
        ] {
            let path = dir.join(".claude").join(file);
            if path.is_file() {
                layers.push((path.display().to_string(), source, read_json(&path)));
            }
        }
    }
    if let Some(spec) = ctx.spec {
        if !spec.settings_overrides.is_empty() {
            layers.push((
                "profile settings_overrides".into(),
                "profile",
                spec.settings_overrides.clone(),
            ));
        }
    }
    layers
}

/// Objects merge key by key and arrays become a union, as Claude Code combines settings layers;
/// any other value is replaced by the later layer.
fn merge_into(base: &mut Value, over: &Value) {
    match (base, over) {
        (Value::Object(have), Value::Object(add)) => {
            for (key, value) in add {
                match have.get_mut(key) {
                    Some(existing) => merge_into(existing, value),
                    None => {
                        have.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        (Value::Array(have), Value::Array(add)) => {
            for value in add {
                if !have.contains(value) {
                    have.push(value.clone());
                }
            }
        }
        (slot, value) => *slot = value.clone(),
    }
}

fn settings(ctx: &mut Ctx) -> Map<String, Value> {
    let layers = settings_layers(ctx);
    let mut effective = Map::new();
    let mut owner: BTreeMap<String, (String, Value)> = BTreeMap::new();
    for (label, source, map) in &layers {
        if label.ends_with("settings.json") || label.ends_with("settings.local.json") {
            let (origin, writable) = if *source == "user" {
                (user_origin(), true)
            } else {
                ctx.project_origin(Path::new(label))
            };
            ctx.push(
                LoadItem::new(
                    "settings",
                    label.rsplit('/').next().unwrap_or(label),
                    label.clone(),
                    source,
                    "configuration only, no prompt tokens",
                    0,
                )
                .from(origin, writable),
            );
        }
        for (key, value) in map {
            if let Some((prev_label, prev)) = owner.get(key) {
                let merged = (prev.is_array() && value.is_array())
                    || (prev.is_object() && value.is_object());
                if merged || prev != value {
                    let relation = if merged { "merges" } else { "overrides" };
                    ctx.clobber("settings", key, label, prev_label, relation);
                }
            }
            owner.insert(key.clone(), (label.clone(), value.clone()));
            match effective.get_mut(key) {
                Some(existing) => merge_into(existing, value),
                None => {
                    effective.insert(key.clone(), value.clone());
                }
            }
        }
        if let Some(plugins) = map.get("enabledPlugins").and_then(Value::as_object) {
            for (id, on) in plugins {
                if let Some(on) = on.as_bool() {
                    ctx.plugin_switches.insert(id.clone(), (on, label.clone()));
                }
            }
        }
    }
    if !ctx.spec.map(|s| s.org).unwrap_or(true) {
        let org = ctx
            .roots
            .claude_home
            .join("CLAUDE.md")
            .display()
            .to_string();
        let entry = json!(org);
        let mut ex = effective
            .get("claudeMdExcludes")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if !ex.contains(&entry) {
            ex.push(entry);
        }
        effective.insert("claudeMdExcludes".into(), Value::Array(ex));
    }
    effective
}

fn servers_of(map: &Map<String, Value>) -> Map<String, Value> {
    match map.get("mcpServers") {
        Some(Value::Object(m)) => m.clone(),
        _ => Map::new(),
    }
}

fn mcp(ctx: &mut Ctx, legacy_names: &[String]) {
    let claude_json = read_json(&ctx.roots.claude_json);
    let user = servers_of(&claude_json);
    let mut scopes: Vec<(String, &'static str, Map<String, Value>)> = Vec::new();
    let strict = ctx.spec.is_some();
    match ctx.spec.map(|s| parse_selection(&s.servers, "servers")) {
        Some(Ok(Selection::None)) => {}
        Some(Ok(Selection::Named(names))) => {
            let mut picked = Map::new();
            for n in names {
                match user.get(&n) {
                    Some(v) => {
                        picked.insert(n, v.clone());
                    }
                    None => ctx.notes.push(format!("profile server '{n}' not found")),
                }
            }
            scopes.push((ctx.roots.claude_json.display().to_string(), "user", picked));
        }
        _ => scopes.push((ctx.roots.claude_json.display().to_string(), "user", user)),
    }
    if strict {
        ctx.notes.push(
            "--strict-mcp-config: project .mcp.json and per-project servers are ignored".into(),
        );
    } else {
        for dir in ancestors(ctx.cwd) {
            let path = dir.join(".mcp.json");
            if path.is_file() {
                scopes.push((
                    path.display().to_string(),
                    "project",
                    servers_of(&read_json(&path)),
                ));
            }
        }
        let projects = claude_json.get("projects").and_then(Value::as_object);
        if let Some(entry) = projects.and_then(|p| p.get(&ctx.cwd.display().to_string())) {
            if let Some(obj) = entry.as_object() {
                scopes.push((
                    format!("{} [project]", ctx.roots.claude_json.display()),
                    "project-local",
                    servers_of(obj),
                ));
            }
        }
    }
    for (label, source, servers) in scopes {
        for (name, entry) in servers {
            let text = serde_json::to_string(&entry).unwrap_or_default();
            ctx.claim("mcp", &name, &label);
            let org = legacy_names.contains(&name);
            let (origin, writable) = if org {
                (Origin::new("org", ctx.corp_name.clone()), false)
            } else if source == "user" {
                (user_origin(), true)
            } else {
                ctx.project_origin(Path::new(label.trim_end_matches(" [project]")))
            };
            ctx.push(
                LoadItem::new(
                    "mcp",
                    name,
                    label.clone(),
                    if org { "org" } else { source },
                    "server definition; tool schemas are not included in the estimate",
                    text_tokens(&text),
                )
                .from(origin, writable),
            );
        }
    }
}

pub(super) fn skill_dirs(dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(read) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<(String, PathBuf)> = read
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("SKILL.md").is_file())
        .filter_map(|p| Some((p.file_name()?.to_string_lossy().into_owned(), p)))
        .collect();
    out.sort();
    out
}

fn skills(ctx: &mut Ctx) {
    let canonical_root = ctx.roots.skills_repo_path();
    let mut scopes: Vec<(&'static str, PathBuf)> =
        vec![("user", ctx.roots.claude_home.join("skills"))];
    for dir in ancestors(ctx.cwd) {
        if dir != ctx.roots.home {
            scopes.push(("project", dir.join(".claude/skills")));
        }
    }
    for (scope, dir) in scopes {
        for (dirname, path) in skill_dirs(&dir) {
            let skill_md = path.join("SKILL.md");
            let text = fs::read_to_string(&skill_md).unwrap_or_default();
            let fm = frontmatter_of(&text);
            let get = |k: &str| fm.get(Yaml::String(k.into())).map(yaml_text);
            let name = get("name").unwrap_or(dirname);
            let description = get("description").unwrap_or_default();
            ctx.claim("skill", &name, &show(&path));
            let managed = scope == "user" && canonical_root.join("skills").join(&name).is_dir();
            let (source, origin, writable) = if scope == "project" {
                let (origin, writable) = ctx.project_origin(&skill_md);
                ("project", origin, writable)
            } else if managed {
                (
                    "personal",
                    Origin::new("library", ctx.library_name.clone()),
                    true,
                )
            } else if extra::carries_managed_marker(&text) {
                ("personal", managed_origin(), false)
            } else {
                (
                    "loose",
                    Origin::new("loose", USER_DIR),
                    true,
                )
            };
            let tokens = text_tokens(&format!("{name}: {description}"));
            let verdict = crate::plus::skills::frontmatter::frontmatter_accepted(&text);
            let mut row = LoadItem::new(
                "skill",
                name,
                show(&skill_md),
                source,
                "name and description load at start; body on demand",
                tokens,
            )
            .from(origin, writable)
            .visible(Some(verdict.is_ok()));
            if let Err(why) = verdict {
                row = row.unless(
                    true,
                    &format!("Claude Code does not list this skill: {why}"),
                );
            }
            ctx.push(row);
        }
    }
}

/// Computes what loads for `profile` (a key of `config.profiles`, or `None` for the plain
/// default launch) when `claude` starts in `cwd`.
pub fn what_loads(
    roots: &Roots,
    config: &ContextConfig,
    profile: Option<&str>,
    cwd: &Path,
) -> Result<WhatLoads, String> {
    what_loads_with(roots, config, profile, cwd, &LoadsOptions::default())
}

pub fn what_loads_with(
    roots: &Roots,
    config: &ContextConfig,
    profile: Option<&str>,
    cwd: &Path,
    options: &LoadsOptions,
) -> Result<WhatLoads, String> {
    let spec = match profile {
        Some(name) => Some(
            config
                .profiles
                .get(name)
                .ok_or_else(|| format!("unknown profile: {name}"))?,
        ),
        None => None,
    };
    let corp_clone = roots.resolve_corp_tools_dir(config);
    let mut ctx = Ctx {
        roots,
        config,
        cwd,
        spec,
        items: Vec::new(),
        clobbers: Vec::new(),
        notes: Vec::new(),
        partial: false,
        winners: BTreeMap::new(),
        plugin_switches: BTreeMap::new(),
        clients_root: roots.resolve_clients_root(config),
        corp_name: extra::dir_name(&corp_clone),
        library_name: extra::dir_name(&roots.skills_repo_path()),
    };
    if let Some(spec) = spec {
        for (field, on) in [
            ("commands", spec.commands),
            ("skills", spec.skills),
            ("agents", spec.agents),
        ] {
            if !on {
                ctx.notes.push(format!(
                    "{field}=false cannot be enforced by a launch profile; still counted"
                ));
            }
        }
    }
    if let Some(spec) = spec {
        for w in super::compact::warnings(roots, profile.unwrap_or(""), spec) {
            ctx.notes.push(w);
        }
    }
    let effective = settings(&mut ctx);
    memory(&mut ctx, &effective);
    extra::memory_index(&mut ctx);
    rules(&mut ctx);
    mcp(&mut ctx, &config.dedupe.legacy_names);
    skills(&mut ctx);
    extra::commands_and_agents(&mut ctx);
    let found = extra::scan_plugins(&mut ctx);
    let plugin_skills = extra::plugins(&mut ctx, &found);
    if !options.no_lazy {
        extra::nested(&mut ctx, &effective);
    }
    if options.no_lazy {
        ctx.items.retain(|i| !i.lazy);
    }

    let mut listed: Vec<(String, u64)> = ctx
        .items
        .iter()
        .filter(|i| i.kind == "skill" && i.loaded && i.visible != Some(false))
        .map(|i| (i.name.clone(), i.tokens))
        .collect();
    listed.extend(plugin_skills);
    let budget = skill_budget(options.context_window.unwrap_or(DEFAULT_CONTEXT_WINDOW), &listed);
    if !budget.capped.is_empty() {
        ctx.notes.push(format!(
            "the skill list is over its budget ({} of {} tokens): {} skills lose their description; which ones is a guess until `context measure` counts them",
            budget.used_tokens,
            budget.limit_tokens,
            budget.capped.len()
        ));
    }
    let mut by_kind: BTreeMap<String, u64> = BTreeMap::new();
    let mut total = 0;
    let mut lazy = 0;
    for item in &ctx.items {
        if item.loaded {
            *by_kind.entry(item.kind.to_string()).or_default() += item.tokens;
            total += item.tokens;
        } else if item.lazy {
            lazy += item.tokens;
        }
    }
    Ok(WhatLoads {
        profile: profile.map(String::from),
        cwd: cwd.display().to_string(),
        items: ctx.items,
        clobbers: ctx.clobbers,
        tokens_by_kind: by_kind,
        total_tokens: total,
        tokens_lazy: lazy,
        basis: "estimate",
        skill_budget: budget,
        measured: None,
        measured_info: None,
        partial: ctx.partial,
        notes: ctx.notes,
        compact: spec.and_then(|s| super::compact::info(roots, s)),
    })
}
