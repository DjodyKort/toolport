//! Plugin adapters (D-075): a small yaml file per plugin that names the switches the plugin really
//! has, and the bundle `plugins.config` values written as `env` keys. `ecc` is built in; more load from `<data dir>/plus/adapters/<plugin>.yaml`. A malformed
//! adapter is reported and ignored, never half used.

use regex::Regex;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const FORMAT: u64 = 1;
const ECC: &str = include_str!("adapters/ecc.yaml");

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
    let raw: RawAdapter =
        serde_yaml::from_str(text).map_err(|e| format!("not a valid adapter: {e}"))?;
    if raw.format != FORMAT {
        return Err(format!("format must be {FORMAT}"));
    }
    if !valid_ident(&raw.plugin) || !raw.plugin.contains('@') {
        return Err(format!(
            "plugin must be <name>@<marketplace>, got {:?}",
            raw.plugin
        ));
    }
    let mut knobs: Vec<Knob> = Vec::new();
    for k in raw.knobs {
        let kind = Kind::parse(&k.kind)
            .ok_or_else(|| format!("knob {}: unknown kind {:?}", k.key, k.kind))?;
        if !ident(&k.key, &['-']) {
            return Err(format!(
                "knob key {:?} may hold letters, digits, _ and - only",
                k.key
            ));
        }
        if !ident(&k.env, &[]) || k.env.starts_with(|c: char| c.is_ascii_digit()) {
            return Err(format!(
                "knob {}: env {:?} is not an environment variable name",
                k.key, k.env
            ));
        }
        if knobs.iter().any(|o| o.key == k.key) {
            return Err(format!("knob {} is listed twice", k.key));
        }
        if let Some(opt) = &k.option {
            if !ident(opt, &['-']) {
                return Err(format!(
                    "knob {}: option {:?} is not a plugin option name",
                    k.key, opt
                ));
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
            label: if k.label.is_empty() {
                k.key.clone()
            } else {
                k.label
            },
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
                .map(|regex| HookIds {
                    file: h.file,
                    regex,
                })
                .map_err(|e| format!("hook_ids.regex: {e}"))
        })
        .transpose()?;
    if let Some(h) = &hook_ids {
        if h.regex.captures_len() < 2
            || h.file.starts_with('/')
            || h.file.split('/').any(|p| p == "..")
        {
            return Err(
                "hook_ids needs a regex with one capture group and a file inside the plugin".into(),
            );
        }
    }
    Ok(Adapter {
        plugin: raw.plugin,
        knobs,
        hook_ids,
        origin: None,
    })
}

pub fn user_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("plus").join("adapters")
}

impl Registry {
    pub fn load(data_dir: Option<&Path>) -> Self {
        let mut reg = Registry::default();
        match parse(ECC) {
            Ok(a) => reg.adapters.push(a),
            Err(e) => reg.problems.push(Problem {
                file: "built-in ecc".into(),
                message: e,
            }),
        }
        let Some(dir) = data_dir.map(user_dir) else {
            return reg;
        };
        let mut entries = super::fsx::list_dir(&dir);
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        for entry in entries.into_iter().filter(|e| e.name.ends_with(".yaml")) {
            let file = super::fsx::display(&entry.path);
            let Some(text) = super::fsx::read_text(&entry.path, 256 * 1024) else {
                reg.problems.push(Problem {
                    file,
                    message: "cannot be read".into(),
                });
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
                    message: format!(
                        "the file is named {} but the adapter is for {}",
                        entry.name, a.plugin
                    ),
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

    pub fn knob_names(&self) -> String {
        self.knobs
            .iter()
            .map(|k| k.key.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

pub fn valid_ident(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with('-')
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '@'))
}

const MAX_VALUE: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EnvChange {
    Set(String, String),
    Remove(String),
}

fn single_line(value: &str) -> Result<(), String> {
    if value.len() > MAX_VALUE || value.chars().any(|c| c.is_control()) {
        return Err(
            "a value is one line of at most 4096 characters without control characters".into(),
        );
    }
    Ok(())
}

fn list_of(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

fn truth(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "on" | "yes" | "1" => Some(true),
        "false" | "off" | "no" | "0" => Some(false),
        _ => None,
    }
}

/// The environment change a knob value stands for, or why the value is refused.
pub fn change_of(knob: &Knob, value: &str) -> Result<EnvChange, String> {
    single_line(value)?;
    let env = knob.env.clone();
    match knob.kind {
        Kind::Enum => {
            let choices = knob.choices.as_deref().unwrap_or_default();
            if choices.iter().any(|c| c == value) {
                Ok(EnvChange::Set(env, value.to_string()))
            } else {
                Err(format!(
                    "{} must be one of {}",
                    knob.key,
                    choices.join(", ")
                ))
            }
        }
        Kind::Bool => truth(value)
            .map(|on| EnvChange::Set(env, on.to_string()))
            .ok_or_else(|| format!("{} is true or false", knob.key)),
        Kind::BoolOff => match truth(value) {
            Some(true) => Ok(EnvChange::Remove(env)),
            Some(false) => Ok(EnvChange::Set(env, "off".into())),
            None => Err(format!("{} is on or off", knob.key)),
        },
        Kind::Csv | Kind::Globs | Kind::HookIds => {
            let items = list_of(value);
            if items.is_empty() {
                return Err(format!(
                    "{} needs at least one value (use --unset to remove it)",
                    knob.key
                ));
            }
            Ok(EnvChange::Set(env, items.join(",")))
        }
    }
}

pub fn check_value(knob: &Knob, value: &str) -> Result<(), String> {
    change_of(knob, value).map(|_| ())
}

/// The `env` pairs of a bundle's `plugins.config`; what cannot be written becomes a warning.
pub fn bundle_env(
    registry: &Registry,
    config: &[(String, Vec<(String, String)>)],
) -> (Vec<(String, String)>, Vec<String>) {
    let mut env: BTreeMap<String, String> = BTreeMap::new();
    let mut warnings = Vec::new();
    for (plugin, knobs) in config {
        let Some(adapter) = registry.for_plugin(plugin) else {
            warnings.push(format!(
                "plugins.config: no adapter for {plugin}; not applied"
            ));
            continue;
        };
        for (key, value) in knobs {
            match adapter.knob(key).map(|k| change_of(k, value)) {
                None => warnings.push(format!(
                    "plugins.config.{plugin}.{key}: unknown knob; not applied"
                )),
                Some(Err(e)) => {
                    warnings.push(format!("plugins.config.{plugin}.{key}: {e}; not applied"))
                }
                Some(Ok(EnvChange::Set(name, v))) => {
                    env.insert(name, v);
                }
                Some(Ok(EnvChange::Remove(_))) => {}
            }
        }
    }
    (env.into_iter().collect(), warnings)
}
