//! "What loads": which memory layers, rules, settings, MCP servers and skills a `claude` session
//! gets for a launch profile and working directory, with a token estimate, provenance and the
//! override ("clobber") relations between layers. Pure: it only reads files under [`Roots`].

use super::config::{ContextConfig, ProfileSpec};
use super::launch::{parse_selection, read_json_object as read_json, Selection};
use super::layers::{body_of, frontmatter, list_layers, yaml_text, MANAGED_LOCAL_HEADER};
use super::roots::Roots;
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
}

impl LoadItem {
    fn new(
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
        }
    }

    fn unless(mut self, skip: bool, reason: &str) -> Self {
        if skip {
            self.loaded = false;
            self.reason = reason.into();
        }
        self
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

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct WhatLoads {
    pub profile: Option<String>,
    pub cwd: String,
    pub items: Vec<LoadItem>,
    pub clobbers: Vec<Clobber>,
    pub tokens_by_kind: BTreeMap<String, u64>,
    pub total_tokens: u64,
    pub notes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compact: Option<super::compact::CompactInfo>,
}

fn text_tokens(text: &str) -> u64 {
    estimated_tokens(text.len() as u64)
}

fn show(path: &Path) -> String {
    path.display().to_string()
}

fn memory_source(text: &str) -> &'static str {
    if text.contains(MANAGED_LOCAL_HEADER) {
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

fn excluded(settings: &Map<String, Value>, path: &Path) -> bool {
    let target = path.display().to_string();
    settings
        .get("claudeMdExcludes")
        .and_then(Value::as_array)
        .map(|items| items.iter().any(|v| v.as_str() == Some(target.as_str())))
        .unwrap_or(false)
}

struct Ctx<'a> {
    roots: &'a Roots,
    cwd: &'a Path,
    spec: Option<&'a ProfileSpec>,
    items: Vec<LoadItem>,
    clobbers: Vec<Clobber>,
    notes: Vec<String>,
}

impl Ctx<'_> {
    fn push(&mut self, item: LoadItem) {
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

    fn org_enabled(&self) -> bool {
        self.spec.map(|s| s.org).unwrap_or(true)
    }
}

fn memory(ctx: &mut Ctx, effective_settings: &Map<String, Value>) {
    let user_md = ctx.roots.claude_home.join("CLAUDE.md");
    if let Ok(text) = fs::read_to_string(&user_md) {
        let off = !ctx.org_enabled() || excluded(effective_settings, &user_md);
        ctx.push(
            LoadItem::new(
                "memory",
                "CLAUDE.md",
                show(&user_md),
                "org",
                "user memory",
                text_tokens(&text),
            )
            .unless(off, "excluded by claudeMdExcludes"),
        );
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
            ctx.push(
                LoadItem::new(
                    "memory",
                    rel,
                    show(&path),
                    source,
                    "directory walk to cwd",
                    text_tokens(&text),
                )
                .unless(off, "excluded by claudeMdExcludes"),
            );
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

fn has_paths(path: &Path) -> bool {
    let fm = frontmatter(path);
    fm.contains_key(Yaml::String("paths".into())) || fm.contains_key(Yaml::String("globs".into()))
}

fn rules(ctx: &mut Ctx) {
    let canonical: Vec<String> = list_layers(ctx.roots).into_iter().map(|l| l.name).collect();
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
            let scoped = has_paths(&path);
            let source = if scope == "project" {
                "project"
            } else if canonical.contains(&name) {
                if name.starts_with("client-") {
                    "client-layer"
                } else {
                    "personal"
                }
            } else {
                "org"
            };
            let label = path.display().to_string();
            if let Some(prev) = seen.get(&name) {
                ctx.clobber("rule", &name, &label, prev, "overrides");
            }
            seen.insert(name.clone(), label);
            ctx.push(
                LoadItem::new(
                    "rule",
                    name,
                    show(&path),
                    source,
                    "always on",
                    text_tokens(&text),
                )
                .unless(scoped, "path-scoped: loads when a matching file is read"),
            );
        }
    }
    if let Some(spec) = ctx.spec {
        if let Ok(Selection::Named(names)) = parse_selection(&spec.rules, "rules") {
            let layers = list_layers(ctx.roots);
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
                ctx.push(LoadItem::new(
                    "rule",
                    format!("{rule} (append-system-prompt)"),
                    show(&layer.path),
                    source,
                    "profile --append-system-prompt-file",
                    text_tokens(body.trim()),
                ));
            }
        }
    }
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
    for dir in ancestors(ctx.cwd) {
        if dir == ctx.roots.home {
            continue;
        }
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

fn settings(ctx: &mut Ctx) -> Map<String, Value> {
    let layers = settings_layers(ctx);
    let mut effective = Map::new();
    let mut owner: BTreeMap<String, (String, Value)> = BTreeMap::new();
    for (label, source, map) in &layers {
        if label.ends_with("settings.json") || label.ends_with("settings.local.json") {
            ctx.push(LoadItem::new(
                "settings",
                label.rsplit('/').next().unwrap_or(label),
                label.clone(),
                source,
                "configuration only, no prompt tokens",
                0,
            ));
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
            effective.insert(key.clone(), value.clone());
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
    let mut winner: BTreeMap<String, (String, usize)> = BTreeMap::new();
    for (label, source, servers) in scopes {
        for (name, entry) in servers {
            let text = serde_json::to_string(&entry).unwrap_or_default();
            if let Some((prev, i)) = winner.get(&name).cloned() {
                ctx.clobber("mcp", &name, &label, &prev, "overrides");
                ctx.override_item(i, &label);
            }
            winner.insert(name.clone(), (label.clone(), ctx.items.len()));
            let org = legacy_names.contains(&name);
            ctx.push(LoadItem::new(
                "mcp",
                name,
                label.clone(),
                if org { "org" } else { source },
                "server definition; tool schemas are not included in the estimate",
                text_tokens(&text),
            ));
        }
    }
}

fn skill_dirs(dir: &Path) -> Vec<(String, PathBuf)> {
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
    let mut winner: BTreeMap<String, (String, usize)> = BTreeMap::new();
    for (scope, dir) in scopes {
        for (dirname, path) in skill_dirs(&dir) {
            let skill_md = path.join("SKILL.md");
            let fm = frontmatter(&skill_md);
            let get = |k: &str| fm.get(Yaml::String(k.into())).map(yaml_text);
            let name = get("name").unwrap_or(dirname);
            let description = get("description").unwrap_or_default();
            let label = path.display().to_string();
            if let Some((prev, i)) = winner.get(&name).cloned() {
                ctx.clobber("skill", &name, &label, &prev, "overrides");
                ctx.override_item(i, &label);
            }
            winner.insert(name.clone(), (label, ctx.items.len()));
            let managed = scope == "user" && canonical_root.join("skills").join(&name).is_dir();
            let source = if scope == "project" {
                "project"
            } else if managed {
                "personal"
            } else {
                "org"
            };
            let tokens = text_tokens(&format!("{name}: {description}"));
            ctx.push(LoadItem::new(
                "skill",
                name,
                show(&skill_md),
                source,
                "name and description load at start; body on demand",
                tokens,
            ));
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
    let spec = match profile {
        Some(name) => Some(
            config
                .profiles
                .get(name)
                .ok_or_else(|| format!("unknown profile: {name}"))?,
        ),
        None => None,
    };
    let mut ctx = Ctx {
        roots,
        cwd,
        spec,
        items: Vec::new(),
        clobbers: Vec::new(),
        notes: Vec::new(),
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
    rules(&mut ctx);
    mcp(&mut ctx, &config.dedupe.legacy_names);
    skills(&mut ctx);

    let mut by_kind: BTreeMap<String, u64> = BTreeMap::new();
    let mut total = 0;
    for item in ctx.items.iter().filter(|i| i.loaded) {
        *by_kind.entry(item.kind.to_string()).or_default() += item.tokens;
        total += item.tokens;
    }
    Ok(WhatLoads {
        profile: profile.map(String::from),
        cwd: cwd.display().to_string(),
        items: ctx.items,
        clobbers: ctx.clobbers,
        tokens_by_kind: by_kind,
        total_tokens: total,
        notes: ctx.notes,
        compact: spec.and_then(|s| super::compact::info(roots, s)),
    })
}
