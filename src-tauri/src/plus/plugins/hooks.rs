//! `hooks ls`: every hook Claude Code would start in a folder, read from files. A hook is never
//! run and a plugin is never started; commands are text here.

use super::installed;
use super::manifest;
use super::settings::{Layers, Scope};
use super::Env;
use regex::Regex;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const TOOLS: [&str; 4] = ["Bash", "Edit", "Write", "Read"];
pub const OWNER_KINDS: [&str; 7] = [
    "plugin", "user", "project", "local", "skill", "toolport", "managed",
];
pub const CONFLICT_NOTE: &str = "strictest answer wins (deny, then defer, then ask, then allow)";
const LABEL_EXTS: [&str; 7] = [".js", ".sh", ".py", ".ts", ".mjs", ".cjs", ".rb"];
const SKILL_SECTIONS: [&str; 4] = ["skills", "rules", "agents", "styles"];

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub owner_kind: &'static str,
    pub owner_name: String,
    pub event: String,
    pub matcher: String,
    pub kind: String,
    pub command: String,
    pub runtime_id: Option<String>,
    pub timeout_sec: Option<u64>,
    pub is_async: bool,
    pub source: String,
    pub marker: Option<String>,
    pub switch_method: &'static str,
    pub switch_detail: String,
    pub tools: Vec<&'static str>,
    /// False for hooks that only run under a launch profile, or from a plugin that is off.
    pub active: bool,
}

impl Entry {
    pub fn owner(&self) -> String {
        format!("{}:{}", self.owner_kind, self.owner_name)
    }

    pub fn to_json(&self) -> Value {
        json!({
            "owner": {"kind": self.owner_kind, "name": self.owner_name},
            "event": self.event,
            "matcher": self.matcher,
            "type": self.kind,
            "command": self.command,
            "runtimeId": self.runtime_id,
            "timeoutSec": self.timeout_sec,
            "async": self.is_async,
            "source": self.source,
            "marker": self.marker,
            "switch": {"method": self.switch_method, "detail": self.switch_detail},
            "tools": self.tools,
            "active": self.active,
        })
    }

    fn is_tool_event(&self) -> bool {
        matches!(self.event.as_str(), "PreToolUse" | "PostToolUse")
    }

    /// A short name for the entry: the script and its first plain argument.
    pub fn label(&self) -> String {
        format!("{}: {}", self.owner_name, short_command(&self.command))
    }
}

/// Whether a PreToolUse or PostToolUse matcher fires for `tool`: `*` and empty match everything, a
/// matcher of letters, digits, `_`, `-`, spaces, `,` and `|` is an exact list, anything else is an
/// unanchored regular expression.
pub fn matches_tool(matcher: &str, tool: &str) -> Result<bool, String> {
    let m = matcher.trim();
    if m.is_empty() || m == "*" {
        return Ok(true);
    }
    if m.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | ' ' | ',' | '|'))
    {
        return Ok(m.split(['|', ',']).any(|part| part.trim() == tool));
    }
    Regex::new(m)
        .map(|re| re.is_match(tool))
        .map_err(|e| e.to_string())
}

fn tokens(command: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut started = false;
    let mut chars = command.chars().peekable();
    while let Some(c) = chars.next() {
        match quote {
            Some(q) if c == q => quote = None,
            Some('"') if c == '\\' => {
                if let Some(next) = chars.next() {
                    cur.push(next);
                }
            }
            Some(_) => cur.push(c),
            None if c == '"' || c == '\'' => {
                quote = Some(c);
                started = true;
            }
            None if c.is_whitespace() => {
                if started || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    started = false;
                }
            }
            None => cur.push(c),
        }
    }
    if started || !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn short_command(command: &str) -> String {
    let toks: Vec<String> = tokens(command)
        .into_iter()
        .skip_while(|t| t.contains('=') && !t.starts_with('-') && !t.contains('/'))
        .collect();
    let plain = |t: &str| !t.is_empty() && !t.chars().any(char::is_whitespace);
    let script = toks.iter().position(|t| {
        plain(t) && !t.starts_with('-') && (t.contains('/') || LABEL_EXTS.iter().any(|e| t.ends_with(e)))
    });
    let Some(at) = script else {
        return toks.first().cloned().unwrap_or_default();
    };
    let base = toks[at].rsplit('/').next().unwrap_or(&toks[at]).to_string();
    let arg = toks[at + 1..]
        .first()
        .filter(|t| plain(t) && !t.starts_with('-') && !t.contains('/') && !t.contains('='));
    match arg {
        Some(arg) => format!("{base} {arg}"),
        None => base,
    }
}

/// The command as shown: home shortened to `~`, the values of leading `NAME=value` assignments
/// dropped, live secrets masked.
pub fn command_text(command: &str, home: &Path) -> String {
    let mut rest = command.trim_start();
    let mut prefix = String::new();
    loop {
        let Some(eq) = rest.find('=') else { break };
        let name = &rest[..eq];
        if name.is_empty()
            || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            || name.starts_with(|c: char| c.is_ascii_digit())
        {
            break;
        }
        let value = &rest[eq + 1..];
        let end = match value.chars().next() {
            Some(q @ ('"' | '\'')) => value[1..].find(q).map(|i| i + 2).unwrap_or(value.len()),
            _ => value.find(char::is_whitespace).unwrap_or(value.len()),
        };
        prefix.push_str(&format!("{name}=… "));
        rest = value[end..].trim_start();
    }
    let mut text = format!("{prefix}{}", mask_command_line(rest))
        .trim_end()
        .to_string();
    let home = home.to_string_lossy();
    if home.len() > 1 {
        text = text.replace(home.as_ref(), "~");
    }
    crate::plus::redact::scrub_text(text)
}

fn quote_chars(text: &str) -> &str {
    text.trim_start_matches(['"', '\''])
}

fn trailing_quotes(text: &str) -> &str {
    &text[text.trim_end_matches(['"', '\'']).len()..]
}

/// The words of a command line with the whitespace in front of each, quoted runs kept whole.
fn words(text: &str) -> Vec<(&str, &str)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < text.len() {
        let gap = i;
        while let Some(c) = text[i..].chars().next().filter(|c| c.is_whitespace()) {
            i += c.len_utf8();
        }
        let start = i;
        let mut quote: Option<char> = None;
        while let Some(c) = text[i..].chars().next() {
            match quote {
                Some(q) if c == q => quote = None,
                Some(_) => {}
                None if c == '"' || c == '\'' => quote = Some(c),
                None if c.is_whitespace() => break,
                None => {}
            }
            i += c.len_utf8();
        }
        out.push((&text[gap..start], &text[start..i]));
    }
    out
}

fn sensitive_assignment(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [
        "token", "secret", "pass", "key", "auth", "credential", "cookie", "session",
    ]
    .iter()
    .any(|s| lower.contains(s))
}

/// Values that look like credentials replaced: `--api-key=x`, `--token x`, `Authorization: x`,
/// `Bearer x`, `NAME_KEY=x` anywhere in the line and the userinfo and query of URLs. This is a
/// best effort on free text; the hooks file itself is never rewritten.
fn mask_command_line(text: &str) -> String {
    let mask = manifest::MASK;
    let mut out = String::new();
    let mut hide_next = false;
    for (gap, word) in words(text) {
        out.push_str(gap);
        if std::mem::take(&mut hide_next) {
            let plain = word.trim_matches(['"', '\'']);
            if plain.eq_ignore_ascii_case("bearer") || plain.eq_ignore_ascii_case("basic") {
                out.push_str(word);
                hide_next = true;
                continue;
            }
            let opened = word.chars().next().filter(|c| matches!(c, '"' | '\''));
            let tail = if opened.is_some() {
                ""
            } else {
                trailing_quotes(word)
            };
            out.push_str(mask);
            out.push_str(tail);
            continue;
        }
        if let Some(q) = word.chars().next().filter(|c| matches!(c, '"' | '\'')) {
            if word.len() > 1 && word.ends_with(q) {
                out.push(q);
                out.push_str(&mask_command_line(&word[1..word.len() - 1]));
                out.push(q);
                continue;
            }
        }
        let bare = quote_chars(word);
        let lead = &word[..word.len() - bare.len()];
        if let Some((name, value)) = bare.split_once('=') {
            let flag = name.starts_with('-') && manifest::secret_name(name);
            let var = !name.starts_with('-')
                && !name.is_empty()
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                && sensitive_assignment(name);
            if flag || var {
                let tail = if value.starts_with(['"', '\'']) {
                    ""
                } else {
                    trailing_quotes(value)
                };
                out.push_str(&format!("{lead}{name}={mask}{tail}"));
                continue;
            }
        }
        if bare.starts_with('-') && !bare.contains('=') && manifest::secret_name(bare) {
            hide_next = true;
        } else if bare.ends_with(':') && manifest::secret_name(bare) {
            hide_next = true;
        } else if bare.trim_end_matches(['"', '\'']).eq_ignore_ascii_case("bearer")
            || bare.trim_end_matches(['"', '\'']).eq_ignore_ascii_case("basic")
        {
            hide_next = true;
        }
        out.push_str(&clean_urls(word));
    }
    out
}

fn clean_urls(word: &str) -> String {
    let Some(at) = word.find("://") else {
        return word.to_string();
    };
    let start = word[..at]
        .rfind(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')))
        .map(|p| p + 1)
        .unwrap_or(0);
    let end = word[at..]
        .find(['"', '\'', '`'])
        .map(|p| at + p)
        .unwrap_or(word.len());
    let url = &word[start..end];
    let query = url.contains(['?', '#']);
    let cleaned = manifest::clean_url(url);
    if cleaned == url {
        return word.to_string();
    }
    format!(
        "{}{cleaned}{}{}",
        &word[..start],
        if query { "?…" } else { "" },
        &word[end..]
    )
}

struct Raw {
    event: String,
    matcher: String,
    handler: Value,
    key: String,
}

/// The `{Event: [{matcher, hooks: [...]}]}` object of a settings or hooks document.
fn events_of(doc: &Value, bare_ok: bool) -> Option<&Map<String, Value>> {
    match doc.get("hooks") {
        Some(Value::Object(map)) => Some(map),
        Some(_) => None,
        None if bare_ok => doc
            .as_object()
            .filter(|m| !m.is_empty() && m.values().all(Value::is_array)),
        None => None,
    }
}

fn raw_entries(events: &Map<String, Value>) -> Vec<Raw> {
    let mut out = Vec::new();
    for (event, groups) in events {
        for (i, group) in groups.as_array().into_iter().flatten().enumerate() {
            let matcher = group
                .get("matcher")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            match group.get("hooks").and_then(Value::as_array) {
                Some(handlers) => {
                    for (j, handler) in handlers.iter().enumerate().filter(|(_, h)| h.is_object()) {
                        out.push(Raw {
                            event: event.clone(),
                            matcher: matcher.clone(),
                            handler: handler.clone(),
                            key: format!("hooks.{event}[{i}].hooks[{j}]"),
                        });
                    }
                }
                None if group.get("command").is_some() => out.push(Raw {
                    event: event.clone(),
                    matcher: matcher.clone(),
                    handler: group.clone(),
                    key: format!("hooks.{event}[{i}]"),
                }),
                None => {}
            }
        }
    }
    out
}

fn handler_text(handler: &Value) -> String {
    ["command", "url", "prompt", "tool"]
        .iter()
        .find_map(|k| handler.get(*k).and_then(Value::as_str))
        .unwrap_or_default()
        .to_string()
}

#[derive(Clone, Debug, Default)]
struct Ledger(BTreeMap<String, String>);

impl Ledger {
    fn load(data_dir: Option<&Path>) -> Self {
        let mut map = BTreeMap::new();
        let doc = data_dir
            .map(|d| d.join(crate::plus::skills::lock::LOCKFILE_NAME))
            .and_then(|p| manifest::read_doc(&p));
        for section in SKILL_SECTIONS {
            let Some(items) = doc.as_ref().and_then(|d| d.get(section)).and_then(Value::as_object)
            else {
                continue;
            };
            for (name, entry) in items {
                let commands = entry
                    .get("hooks_installed")
                    .and_then(|h| h.get("claude-code"))
                    .and_then(Value::as_array);
                for command in commands.into_iter().flatten().filter_map(Value::as_str) {
                    map.entry(command.trim().to_string())
                        .or_insert_with(|| name.clone());
                }
            }
        }
        Self(map)
    }

    fn skill_for(&self, command: &str) -> Option<&str> {
        let command = command.trim();
        self.0
            .get(command)
            .or_else(|| command.split_whitespace().next().and_then(|c| self.0.get(c)))
            .map(String::as_str)
    }
}

struct Ctx<'a> {
    home: &'a Path,
    warnings: Vec<String>,
}

impl Ctx<'_> {
    fn tools(&mut self, event: &str, matcher: &str, label: &str) -> Vec<&'static str> {
        if !matches!(event, "PreToolUse" | "PostToolUse") {
            return Vec::new();
        }
        let mut out = Vec::new();
        for tool in TOOLS {
            match matches_tool(matcher, tool) {
                Ok(true) => out.push(tool),
                Ok(false) => {}
                Err(e) => {
                    let note = format!(
                        "matcher '{matcher}' of {label} is not a valid pattern ({}); counted as matching no tool",
                        e.lines().last().unwrap_or("invalid").trim()
                    );
                    if !self.warnings.contains(&note) {
                        self.warnings.push(note);
                    }
                    return Vec::new();
                }
            }
        }
        out
    }

    fn entry(
        &mut self,
        raw: &Raw,
        file: &Path,
        owner: (&'static str, String),
        how: (&'static str, String),
        marker: Option<String>,
        active: bool,
    ) -> Entry {
        let command = handler_text(&raw.handler);
        let text = command_text(&command, self.home);
        let tools = self.tools(&raw.event, &raw.matcher, &format!("{} {}", owner.1, short_command(&command)));
        Entry {
            owner_kind: owner.0,
            owner_name: owner.1,
            event: raw.event.clone(),
            matcher: raw.matcher.clone(),
            kind: raw
                .handler
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("command")
                .to_string(),
            command: text,
            runtime_id: None,
            timeout_sec: raw.handler.get("timeout").and_then(Value::as_u64),
            is_async: raw.handler.get("async").and_then(Value::as_bool) == Some(true),
            source: format!("{}#{}", file.display(), raw.key),
            marker,
            switch_method: how.0,
            switch_detail: how.1,
            tools,
            active,
        }
    }
}

fn settings_entries(
    ctx: &mut Ctx,
    ledger: &Ledger,
    file: &super::settings::SettingsFile,
) -> Vec<Entry> {
    let Some(events) = file.doc.as_ref().and_then(|d| events_of(d, false)) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for raw in raw_entries(events) {
        let command = handler_text(&raw.handler);
        let toolport = [crate::hooks::HOOK_MARKER, crate::agent_guard::GUARD_MARKER]
            .into_iter()
            .find(|m| command.contains(m));
        let (owner, how, marker) = if let Some(marker) = toolport {
            (
                ("toolport", "toolport".to_string()),
                ("toolport-command", "installed by Toolport".to_string()),
                Some(marker.to_string()),
            )
        } else if let Some(skill) = ledger.skill_for(&command) {
            (
                ("skill", skill.to_string()),
                (
                    "skills-sync",
                    "written by skills sync; change the skill's hooks and sync again".to_string(),
                ),
                Some("hooks_installed".to_string()),
            )
        } else if file.scope == Scope::Managed {
            (
                ("managed", file.name.to_string()),
                ("none", "managed settings cannot be changed here".to_string()),
                None,
            )
        } else {
            (
                (file.scope.as_str(), file.name.to_string()),
                ("edit-settings", format!("edit {} or remove the entry", file.name)),
                None,
            )
        };
        out.push(ctx.entry(&raw, &file.path, owner, how, marker, true));
    }
    out
}

/// The hooks a plugin brings, from its hooks files. `active` says whether the plugin is on.
pub fn plugin_entries(
    home: &Path,
    id: &str,
    install: &Path,
    manifest_doc: &Value,
    active: bool,
    warnings: &mut Vec<String>,
) -> Vec<Entry> {
    let mut ctx = Ctx {
        home,
        warnings: Vec::new(),
    };
    let mut out = Vec::new();
    for piece in manifest::hook_docs(install, manifest_doc) {
        let Some(events) = events_of(&piece.value, true) else {
            continue;
        };
        for raw in raw_entries(events) {
            out.push(ctx.entry(
                &raw,
                &piece.file,
                ("plugin", id.to_string()),
                (
                    "disable-plugin",
                    format!("turn the plugin off with `claude plugin disable {id}`; a switched-off hook still starts a process"),
                ),
                None,
                active,
            ));
        }
    }
    warnings.append(&mut ctx.warnings);
    out
}

#[derive(Clone, Debug)]
pub struct Inventory {
    pub cwd: Option<PathBuf>,
    pub disabled_all: bool,
    pub entries: Vec<Entry>,
    pub warnings: Vec<String>,
}

fn launch_profile_entries(ctx: &mut Ctx, profiles_root: &Path, known: &[Entry]) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    let mut dirs: Vec<_> = crate::plus::sources::fsx::list_dir(profiles_root)
        .into_iter()
        .filter(|e| e.kind == crate::plus::sources::fsx::Kind::Dir)
        .collect();
    dirs.sort_by(|a, b| a.name.cmp(&b.name));
    for dir in dirs {
        let file = dir.path.join("settings.json");
        let Some(doc) = manifest::read_doc(&file) else {
            continue;
        };
        let Some(events) = events_of(&doc, false) else {
            continue;
        };
        for raw in raw_entries(events) {
            let text = command_text(&handler_text(&raw.handler), ctx.home);
            let seen = known
                .iter()
                .chain(out.iter())
                .any(|e| e.event == raw.event && e.matcher == raw.matcher && e.command == text);
            if seen {
                continue;
            }
            out.push(ctx.entry(
                &raw,
                &file,
                ("toolport", format!("launch-profile:{}", dir.name)),
                (
                    "toolport-command",
                    "runs only when Claude Code starts through this launch profile".to_string(),
                ),
                Some("launch-profile".to_string()),
                false,
            ));
        }
    }
    out
}

/// Everything that applies in `cwd` (user and managed settings only without one).
pub fn collect(env: &Env, cwd: Option<&Path>, layers: &Layers) -> Inventory {
    let mut ctx = Ctx {
        home: &env.home,
        warnings: layers.problems(),
    };
    let ledger = Ledger::load(env.data_dir.as_deref());
    let mut entries: Vec<Entry> = Vec::new();
    let order = [Scope::Managed, Scope::User, Scope::Project, Scope::Local];
    for scope in order {
        for file in layers.files().iter().filter(|f| f.scope == scope) {
            entries.extend(settings_entries(&mut ctx, &ledger, file));
        }
    }
    for plugin in installed::file_list(&env.claude_home) {
        if !layers.enabled(&plugin.id).effective {
            continue;
        }
        let Some(install) = plugin.install_path.as_deref().filter(|p| p.is_dir()) else {
            ctx.warnings.push(format!(
                "plugin {} is on but its install folder is missing; its hooks are not listed",
                plugin.id
            ));
            continue;
        };
        let doc = manifest::load(install);
        entries.extend(plugin_entries(&env.home, &plugin.id, install, &doc, true, &mut ctx.warnings));
    }
    let profile_entries = launch_profile_entries(&mut ctx, &env.profiles_root, &entries);
    entries.extend(profile_entries);

    let off = layers.disable_all_hooks();
    if let Some(off) = &off {
        entries.retain(|e| !off.managed_too && e.owner_kind == "managed");
        ctx.warnings.push(format!(
            "disableAllHooks is set in {}: {}",
            off.set_in.display(),
            if off.managed_too {
                "every hook is off"
            } else {
                "every hook is off except the managed ones"
            }
        ));
    }
    Inventory {
        cwd: cwd.map(Path::to_path_buf),
        disabled_all: off.is_some(),
        entries,
        warnings: ctx.warnings,
    }
}

#[derive(Clone, Debug, Default)]
pub struct Filter {
    pub tool: Option<String>,
    pub event: Option<String>,
    pub owner: Option<String>,
}

impl Filter {
    pub fn validate(&self) -> Result<(), String> {
        if let Some(tool) = &self.tool {
            if !TOOLS.iter().any(|t| t.eq_ignore_ascii_case(tool)) {
                return Err(format!("tool must be one of {}", TOOLS.join(", ")));
            }
        }
        if let Some(owner) = &self.owner {
            if !OWNER_KINDS.contains(&owner.as_str()) {
                return Err(format!("owner must be one of {}", OWNER_KINDS.join(", ")));
            }
        }
        Ok(())
    }

    fn tool_name(&self) -> Option<&'static str> {
        let tool = self.tool.as_deref()?;
        TOOLS.iter().copied().find(|t| t.eq_ignore_ascii_case(tool))
    }

    fn keeps(&self, e: &Entry) -> bool {
        self.tool_name().is_none_or(|t| e.tools.contains(&t))
            && self
                .event
                .as_deref()
                .is_none_or(|ev| e.event.eq_ignore_ascii_case(ev))
            && self.owner.as_deref().is_none_or(|o| e.owner_kind == o)
    }
}

fn counts(entries: &[&Entry]) -> Value {
    let mut by_owner: BTreeMap<String, u64> = BTreeMap::new();
    let mut per_tool: BTreeMap<&str, (u64, u64)> = TOOLS.iter().map(|t| (*t, (0, 0))).collect();
    let mut other: BTreeMap<&str, u64> = BTreeMap::new();
    let mut other_by_owner: BTreeMap<String, BTreeMap<&str, u64>> = BTreeMap::new();
    for e in entries {
        *by_owner.entry(e.owner()).or_default() += 1;
        if e.is_tool_event() {
            for tool in &e.tools {
                let slot = per_tool.entry(tool).or_default();
                if e.event == "PreToolUse" {
                    slot.0 += 1;
                } else {
                    slot.1 += 1;
                }
            }
        } else {
            *other.entry(&e.event).or_default() += 1;
            *other_by_owner
                .entry(e.owner())
                .or_default()
                .entry(&e.event)
                .or_default() += 1;
        }
    }
    let per_tool: BTreeMap<&str, Value> = per_tool
        .into_iter()
        .map(|(t, (pre, post))| (t, json!({"pre": pre, "post": post, "total": pre + post})))
        .collect();
    json!({
        "byOwner": by_owner,
        "perTool": per_tool,
        "otherEvents": other,
        "otherEventsByOwner": other_by_owner,
    })
}

fn conflicts(entries: &[&Entry]) -> Vec<(String, Vec<&'static str>, Vec<String>)> {
    let mut found: Vec<(Vec<String>, Vec<&'static str>)> = Vec::new();
    for tool in TOOLS {
        let firing: Vec<&&Entry> = entries
            .iter()
            .filter(|e| e.event == "PreToolUse" && !e.is_async && e.tools.contains(&tool))
            .collect();
        let owners: std::collections::BTreeSet<String> = firing.iter().map(|e| e.owner()).collect();
        if owners.len() < 2 {
            continue;
        }
        let labels: Vec<String> = firing.iter().map(|e| e.label()).collect();
        match found.iter_mut().find(|(l, _)| *l == labels) {
            Some((_, tools)) => tools.push(tool),
            None => found.push((labels, vec![tool])),
        }
    }
    found
        .into_iter()
        .map(|(labels, tools)| ("PreToolUse".to_string(), tools, labels))
        .collect()
}

pub fn to_value(inv: &Inventory, filter: &Filter) -> Value {
    let effective: Vec<&Entry> = inv.entries.iter().filter(|e| e.active).collect();
    let listed: Vec<Value> = inv
        .entries
        .iter()
        .filter(|e| filter.keeps(e))
        .map(Entry::to_json)
        .collect();
    let conflicts: Vec<Value> = conflicts(&effective)
        .into_iter()
        .filter(|(event, tools, _)| {
            filter.event.as_deref().is_none_or(|ev| event.eq_ignore_ascii_case(ev))
                && filter.tool_name().is_none_or(|t| tools.contains(&t))
        })
        .map(|(event, tools, hooks)| {
            json!({"event": event, "tools": tools, "hooks": hooks, "note": CONFLICT_NOTE})
        })
        .collect();
    json!({
        "cwd": inv.cwd.as_ref().map(|c| c.display().to_string()),
        "disabledAll": inv.disabled_all,
        "hooks": listed,
        "counts": counts(&effective),
        "conflicts": conflicts,
        "warnings": inv.warnings,
    })
}

pub fn to_text(data: &Value) -> String {
    let mut out = String::new();
    let hooks = data["hooks"].as_array().cloned().unwrap_or_default();
    if data["disabledAll"] == Value::Bool(true) {
        out.push_str("all hooks are switched off (disableAllHooks)\n");
    }
    if hooks.is_empty() {
        out.push_str("no hooks\n");
    }
    for h in &hooks {
        let tools = h["tools"]
            .as_array()
            .map(|t| t.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(","))
            .unwrap_or_default();
        out.push_str(&format!(
            "{:<12} {:<26} {:<18} {}{}\n",
            h["event"].as_str().unwrap_or(""),
            format!(
                "{}:{}",
                h["owner"]["kind"].as_str().unwrap_or(""),
                h["owner"]["name"].as_str().unwrap_or("")
            ),
            if tools.is_empty() { h["matcher"].as_str().unwrap_or("") } else { &tools },
            h["command"].as_str().unwrap_or(""),
            if h["active"] == Value::Bool(false) { "  (inactive)" } else { "" }
        ));
    }
    let per_tool = &data["counts"]["perTool"];
    let line: Vec<String> = TOOLS
        .iter()
        .map(|t| format!("{t} {}", per_tool[t]["total"].as_u64().unwrap_or(0)))
        .collect();
    out.push_str(&format!("processes per tool call: {}\n", line.join(", ")));
    for c in data["conflicts"].as_array().into_iter().flatten() {
        let tools: Vec<&str> = c["tools"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        out.push_str(&format!(
            "conflict on {} {}: {} ({})\n",
            c["event"].as_str().unwrap_or(""),
            tools.join(","),
            c["hooks"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" | "),
            c["note"].as_str().unwrap_or("")
        ));
    }
    for w in data["warnings"].as_array().into_iter().flatten().filter_map(Value::as_str) {
        out.push_str(&format!("warning: {w}\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matcher_semantics_are_exact_list_or_unanchored_regex() {
        let hit = |m: &str, t: &str| matches_tool(m, t).unwrap();
        assert!(hit("*", "Read") && hit("", "Bash"));
        assert!(hit("Bash", "Bash") && !hit("Bash", "Edit"));
        assert!(hit("Edit|Write", "Write") && !hit("Edit|Write", "Read"));
        assert!(hit("Edit, Write", "Write"));
        assert!(!hit("Edit|MultiEdit", "Multi"));
        assert!(hit(".*", "Read"));
        assert!(hit("mcp__.*", "mcp__x__y") && !hit("mcp__.*", "Bash"));
        assert!(!hit("Bas", "Bash"), "an exact list never matches a prefix");
        assert!(hit("^(Bash|Edit)$", "Edit") && !hit("^(Bash|Edit)$", "Write"));
        assert!(hit("Writ.", "Write"), "a regex is unanchored");
        assert!(matches_tool("(", "Bash").is_err());
    }

    #[test]
    fn command_text_shortens_home_and_drops_env_values() {
        let home = Path::new("/home/u");
        assert_eq!(
            command_text("FOO=bar BAZ=\"a b\" node /home/u/.claude/hooks/x.js --flag", home),
            "FOO=… BAZ=… node ~/.claude/hooks/x.js --flag"
        );
        assert_eq!(command_text("a=b", home), "a=…");
        assert_eq!(command_text("echo a=b", home), "echo a=b");
        assert_eq!(command_text("/usr/bin/true", Path::new("/")), "/usr/bin/true");
    }

    #[test]
    fn command_text_masks_credentials_in_flags_headers_and_urls() {
        let home = Path::new("/home/u");
        let text = |c: &str| command_text(c, home);
        assert_eq!(
            text("./check.sh --api-key=abc123 --verbose"),
            "./check.sh --api-key=[redacted] --verbose"
        );
        assert_eq!(text("run --token abc123 --x"), "run --token [redacted] --x");
        assert_eq!(text("run --token=\"a b\" --x"), "run --token=[redacted] --x");
        assert_eq!(
            text("curl -H \"Authorization: Bearer abc123\" https://svc.example.invalid/x"),
            "curl -H \"Authorization: Bearer [redacted]\" https://svc.example.invalid/x"
        );
        assert_eq!(
            text("sh -c \"notify --password=hunter2 && echo ok\""),
            "sh -c \"notify --password=[redacted] && echo ok\""
        );
        assert_eq!(
            text("curl https://user:pw@svc.example.invalid/hook?key=abc123"),
            "curl https://svc.example.invalid/hook?…"
        );
        assert_eq!(
            text("export SERVICE_KEY=abc123 && go"),
            "export SERVICE_KEY=[redacted] && go"
        );
        assert_eq!(text("node x.js --keep=1 --port 80"), "node x.js --keep=1 --port 80");
    }

    #[test]
    fn labels_name_the_script_and_its_first_plain_argument() {
        assert_eq!(short_command("~/.claude/hooks/gh-write-gate.sh"), "gh-write-gate.sh");
        assert_eq!(
            short_command(
                "node -e \"const p=require('path');require(p.join(r,'x'))\" \"${CLAUDE_PLUGIN_ROOT}/scripts/hooks/run-with-flags.js\" pre:bash:dispatcher scripts/hooks/pre-bash-dispatcher.js standard"
            ),
            "run-with-flags.js pre:bash:dispatcher"
        );
        assert_eq!(short_command("FOO=1 rtk hook claude"), "rtk");
        assert_eq!(short_command(""), "");
    }

    #[test]
    fn flat_and_nested_hook_shapes_are_read() {
        let doc = json!({"hooks": {"Stop": [{"hooks": [{"type": "command", "command": "a"}]}],
                                   "PreToolUse": [{"matcher": "Bash", "command": "b"}]}});
        let raws = raw_entries(events_of(&doc, false).unwrap());
        assert_eq!(raws.len(), 2);
        assert_eq!(raws[0].key, "hooks.PreToolUse[0]");
        assert_eq!(raws[1].key, "hooks.Stop[0].hooks[0]");
        assert!(events_of(&json!({"PreToolUse": [{"hooks": []}]}), true).is_some());
        assert!(events_of(&json!({"PreToolUse": [{"hooks": []}]}), false).is_none());
    }
}
