//! Context bundles (D-064, D-066): `profiles/<name>.yaml` in the skills repository. A bundle says
//! what to hide and add for a folder; `bundle_apply` writes it into git-ignored local files. This
//! file is the format: parse, lint, edit and render. Unknown keys stay in the document and are
//! reported by lint; `plugins.config` (adapter knobs) and `mcp.deny` ride the same ledger.

use crate::plus::plugins::adapters::Registry;
use serde::Serialize;
use serde_yaml::{Mapping, Value as Yaml};

pub const FORMAT: u64 = 1;

const TOP_KEYS: [&str; 10] = [
    "format",
    "name",
    "description",
    "servers",
    "skills",
    "plugins",
    "mcp",
    "layers",
    "agents",
    "bind",
];
const SKILLS_KEYS: [&str; 3] = ["off", "name_only", "allow"];
const PLUGINS_KEYS: [&str; 2] = ["off", "config"];
const MCP_KEYS: [&str; 1] = ["deny"];
const LAYERS_KEYS: [&str; 2] = ["add", "exclude"];
const AGENTS_KEYS: [&str; 1] = ["off"];

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Bundle {
    pub name: String,
    pub description: String,
    pub servers: Option<String>,
    pub skills_off: Vec<String>,
    pub skills_name_only: Vec<String>,
    pub skills_allow: Vec<String>,
    pub plugins_off: Vec<String>,
    /// `plugins.config`: per plugin id, the knob and its value as text (a list is comma-joined).
    pub plugins_config: Vec<(String, Vec<(String, String)>)>,
    pub mcp_deny: Vec<String>,
    pub layers_add: Vec<String>,
    pub layers_exclude: Vec<String>,
    pub agents_off: Vec<String>,
    pub bind: Vec<String>,
    /// `skills:` was the plain list of the old `default.yaml`, read as `skills.allow`.
    pub legacy_list: bool,
    pub doc: Mapping,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Issue {
    pub level: &'static str,
    pub key: String,
    pub message: String,
}

fn key(name: &str) -> Yaml {
    Yaml::String(name.to_string())
}

fn at<'a>(doc: &'a Mapping, path: &[&str]) -> Option<&'a Yaml> {
    let mut node = doc.get(key(path.first()?))?;
    for part in &path[1..] {
        node = node.as_mapping()?.get(key(part))?;
    }
    Some(node)
}

fn strings(doc: &Mapping, path: &[&str]) -> Result<Vec<String>, String> {
    let label = path.join(".");
    match at(doc, path) {
        None | Some(Yaml::Null) => Ok(Vec::new()),
        Some(Yaml::Sequence(items)) => items
            .iter()
            .map(|item| match item {
                Yaml::String(s) => Ok(s.clone()),
                _ => Err(format!("{label} must be a list of strings")),
            })
            .collect(),
        Some(_) => Err(format!("{label} must be a list of strings")),
    }
}

fn knob_text(plugin: &str, knob: &str, value: &Yaml) -> Result<String, String> {
    let one = |v: &Yaml| match v {
        Yaml::String(s) => Some(s.clone()),
        Yaml::Bool(b) => Some(b.to_string()),
        Yaml::Number(n) => Some(n.to_string()),
        _ => None,
    };
    match value {
        Yaml::Sequence(items) => items
            .iter()
            .map(one)
            .collect::<Option<Vec<_>>>()
            .map(|l| l.join(","))
            .ok_or_else(|| format!("plugins.config.{plugin}.{knob} must be a value or a list of values")),
        v => one(v).ok_or_else(|| format!("plugins.config.{plugin}.{knob} must be a value or a list of values")),
    }
}

/// `plugins.config`: `{<plugin id>: {<knob>: value | [values]}}` as ordered text pairs.
pub fn plugins_config(node: Option<&Yaml>) -> Result<Vec<(String, Vec<(String, String)>)>, String> {
    let map = match node {
        None | Some(Yaml::Null) => return Ok(Vec::new()),
        Some(Yaml::Mapping(m)) => m,
        Some(_) => return Err("plugins.config must be a mapping of plugin id to knobs".into()),
    };
    let mut out = Vec::new();
    for (plugin, knobs) in map {
        let plugin = plugin.as_str().ok_or("plugins.config keys must be plugin ids")?;
        let knobs = match knobs {
            Yaml::Null => continue,
            Yaml::Mapping(m) => m,
            _ => return Err(format!("plugins.config.{plugin} must be a mapping of knob to value")),
        };
        let mut pairs = Vec::new();
        for (knob, value) in knobs {
            let knob = knob.as_str().ok_or_else(|| format!("plugins.config.{plugin} keys must be knob names"))?;
            pairs.push((knob.to_string(), knob_text(plugin, knob, value)?));
        }
        out.push((plugin.to_string(), pairs));
    }
    Ok(out)
}

pub fn parse(name: &str, text: &str) -> Result<Bundle, String> {
    let value: Yaml = serde_yaml::from_str(text).map_err(|e| format!("not valid YAML: {e}"))?;
    let doc = match value {
        Yaml::Null => Mapping::new(),
        Yaml::Mapping(m) => m,
        _ => return Err("a bundle is a YAML mapping".into()),
    };
    if let Some(format) = doc.get(key("format")) {
        if format.as_u64() != Some(FORMAT) {
            return Err(format!("format must be {FORMAT}"));
        }
    }
    let text_of = |k: &str| match doc.get(key(k)) {
        None | Some(Yaml::Null) => Ok(None),
        Some(Yaml::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(format!("{k} must be a string")),
    };
    let legacy_list = matches!(doc.get(key("skills")), Some(Yaml::Sequence(_)));
    let skills_allow = if legacy_list {
        strings(&doc, &["skills"])?
    } else {
        strings(&doc, &["skills", "allow"])?
    };
    for k in ["skills", "plugins", "mcp", "layers", "agents"] {
        match doc.get(key(k)) {
            None | Some(Yaml::Null) | Some(Yaml::Mapping(_)) => {}
            Some(Yaml::Sequence(_)) if k == "skills" => {}
            Some(_) => return Err(format!("{k} must be a mapping")),
        }
    }
    Ok(Bundle {
        name: name.to_string(),
        description: text_of("description")?.unwrap_or_default(),
        servers: text_of("servers")?.filter(|s| !s.is_empty()),
        skills_off: if legacy_list { Vec::new() } else { strings(&doc, &["skills", "off"])? },
        skills_name_only: if legacy_list {
            Vec::new()
        } else {
            strings(&doc, &["skills", "name_only"])?
        },
        skills_allow,
        plugins_off: strings(&doc, &["plugins", "off"])?,
        plugins_config: plugins_config(at(&doc, &["plugins", "config"]))?,
        mcp_deny: strings(&doc, &["mcp", "deny"])?,
        layers_add: strings(&doc, &["layers", "add"])?,
        layers_exclude: strings(&doc, &["layers", "exclude"])?,
        agents_off: strings(&doc, &["agents", "off"])?,
        bind: strings(&doc, &["bind"])?,
        legacy_list,
        doc,
    })
}

fn issue(level: &'static str, key: &str, message: impl Into<String>) -> Issue {
    Issue {
        level,
        key: key.to_string(),
        message: message.into(),
    }
}

fn check_keys(map: &Mapping, prefix: &str, known: &[&str], reserved: &[&str], out: &mut Vec<Issue>) {
    for (k, _) in map {
        let Some(k) = k.as_str() else {
            out.push(issue("error", prefix, "keys must be strings"));
            continue;
        };
        let full = if prefix.is_empty() {
            k.to_string()
        } else {
            format!("{prefix}.{k}")
        };
        if reserved.contains(&k) {
            out.push(issue("warning", &full, "reserved: kept in the file, not applied"));
        } else if !known.contains(&k) {
            out.push(issue("warning", &full, "unknown key (kept when the file is written)"));
        }
    }
}

fn repeated(list: &[String], key: &str, out: &mut Vec<Issue>) {
    for (i, item) in list.iter().enumerate() {
        if item.trim().is_empty() {
            out.push(issue("error", key, "entries must not be empty"));
        } else if list[..i].contains(item) {
            out.push(issue("warning", key, format!("{item} is listed twice")));
        }
    }
}

/// Everything a reader of the file should know, errors first. An unreadable file is one error.
pub fn lint(name: &str, text: &str) -> Vec<Issue> {
    lint_with(name, text, &Registry::load(crate::registry::conduit_dir().as_deref()))
}

pub fn lint_with(name: &str, text: &str, registry: &Registry) -> Vec<Issue> {
    let bundle = match parse(name, text) {
        Ok(b) => b,
        Err(message) => return vec![issue("error", "", message)],
    };
    let mut out = Vec::new();
    check_keys(&bundle.doc, "", &TOP_KEYS, &[], &mut out);
    for (section, known, reserved) in [
        ("plugins", &PLUGINS_KEYS[..], &[][..]),
        ("mcp", &MCP_KEYS[..], &[][..]),
        ("layers", &LAYERS_KEYS[..], &[][..]),
        ("agents", &AGENTS_KEYS[..], &[][..]),
    ] {
        if let Some(Yaml::Mapping(map)) = bundle.doc.get(key(section)) {
            check_keys(map, section, known, reserved, &mut out);
        }
    }
    if let Some(Yaml::Mapping(map)) = bundle.doc.get(key("skills")) {
        check_keys(map, "skills", &SKILLS_KEYS, &[], &mut out);
    }
    if let Some(Yaml::String(declared)) = bundle.doc.get(key("name")) {
        if declared != name {
            out.push(issue(
                "warning",
                "name",
                format!("the file is called {name}, the name key says {declared}; the file name wins"),
            ));
        }
    }
    for (label, list) in [
        ("skills.off", &bundle.skills_off),
        ("skills.name_only", &bundle.skills_name_only),
        ("skills.allow", &bundle.skills_allow),
        ("plugins.off", &bundle.plugins_off),
        ("layers.add", &bundle.layers_add),
        ("layers.exclude", &bundle.layers_exclude),
        ("agents.off", &bundle.agents_off),
        ("bind", &bundle.bind),
    ] {
        repeated(list, label, &mut out);
    }
    for skill in &bundle.skills_name_only {
        if bundle.skills_off.contains(skill) {
            out.push(issue("warning", "skills.name_only", format!("{skill} is also in skills.off, which wins")));
        }
    }
    for id in &bundle.plugins_off {
        if !id.contains('@') {
            out.push(issue("warning", "plugins.off", format!("{id} is not a plugin id such as name@marketplace")));
        }
    }
    if bundle.legacy_list {
        out.push(issue("warning", "skills", "a plain skills list is read as skills.allow"));
    }
    for (plugin, knobs) in &bundle.plugins_config {
        let at = format!("plugins.config.{plugin}");
        let Some(adapter) = registry.for_plugin(plugin) else {
            out.push(issue("error", &at, format!("no adapter for {plugin}: its knobs cannot be written")));
            continue;
        };
        for (knob, value) in knobs {
            let key = format!("{at}.{knob}");
            match adapter.knob(knob) {
                None => out.push(issue("error", &key, format!("unknown knob; {plugin} has {}", adapter.knob_names()))),
                Some(k) => {
                    if let Err(e) = crate::plus::plugins::config::check_value(k, value) {
                        out.push(issue("error", &key, e));
                    }
                }
            }
        }
    }
    repeated(&bundle.mcp_deny, "mcp.deny", &mut out);
    for entry in &bundle.mcp_deny {
        let parts: Vec<&str> = entry.split(':').collect();
        if parts.len() < 3 || parts[0] != "plugin" || parts[1..].iter().any(|p| p.is_empty()) {
            out.push(issue("error", "mcp.deny", format!("{entry} must look like plugin:<plugin>:<server>; the bare name does not block anything")));
        }
    }
    out.sort_by_key(|i| i.level != "error");
    out
}

/// What `bundle add` and `bundle edit` change; a given list replaces that list.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Edit {
    pub description: Option<String>,
    pub servers: Option<String>,
    pub skills_off: Option<Vec<String>>,
    pub skills_name_only: Option<Vec<String>>,
    pub skills_allow: Option<Vec<String>>,
    pub plugins_off: Option<Vec<String>>,
    pub layers_add: Option<Vec<String>>,
    pub layers_exclude: Option<Vec<String>>,
    pub agents_off: Option<Vec<String>>,
    pub bind: Option<Vec<String>>,
}

fn set_at(doc: &mut Mapping, path: &[&str], value: Option<Yaml>) {
    let (last, parents) = path.split_last().expect("a path");
    if parents.is_empty() {
        match value {
            Some(v) => {
                doc.insert(key(last), v);
            }
            None => {
                doc.remove(key(last));
            }
        }
        return;
    }
    let head = key(parents[0]);
    if value.is_none() && !matches!(doc.get(&head), Some(Yaml::Mapping(_))) {
        return;
    }
    if !matches!(doc.get(&head), Some(Yaml::Mapping(_))) {
        doc.insert(head.clone(), Yaml::Mapping(Mapping::new()));
    }
    let Some(Yaml::Mapping(child)) = doc.get_mut(&head) else {
        return;
    };
    set_at(child, &path[1..], value);
    if child.is_empty() {
        doc.remove(&head);
    }
}

fn list_value(list: &[String]) -> Option<Yaml> {
    (!list.is_empty()).then(|| Yaml::Sequence(list.iter().map(|s| Yaml::String(s.clone())).collect()))
}

impl Edit {
    pub fn is_empty(&self) -> bool {
        *self == Edit::default()
    }

    fn touches_skills(&self) -> bool {
        self.skills_off.is_some() || self.skills_name_only.is_some() || self.skills_allow.is_some()
    }

    pub fn apply(&self, name: &str, doc: &mut Mapping) {
        if !doc.contains_key(key("format")) {
            let mut fresh = Mapping::new();
            fresh.insert(key("format"), Yaml::Number(FORMAT.into()));
            for (k, v) in std::mem::take(doc) {
                fresh.insert(k, v);
            }
            *doc = fresh;
        }
        if !doc.contains_key(key("name")) {
            let mut fresh = Mapping::new();
            let mut taken = std::mem::take(doc).into_iter();
            if let Some((k, v)) = taken.next() {
                fresh.insert(k, v);
            }
            fresh.insert(key("name"), key(name));
            fresh.extend(taken);
            *doc = fresh;
        }
        if let Some(text) = &self.description {
            set_at(doc, &["description"], (!text.is_empty()).then(|| key(text)));
        }
        if let Some(text) = &self.servers {
            set_at(doc, &["servers"], (!text.is_empty()).then(|| key(text)));
        }
        if self.touches_skills() {
            if let Some(Yaml::Sequence(list)) = doc.get(key("skills")).cloned() {
                doc.remove(key("skills"));
                if self.skills_allow.is_none() {
                    set_at(doc, &["skills", "allow"], Some(Yaml::Sequence(list)));
                }
            }
        }
        for (path, list) in [
            (&["skills", "off"][..], &self.skills_off),
            (&["skills", "name_only"][..], &self.skills_name_only),
            (&["skills", "allow"][..], &self.skills_allow),
            (&["plugins", "off"][..], &self.plugins_off),
            (&["layers", "add"][..], &self.layers_add),
            (&["layers", "exclude"][..], &self.layers_exclude),
            (&["agents", "off"][..], &self.agents_off),
            (&["bind"][..], &self.bind),
        ] {
            if let Some(list) = list {
                set_at(doc, path, list_value(list));
            }
        }
    }
}

pub fn render(doc: &Mapping) -> String {
    serde_yaml::to_string(doc).unwrap_or_default()
}

/// A new definition in the canonical key order.
pub fn create(name: &str, edit: &Edit) -> String {
    let mut doc = Mapping::new();
    edit.apply(name, &mut doc);
    render(&doc)
}

/// The text after an edit; keys the format does not know stay where they were.
pub fn edited(name: &str, text: &str, edit: &Edit) -> Result<String, String> {
    let mut bundle = parse(name, text)?;
    edit.apply(name, &mut bundle.doc);
    Ok(render(&bundle.doc))
}

/// Whether `pattern` (a path with `*`) offers the bundle for `folder`; `~` is the home directory.
pub fn bind_matches(pattern: &str, folder: &str, home: &str) -> bool {
    let expanded = match pattern.strip_prefix("~/") {
        Some(rest) => format!("{}/{rest}", home.trim_end_matches('/')),
        None => pattern.to_string(),
    };
    super::globs::glob_match(expanded.trim_end_matches('/'), folder.trim_end_matches('/'))
}
