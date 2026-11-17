//! Declarative compression policy (`compression.json`), field-for-field compatible with
//! mcpm-compression's pydantic schema: same field order, same defaults, same enum spellings.
//! Unknown fields are kept so a newer file survives a round trip through this build.

use serde::de::{MapAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashSet};
use std::fmt;
use std::marker::PhantomData;

pub const DEFAULT_PORT: u16 = 8787;
pub const AGENT_PORT: u16 = 8788;
pub const DEFAULT_PIN: &str = "0.29.0";

pub type Extra = BTreeMap<String, Value>;

#[derive(Clone, Debug, PartialEq)]
pub struct OrderedMap<T>(Vec<(String, T)>);

impl<T> Default for OrderedMap<T> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

impl<T> OrderedMap<T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn get(&self, key: &str) -> Option<&T> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut T> {
        self.0.iter_mut().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// Replaces in place when the key exists, like a Python dict assignment.
    pub fn insert(&mut self, key: impl Into<String>, value: T) {
        let key = key.into();
        match self.0.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1 = value,
            None => self.0.push((key, value)),
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &T)> {
        self.0.iter().map(|(k, v)| (k, v))
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&String, &mut T)> {
        self.0.iter_mut().map(|(k, v)| (&*k, v))
    }

    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.0.iter().map(|(k, _)| k)
    }
}

impl<T: Serialize> Serialize for OrderedMap<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (k, v) in &self.0 {
            map.serialize_entry(k, v)?;
        }
        map.end()
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for OrderedMap<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct V<T>(PhantomData<T>);
        impl<'de, T: Deserialize<'de>> Visitor<'de> for V<T> {
            type Value = OrderedMap<T>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a map")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<Self::Value, A::Error> {
                let mut out = OrderedMap::new();
                while let Some((k, v)) = access.next_entry::<String, T>()? {
                    out.insert(k, v);
                }
                Ok(out)
            }
        }
        deserializer.deserialize_map(V(PhantomData))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderName {
    #[serde(rename = "headroom")]
    Headroom,
    #[serde(rename = "rtk-only")]
    RtkOnly,
    #[serde(rename = "parsec")]
    Parsec,
    #[serde(rename = "none")]
    None,
}

impl ProviderName {
    pub const ALL: [ProviderName; 4] = [
        ProviderName::Headroom,
        ProviderName::RtkOnly,
        ProviderName::Parsec,
        ProviderName::None,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            ProviderName::Headroom => "headroom",
            ProviderName::RtkOnly => "rtk-only",
            ProviderName::Parsec => "parsec",
            ProviderName::None => "none",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.as_str() == text)
    }

    pub fn default_runtime(self) -> RuntimeKind {
        match self {
            ProviderName::Headroom => RuntimeKind::Proxy,
            ProviderName::RtkOnly => RuntimeKind::Hook,
            ProviderName::Parsec => RuntimeKind::Plugin,
            ProviderName::None => RuntimeKind::None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeKind {
    Proxy,
    Hook,
    Plugin,
    None,
}

impl RuntimeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            RuntimeKind::Proxy => "proxy",
            RuntimeKind::Hook => "hook",
            RuntimeKind::Plugin => "plugin",
            RuntimeKind::None => "none",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CompressionMode {
    Cache,
    Token,
}

impl CompressionMode {
    pub fn as_str(self) -> &'static str {
        match self {
            CompressionMode::Cache => "cache",
            CompressionMode::Token => "token",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KnobSource {
    Profile,
    Policy,
}

fn d_package() -> String {
    "headroom-ai".into()
}
fn d_pin() -> String {
    DEFAULT_PIN.into()
}
fn d_extras() -> Vec<String> {
    vec!["proxy".into(), "code".into(), "ml".into()]
}
fn d_applies() -> String {
    "*".into()
}
fn d_source() -> KnobSource {
    KnobSource::Policy
}
fn d_mode() -> CompressionMode {
    CompressionMode::Cache
}
fn d_port() -> u16 {
    DEFAULT_PORT
}
fn d_provider() -> ProviderName {
    ProviderName::None
}
fn d_runtime() -> RuntimeKind {
    RuntimeKind::None
}
fn d_scope() -> Vec<String> {
    vec!["default".into()]
}
fn d_clients() -> Vec<String> {
    vec!["claude-code".into()]
}
fn d_active() -> String {
    "interactive".into()
}

/// The exact provider build the policy targets. The requirement is `==pin`, never a floor.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProviderVersion {
    #[serde(default = "d_package")]
    pub package: String,
    #[serde(default = "d_pin")]
    pub pin: String,
    #[serde(default = "d_extras")]
    pub extras: Vec<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Default for ProviderVersion {
    fn default() -> Self {
        Self {
            package: d_package(),
            pin: d_pin(),
            extras: d_extras(),
            extra: Extra::new(),
        }
    }
}

impl ProviderVersion {
    pub fn requirement(&self) -> String {
        let extras = if self.extras.is_empty() {
            String::new()
        } else {
            format!("[{}]", self.extras.join(","))
        };
        format!("{}{}=={}", self.package, extras, self.pin)
    }
}

/// One declared `HEADROOM_*` value. `applies` is authored intent (a version range);
/// `source` says whether a re-snapshot may replace it (`profile`) or must keep it (`policy`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KnobSpec {
    pub value: String,
    #[serde(default = "d_applies")]
    pub applies: String,
    #[serde(default = "d_source")]
    pub source: KnobSource,
    #[serde(flatten)]
    pub extra: Extra,
}

impl KnobSpec {
    pub fn new(value: impl Into<String>) -> Self {
        Self::with_source(value, KnobSource::Policy)
    }

    pub fn with_source(value: impl Into<String>, source: KnobSource) -> Self {
        Self {
            value: value.into(),
            applies: d_applies(),
            source,
            extra: Extra::new(),
        }
    }

    #[cfg(test)]
    pub fn applies_to(mut self, spec: &str) -> Self {
        self.applies = spec.into();
        self
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CompressionPreset {
    #[serde(default = "d_mode")]
    pub mode: CompressionMode,
    #[serde(default)]
    pub savings_profile: Option<String>,
    #[serde(default)]
    pub knobs: OrderedMap<KnobSpec>,
    #[serde(default)]
    pub snapshot_version: Option<String>,
    #[serde(default = "d_port")]
    pub port: u16,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Default for CompressionPreset {
    fn default() -> Self {
        Self {
            mode: d_mode(),
            savings_profile: None,
            knobs: OrderedMap::new(),
            snapshot_version: None,
            port: DEFAULT_PORT,
            extra: Extra::new(),
        }
    }
}

impl CompressionPreset {
    pub fn new(mode: CompressionMode, savings_profile: Option<&str>, port: u16) -> Self {
        Self {
            mode,
            savings_profile: savings_profile.map(str::to_string),
            port,
            ..Self::default()
        }
    }
}

pub fn default_presets() -> OrderedMap<CompressionPreset> {
    let mut presets = OrderedMap::new();
    presets.insert(
        "interactive",
        CompressionPreset::new(CompressionMode::Cache, None, DEFAULT_PORT),
    );
    presets.insert(
        "agent",
        CompressionPreset::new(CompressionMode::Token, Some("agent-90"), AGENT_PORT),
    );
    presets.insert(
        "balanced",
        CompressionPreset::new(CompressionMode::Token, Some("balanced"), DEFAULT_PORT),
    );
    presets
}

/// Overrides provider and/or preset when the launch cwd matches the glob. First match wins.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ContextRule {
    #[serde(rename = "match")]
    pub pattern: String,
    #[serde(default)]
    pub provider: Option<ProviderName>,
    #[serde(default)]
    pub preset: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl ContextRule {
    #[cfg(test)]
    pub fn new(pattern: &str, provider: Option<ProviderName>, preset: Option<&str>) -> Self {
        Self {
            pattern: pattern.into(),
            provider,
            preset: preset.map(str::to_string),
            extra: Extra::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CompressionConfig {
    #[serde(default = "d_provider")]
    pub provider: ProviderName,
    #[serde(default = "d_runtime")]
    pub runtime: RuntimeKind,
    #[serde(default)]
    pub provider_version: ProviderVersion,
    #[serde(default = "d_scope")]
    pub scope: Vec<String>,
    #[serde(default = "d_clients")]
    pub clients: Vec<String>,
    #[serde(default)]
    pub contexts: Vec<ContextRule>,
    #[serde(default = "default_presets")]
    pub presets: OrderedMap<CompressionPreset>,
    #[serde(default = "d_active")]
    pub active_preset: String,
    #[serde(default)]
    pub options: Map<String, Value>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Default for CompressionConfig {
    fn default() -> Self {
        Self {
            provider: d_provider(),
            runtime: d_runtime(),
            provider_version: ProviderVersion::default(),
            scope: d_scope(),
            clients: d_clients(),
            contexts: Vec::new(),
            presets: default_presets(),
            active_preset: d_active(),
            options: Map::new(),
            extra: Extra::new(),
        }
    }
}

impl CompressionConfig {
    #[cfg(test)]
    pub fn with_provider(provider: ProviderName) -> Self {
        Self {
            provider,
            ..Self::default()
        }
    }

    #[cfg(test)]
    pub fn legacy_port(&self) -> u16 {
        match self.options.get("port") {
            Some(Value::Number(n)) => n.as_u64().and_then(|p| u16::try_from(p).ok()),
            Some(Value::String(s)) => s.trim().parse().ok(),
            _ => None,
        }
        .unwrap_or(DEFAULT_PORT)
    }

    /// Privacy default is off.
    pub fn telemetry(&self) -> String {
        match self.options.get("telemetry") {
            Some(Value::String(s)) => s.clone(),
            Some(other) => other.to_string(),
            None => "off".into(),
        }
    }

    /// The named preset, the active one, or a safe default; never fails.
    pub fn preset_for(&self, name: Option<&str>) -> CompressionPreset {
        let key = name.unwrap_or(&self.active_preset);
        self.presets
            .get(key)
            .or_else(|| self.presets.get("interactive"))
            .cloned()
            .unwrap_or_default()
    }

    /// Active (provider, preset name) for a launch cwd: first matching context, else default.
    pub fn resolve(&self, cwd: &str) -> (ProviderName, String) {
        for rule in &self.contexts {
            if fnmatch(&rule.pattern, cwd) {
                return (
                    rule.provider.unwrap_or(self.provider),
                    rule.preset
                        .clone()
                        .unwrap_or_else(|| self.active_preset.clone()),
                );
            }
        }
        (self.provider, self.active_preset.clone())
    }

    #[cfg(test)]
    pub fn resolved_provider(&self, cwd: &str) -> ProviderName {
        self.resolve(cwd).0
    }
}

/// Python `fnmatch.fnmatchcase` semantics: `*` crosses `/`, `?`, `[seq]`, `[!seq]`.
pub fn fnmatch(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    fn class_end(p: &[char], open: usize) -> Option<usize> {
        let mut j = open + 1;
        if j < p.len() && p[j] == '!' {
            j += 1;
        }
        if j < p.len() && p[j] == ']' {
            j += 1;
        }
        while j < p.len() && p[j] != ']' {
            j += 1;
        }
        (j < p.len()).then_some(j)
    }
    fn class_has(set: &[char], c: char) -> bool {
        let mut i = 0;
        while i < set.len() {
            if i + 2 < set.len() && set[i + 1] == '-' {
                if set[i] <= c && c <= set[i + 2] {
                    return true;
                }
                i += 3;
            } else {
                if set[i] == c {
                    return true;
                }
                i += 1;
            }
        }
        false
    }
    fn go(p: &[char], t: &[char], failed: &mut HashSet<(usize, usize)>) -> bool {
        if failed.contains(&(p.len(), t.len())) {
            return false;
        }
        let matched = match p.first() {
            None => t.is_empty(),
            Some(&first) => match first {
                '*' => (0..=t.len()).any(|skip| go(&p[1..], &t[skip..], failed)),
                '?' => !t.is_empty() && go(&p[1..], &t[1..], failed),
                '[' => match class_end(p, 0) {
                    Some(end) => {
                        let Some(&c) = t.first() else { return false };
                        let (negate, set) = match p[1] {
                            '!' => (true, &p[2..end]),
                            _ => (false, &p[1..end]),
                        };
                        class_has(set, c) != negate && go(&p[end + 1..], &t[1..], failed)
                    }
                    None => t.first() == Some(&'[') && go(&p[1..], &t[1..], failed),
                },
                c => t.first() == Some(&c) && go(&p[1..], &t[1..], failed),
            },
        };
        if !matched {
            failed.insert((p.len(), t.len()));
        }
        matched
    }
    go(&p, &t, &mut HashSet::new())
}

/// `X.Y.Z` to a comparable tuple; empty for unknown or unparseable.
pub fn version_tuple(version: Option<&str>) -> Vec<u64> {
    let Some(v) = version else { return Vec::new() };
    let parts: Vec<&str> = v.trim().split('.').take(3).collect();
    parts
        .iter()
        .map(|p| p.trim().parse::<u64>().ok())
        .collect::<Option<Vec<_>>>()
        .unwrap_or_default()
}

fn clause_matches(version: &[u64], clause: &str) -> bool {
    let clause = clause.trim();
    if clause.is_empty() || clause == "*" {
        return true;
    }
    for op in ["<=", ">=", "==", "!=", "<", ">"] {
        if let Some(rest) = clause.strip_prefix(op) {
            let rhs = version_tuple(Some(rest));
            if rhs.is_empty() {
                return false;
            }
            let v = version.to_vec();
            return match op {
                "==" => v == rhs,
                "!=" => v != rhs,
                ">=" => v >= rhs,
                "<=" => v <= rhs,
                ">" => v > rhs,
                _ => v < rhs,
            };
        }
    }
    let rhs = version_tuple(Some(clause));
    !rhs.is_empty() && version == rhs.as_slice()
}

/// Does `version` satisfy `spec` (`*` or comma-joined clauses)? An unknown version
/// matches only `*`: a build is never guessed into a range. A spec of nothing but commas
/// holds no clause and matches nothing.
pub fn spec_matches(version: Option<&str>, spec: &str) -> bool {
    let spec = spec.trim();
    let spec = if spec.is_empty() { "*" } else { spec };
    if spec == "*" {
        return true;
    }
    let parsed = version_tuple(version);
    if parsed.is_empty() {
        return false;
    }
    let mut clauses = spec.split(',').filter(|c| !c.trim().is_empty()).peekable();
    clauses.peek().is_some() && clauses.all(|c| clause_matches(&parsed, c))
}

/// The launch env for a preset against the pinned build. Every declared knob whose range
/// covers the pin is emitted literally, including the offs: headroom seeds persona defaults
/// into anything left unset, so "off" must be an explicit `0`. A knob is dropped only when
/// its authored range excludes the pin, never because the build might not read it.
pub fn env_for_preset(
    config: &CompressionConfig,
    preset: &CompressionPreset,
) -> OrderedMap<String> {
    let pin = config.provider_version.pin.as_str();
    let mut env = OrderedMap::new();
    for (name, knob) in preset.knobs.iter() {
        if spec_matches(Some(pin), &knob.applies) {
            env.insert(name.clone(), knob.value.clone());
        }
    }
    env.insert(
        "ANTHROPIC_BASE_URL",
        format!("http://127.0.0.1:{}", preset.port),
    );
    // Without it a custom base URL makes Claude Code eager-load every tool schema.
    env.insert("ENABLE_TOOL_SEARCH", "true".to_string());
    env.insert("HEADROOM_MODE", preset.mode.as_str().to_string());
    env.insert("HEADROOM_TELEMETRY", config.telemetry());
    env
}
