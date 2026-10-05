//! Plugin adapters (D-075): a small yaml file per plugin that names the switches the plugin really
//! has. `ecc` is built in; more load from `<data dir>/plus/adapters/<plugin>.yaml`. A malformed
//! adapter is reported and ignored, never half used.

use regex::Regex;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const FORMAT: u64 = 1;
const ECC: &str = include_str!("ecc.yaml");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Enum,
    Bool,
    BoolOff,
    Csv,
    Globs,
    HookIds,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Enum => "enum",
            Kind::Bool => "bool",
            Kind::BoolOff => "bool-off",
            Kind::Csv => "csv",
            Kind::Globs => "globs",
            Kind::HookIds => "hook-ids",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "enum" => Kind::Enum,
            "bool" => Kind::Bool,
            "bool-off" => Kind::BoolOff,
            "csv" => Kind::Csv,
            "globs" => Kind::Globs,
            "hook-ids" => Kind::HookIds,
            _ => return None,
        })
    }

    /// A list kind: `--set` takes a comma list and the value replaces any user-level one.
    pub fn is_list(self) -> bool {
        matches!(self, Kind::Csv | Kind::Globs | Kind::HookIds)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Knob {
    pub key: String,
    pub label: String,
    pub env: String,
    pub option: Option<String>,
    pub kind: Kind,
    pub choices: Option<Vec<String>>,
    pub default: Option<String>,
}

#[derive(Clone, Debug)]
pub struct HookIds {
    pub file: String,
    pub regex: Regex,
}

#[derive(Clone, Debug)]
pub struct Adapter {
    pub plugin: String,
    pub knobs: Vec<Knob>,
    pub hook_ids: Option<HookIds>,
    pub origin: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub file: String,
    pub message: String,
}

#[derive(Default)]
pub struct Registry {
    pub adapters: Vec<Adapter>,
    pub problems: Vec<Problem>,
}

#[derive(Deserialize)]
struct RawKnob {
    key: String,
    #[serde(default)]
    label: String,
    env: String,
    #[serde(default)]
    option: Option<String>,
    kind: String,
    #[serde(default)]
    choices: Option<Vec<serde_yaml::Value>>,
    #[serde(default)]
    default: Option<serde_yaml::Value>,
}

#[derive(Deserialize)]
struct RawHookIds {
    file: String,
    regex: String,
}

#[derive(Deserialize)]
struct RawAdapter {
    format: u64,
    plugin: String,
    knobs: Vec<RawKnob>,
    #[serde(default)]
    hook_ids: Option<RawHookIds>,
}

fn scalar(v: &serde_yaml::Value) -> Result<String, String> {
    match v {
        serde_yaml::Value::String(s) => Ok(s.clone()),
        serde_yaml::Value::Bool(b) => Ok(b.to_string()),
        serde_yaml::Value::Number(n) => Ok(n.to_string()),
        _ => Err("expected a plain value".to_string()),
    }
}

fn ident(text: &str, extra: &[char]) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || extra.contains(&c))
}

pub fn parse(text: &str) -> Result<Adapter, String> {
    let raw: RawAdapter = serde_yaml::from_str(text).map_err(|e| format!("not a valid adapter: {e}"))?;
    if raw.format != FORMAT {
        return Err(format!("format must be {FORMAT}"));
    }
    if !super::installed::valid_ident(&raw.plugin) || !raw.plugin.contains('@') {
        return Err(format!("plugin must be <name>@<marketplace>, got {:?}", raw.plugin));
    }
    let mut knobs: Vec<Knob> = Vec::new();
    for k in raw.knobs {
        let kind = Kind::parse(&k.kind).ok_or_else(|| format!("knob {}: unknown kind {:?}", k.key, k.kind))?;
        if !ident(&k.key, &['-']) {
            return Err(format!("knob key {:?} may hold letters, digits, _ and - only", k.key));
        }
        if !ident(&k.env, &[]) || k.env.starts_with(|c: char| c.is_ascii_digit()) {
            return Err(format!("knob {}: env {:?} is not an environment variable name", k.key, k.env));
        }
        if knobs.iter().any(|o| o.key == k.key) {
            return Err(format!("knob {} is listed twice", k.key));
        }
        if let Some(opt) = &k.option {
            if !ident(opt, &['-']) {
                return Err(format!("knob {}: option {:?} is not a plugin option name", k.key, opt));
            }
        }
        let choices = k
            .choices
            .map(|list| list.iter().map(scalar).collect::<Result<Vec<_>, _>>())
            .transpose()
            .map_err(|e| format!("knob {}: choices: {e}", k.key))?;
        if kind == Kind::Enum && choices.as_ref().is_none_or(Vec::is_empty) {
            return Err(format!("knob {}: an enum needs choices", k.key));
        }
        let default = k
            .default
            .as_ref()
            .map(scalar)
            .transpose()
            .map_err(|e| format!("knob {}: default: {e}", k.key))?;
        knobs.push(Knob {
            label: if k.label.is_empty() { k.key.clone() } else { k.label },
            key: k.key,
            env: k.env,
            option: k.option,
            kind,
            choices,
            default,
        });
    }
    let hook_ids = raw
        .hook_ids
        .map(|h| {
            Regex::new(&h.regex)
                .map(|regex| HookIds { file: h.file, regex })
                .map_err(|e| format!("hook_ids.regex: {e}"))
        })
        .transpose()?;
    if let Some(h) = &hook_ids {
        if h.regex.captures_len() < 2 || h.file.starts_with('/') || h.file.split('/').any(|p| p == "..") {
            return Err("hook_ids needs a regex with one capture group and a file inside the plugin".into());
        }
    }
    Ok(Adapter { plugin: raw.plugin, knobs, hook_ids, origin: None })
}

pub fn user_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("plus").join("adapters")
}

impl Registry {
    pub fn load(data_dir: Option<&Path>) -> Self {
        let mut reg = Registry::default();
        match parse(ECC) {
            Ok(a) => reg.adapters.push(a),
            Err(e) => reg.problems.push(Problem { file: "built-in ecc".into(), message: e }),
        }
        let Some(dir) = data_dir.map(user_dir) else {
            return reg;
        };
        let mut entries = crate::plus::sources::fsx::list_dir(&dir);
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        for entry in entries.into_iter().filter(|e| e.name.ends_with(".yaml")) {
            let file = crate::plus::sources::fsx::display(&entry.path);
            let Some(text) = crate::plus::sources::fsx::read_text(&entry.path, 256 * 1024) else {
                reg.problems.push(Problem { file, message: "cannot be read".into() });
                continue;
            };
            match parse(&text) {
                Ok(mut a) if entry.name.trim_end_matches(".yaml") == a.plugin => {
                    a.origin = Some(entry.path.clone());
                    reg.adapters.retain(|o| o.plugin != a.plugin);
                    reg.adapters.push(a);
                }
                Ok(a) => reg.problems.push(Problem {
                    file,
                    message: format!("the file is named {} but the adapter is for {}", entry.name, a.plugin),
                }),
                Err(message) => reg.problems.push(Problem { file, message }),
            }
        }
        reg
    }

    pub fn for_plugin(&self, id: &str) -> Option<&Adapter> {
        self.adapters.iter().find(|a| a.plugin == id)
    }

    pub fn problems_json(&self) -> Value {
        Value::Array(
            self.problems
                .iter()
                .map(|p| json!({"file": p.file, "message": p.message}))
                .collect(),
        )
    }
}

impl Adapter {
    pub fn knob(&self, key: &str) -> Option<&Knob> {
        self.knobs.iter().find(|k| k.key == key)
    }

    /// The plugin's own hook ids from its hooks file, in file order, without repeats.
    pub fn hook_ids_of(&self, install: &Path) -> Vec<String> {
        let Some(spec) = &self.hook_ids else {
            return Vec::new();
        };
        let Some(text) = crate::plus::sources::fsx::read_text(&install.join(&spec.file), 4 * 1024 * 1024) else {
            return Vec::new();
        };
        let Ok(doc) = serde_json::from_str::<Value>(&text) else {
            return Vec::new();
        };
        let mut out: Vec<String> = Vec::new();
        collect_commands(&doc, &mut |command| {
            for caps in spec.regex.captures_iter(command) {
                let id = caps[1].trim_matches(['"', '\'', '\\']).to_string();
                if !id.is_empty() && !out.contains(&id) {
                    out.push(id);
                }
            }
        });
        out
    }
}

fn collect_commands(v: &Value, each: &mut impl FnMut(&str)) {
    match v {
        Value::Object(map) => {
            for (k, v) in map {
                match (k.as_str(), v) {
                    ("command", Value::String(s)) => each(s),
                    _ => collect_commands(v, each),
                }
            }
        }
        Value::Array(items) => items.iter().for_each(|i| collect_commands(i, each)),
        _ => {}
    }
}

/// The knobs of an adapter as `plugins show` lists them: each with the value Claude Code will
/// see and where it comes from (a folder's env, the user's env, the plugin option, the default).
pub fn knobs_json(
    adapter: &Adapter,
    layers: &super::settings::Layers,
    options: &[super::manifest::OptionRow],
    install: Option<&Path>,
) -> Value {
    let hook_ids = install.map(|d| adapter.hook_ids_of(d)).unwrap_or_default();
    Value::Array(
        adapter
            .knobs
            .iter()
            .map(|k| {
                let (value, from) = match layers.env_value(&k.env) {
                    Some((v, super::settings::Scope::User)) => (Some(v), "user-env"),
                    Some((v, _)) => (Some(v), "folder-env"),
                    None => match k
                        .option
                        .as_ref()
                        .and_then(|o| options.iter().find(|r| &r.key == o))
                        .and_then(|r| r.current.clone())
                    {
                        Some(v) => (Some(v), "option"),
                        None => (k.default.clone(), "default"),
                    },
                };
                let choices = match k.kind {
                    Kind::HookIds if !hook_ids.is_empty() => Some(hook_ids.clone()),
                    _ => k.choices.clone(),
                };
                json!({
                    "key": k.key,
                    "label": k.label,
                    "kind": k.kind.as_str(),
                    "env": k.env,
                    "option": k.option,
                    "choices": choices,
                    "default": k.default,
                    "current": {"value": value, "from": from},
                })
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests;
