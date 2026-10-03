//! `compression.json` persistence: tolerant migration of the pre-pin shape, strict parse,
//! mcpm-compatible serialization (2-space indent, ASCII-escaped, trailing newline) and an
//! atomic owner-only write through the registry helper.

use super::model::{CompressionConfig, OrderedMap};
use crate::registry;
use serde::de::IgnoredAny;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::path::PathBuf;

pub const CONFIG_FILE: &str = "compression.json";
pub const SHIMS_FILE: &str = "compression-shims.zsh";
pub const ENV_SNIPPET_FILE: &str = "compression-env.sh";
pub const LAUNCHES_FILE: &str = "compression-launches.jsonl";
pub const SAVINGS_FILE: &str = "compression-savings.jsonl";

#[derive(Clone, Debug)]
pub struct Paths {
    pub dir: PathBuf,
}

impl Paths {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// The Toolport data directory, or `None` when it cannot be resolved.
    pub fn from_data_dir() -> Option<Self> {
        registry::conduit_dir().map(Self::new)
    }

    pub fn config(&self) -> PathBuf {
        self.dir.join(CONFIG_FILE)
    }

    pub fn shims(&self) -> PathBuf {
        self.dir.join(SHIMS_FILE)
    }

    pub fn env_snippet(&self) -> PathBuf {
        self.dir.join(ENV_SNIPPET_FILE)
    }

    pub fn launches(&self) -> PathBuf {
        self.dir.join(LAUNCHES_FILE)
    }

    pub fn savings(&self) -> PathBuf {
        self.dir.join(SAVINGS_FILE)
    }
}

#[derive(Debug)]
pub struct Loaded {
    pub config: CompressionConfig,
    pub notes: Vec<String>,
    pub existed: bool,
}

const FLAG_KNOBS: [(&str, &str); 2] = [
    ("code_aware", "HEADROOM_CODE_AWARE_ENABLED"),
    ("intercept_tool_results", "HEADROOM_INTERCEPT_ENABLED"),
];

/// Brings a parsed config up to the current schema. Idempotent; never fails on an odd
/// fragment, which is left for the strict parse to reject.
pub fn migrate_raw(mut raw: Value) -> (Value, Vec<String>) {
    let mut notes = Vec::new();
    let Some(obj) = raw.as_object_mut() else {
        return (raw, notes);
    };
    if let Some(Value::Object(presets)) = obj.get_mut("presets") {
        for (name, preset) in presets.iter_mut() {
            if let Some(preset) = preset.as_object_mut() {
                migrate_preset(name, preset, &mut notes);
            }
        }
    }
    if !obj.contains_key("provider_version") {
        obj.insert("provider_version".into(), json!({}));
        notes.push("seeded provider_version (exact pin; was an unpinned '>=' install)".into());
    }
    (raw, notes)
}

fn migrate_preset(name: &str, preset: &mut Map<String, Value>, notes: &mut Vec<String>) {
    let mut knobs: Map<String, Value> = match preset.get("knobs") {
        Some(Value::Object(k)) => k.clone(),
        _ => Map::new(),
    };
    if let Some(Value::Object(env)) = preset.remove("env") {
        if !env.is_empty() {
            let count = env.len();
            for (k, v) in env {
                let text = match &v {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                knobs
                    .entry(k)
                    .or_insert_with(|| json!({"value": text, "applies": "*", "source": "profile"}));
            }
            notes.push(format!(
                "preset '{name}': migrated {count} env keys -> knobs"
            ));
        }
    }
    for (flag, knob) in FLAG_KNOBS {
        if !preset.contains_key(flag) {
            continue;
        }
        let Some(Value::Bool(on)) = preset.remove(flag) else {
            continue;
        };
        let explicit = if on { "1" } else { "0" };
        let prior = knobs
            .get(knob)
            .and_then(|k| k.get("value"))
            .and_then(Value::as_str)
            .map(str::to_string);
        knobs.insert(
            knob.into(),
            json!({"value": explicit, "applies": "*", "source": "policy"}),
        );
        match prior {
            Some(prior) if prior != explicit => notes.push(format!(
                "preset '{name}': {flag}={} now declared explicitly as {knob}={explicit} \
                 (snapshot had {knob}={prior} \u{2014} the declared value was being ignored)",
                if on { "True" } else { "False" }
            )),
            _ => notes.push(format!(
                "preset '{name}': {flag}={} -> {knob}={explicit}",
                if on { "True" } else { "False" }
            )),
        }
    }
    if !knobs.is_empty() {
        preset.insert("knobs".into(), Value::Object(knobs));
    }
    preset.entry("snapshot_version").or_insert(Value::Null);
}

pub fn parse(text: &str) -> Result<(CompressionConfig, Vec<String>), String> {
    let raw: Value = serde_json::from_str(text).map_err(|e| format!("not valid JSON: {e}"))?;
    let (migrated, notes) = migrate_raw(raw);
    // Value objects are key-sorted. A file that needs no migration is parsed from its own
    // text instead, so preset and knob order survive; a legacy one is re-ordered afterwards.
    if notes.is_empty() {
        let config = serde_json::from_str(text).map_err(|e| format!("invalid policy: {e}"))?;
        return Ok((config, notes));
    }
    let mut config: CompressionConfig =
        serde_json::from_value(migrated).map_err(|e| format!("invalid policy: {e}"))?;
    restore_legacy_order(&mut config, text);
    Ok((config, notes))
}

#[derive(Deserialize)]
struct LegacyPreset {
    #[serde(default)]
    knobs: Option<OrderedMap<IgnoredAny>>,
    #[serde(default)]
    env: Option<OrderedMap<IgnoredAny>>,
}

#[derive(Deserialize)]
struct LegacyOrder {
    #[serde(default)]
    presets: Option<OrderedMap<LegacyPreset>>,
}

/// Python's migration order: existing knobs, then flattened env keys, then flag knobs.
fn restore_legacy_order(config: &mut CompressionConfig, text: &str) {
    let Ok(order) = serde_json::from_str::<LegacyOrder>(text) else {
        return;
    };
    let Some(presets) = order.presets else { return };
    let mut sorted = OrderedMap::new();
    for (name, _) in presets.iter() {
        if let Some(p) = config.presets.get(name) {
            sorted.insert(name.clone(), p.clone());
        }
    }
    for (name, p) in config.presets.iter() {
        if !sorted.contains_key(name) {
            sorted.insert(name.clone(), p.clone());
        }
    }
    config.presets = sorted;
    for (name, legacy) in presets.iter() {
        let Some(preset) = config.presets.get_mut(name) else {
            continue;
        };
        let mut wanted: Vec<String> = Vec::new();
        for list in [&legacy.knobs, &legacy.env].into_iter().flatten() {
            wanted.extend(list.keys().cloned());
        }
        wanted.extend(FLAG_KNOBS.iter().map(|(_, knob)| knob.to_string()));
        let mut knobs = OrderedMap::new();
        for key in &wanted {
            if let Some(spec) = preset.knobs.get(key) {
                knobs.insert(key.clone(), spec.clone());
            }
        }
        for (key, spec) in preset.knobs.iter() {
            if !knobs.contains_key(key) {
                knobs.insert(key.clone(), spec.clone());
            }
        }
        preset.knobs = knobs;
    }
}

/// A missing file is the default policy. A corrupt one is an error: unlike mcpm, which
/// silently falls back, a caller that went on to save would overwrite the user's file.
/// Never writes, so inspection leaves the data directory untouched.
pub fn read(paths: &Paths) -> Result<Loaded, String> {
    let path = paths.config();
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Loaded {
                config: CompressionConfig::default(),
                notes: Vec::new(),
                existed: false,
            })
        }
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let (config, notes) = parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(Loaded {
        config,
        notes,
        existed: true,
    })
}

/// [`read`], then persists a migration (best effort, as mcpm does) so it runs once.
pub fn load(paths: &Paths) -> Result<Loaded, String> {
    let loaded = read(paths)?;
    if !loaded.notes.is_empty() {
        let _ = save(paths, &loaded.config);
    }
    Ok(loaded)
}

/// `json.dumps(indent=2)` with `ensure_ascii`: every non-ASCII char sits inside a string,
/// so escaping at the text level is exact (surrogate pairs above the BMP).
pub fn render(config: &CompressionConfig) -> Result<String, String> {
    let pretty = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    let mut out = String::with_capacity(pretty.len() + 1);
    let mut units = [0u16; 2];
    for c in pretty.chars() {
        if c.is_ascii() {
            out.push(c);
        } else {
            for unit in c.encode_utf16(&mut units) {
                out.push_str(&format!("\\u{unit:04x}"));
            }
        }
    }
    out.push('\n');
    Ok(out)
}

pub fn save(paths: &Paths, config: &CompressionConfig) -> Result<(), String> {
    registry::atomic_write(&paths.config(), &render(config)?)
}
