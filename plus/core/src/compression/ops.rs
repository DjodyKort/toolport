//! Policy operations as pure functions over the model: enable, set-provider, use, pin,
//! update, snapshot refresh and seal. Nothing here touches disk, the network or a binary.

use super::model::*;

#[derive(Clone, Debug, Default)]
pub struct EnableOpts {
    pub provider: Option<ProviderName>,
    pub port: Option<u16>,
    pub telemetry: Option<String>,
    pub preset: Option<String>,
    pub mode: Option<CompressionMode>,
}

fn unknown_preset(config: &CompressionConfig, name: &str) -> String {
    let have: Vec<&str> = config.presets.keys().map(String::as_str).collect();
    format!("unknown preset '{name}' (have: {})", have.join(", "))
}

/// Load-merge semantics: contexts, clients, scope, presets and unrelated options survive.
pub fn enable(config: &mut CompressionConfig, opts: &EnableOpts) -> Result<(), String> {
    let provider = opts.provider.unwrap_or(ProviderName::Headroom);
    config.provider = provider;
    config.runtime = provider.default_runtime();
    if let Some(name) = &opts.preset {
        if !config.presets.contains_key(name) {
            return Err(unknown_preset(config, name));
        }
        config.active_preset = name.clone();
    }
    let active = config.active_preset.clone();
    if let Some(preset) = config.presets.get_mut(&active) {
        if let Some(mode) = opts.mode {
            preset.mode = mode;
        }
        if let Some(port) = opts.port {
            preset.port = port;
        }
    }
    if let Some(telemetry) = &opts.telemetry {
        config.options.insert(
            "telemetry".into(),
            serde_json::Value::String(telemetry.clone()),
        );
    }
    Ok(())
}

pub fn set_provider(config: &mut CompressionConfig, provider: ProviderName) {
    config.provider = provider;
    config.runtime = provider.default_runtime();
}

pub fn use_preset(config: &mut CompressionConfig, name: &str) -> Result<(), String> {
    if !config.presets.contains_key(name) {
        return Err(unknown_preset(config, name));
    }
    config.active_preset = name.into();
    Ok(())
}

/// Disabling selects `none`; presets, contexts and the pin stay in the file.
pub fn disable(config: &mut CompressionConfig) {
    set_provider(config, ProviderName::None);
}

pub fn set_pin(config: &mut CompressionConfig, version: &str) {
    config.provider_version.pin = version.into();
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateError {
    Conflict,
    Unresolvable,
    Unparseable(String),
}

impl UpdateError {
    pub fn message(&self) -> String {
        match self {
            UpdateError::Conflict => "pass either --to or --latest, not both".into(),
            UpdateError::Unresolvable => "could not resolve the latest version (offline?); \
                                          name it explicitly with --to"
                .into(),
            UpdateError::Unparseable(v) => format!("'{v}' is not a parseable X.Y.Z version"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateTarget {
    pub current: String,
    pub target: String,
    pub same: bool,
}

/// The new exact pin for `update`. `latest` is a lookup the caller supplies, so the network
/// stays out of here and `--to` never invokes it.
pub fn resolve_update(
    config: &CompressionConfig,
    to: Option<&str>,
    to_latest: bool,
    latest: impl FnOnce() -> Option<String>,
) -> Result<UpdateTarget, UpdateError> {
    if to.is_some() && to_latest {
        return Err(UpdateError::Conflict);
    }
    let target = match to {
        Some(v) => v.to_string(),
        None => latest().ok_or(UpdateError::Unresolvable)?,
    };
    if version_tuple(Some(&target)).is_empty() {
        return Err(UpdateError::Unparseable(target));
    }
    let current = config.provider_version.pin.clone();
    Ok(UpdateTarget {
        same: target == current,
        current,
        target,
    })
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SnapshotDiff {
    pub added: Vec<(String, String)>,
    pub removed: Vec<(String, String)>,
    pub moved: Vec<(String, String, String)>,
    pub kept: Vec<String>,
}

impl SnapshotDiff {
    pub fn changed(&self) -> bool {
        !(self.added.is_empty() && self.removed.is_empty() && self.moved.is_empty())
    }
}

/// Re-snapshots one preset from `profile`. Only `profile` knobs are replaced; `policy`
/// declarations survive and outrank the vendor value for the same knob, because dropping
/// one hands the knob back to the vendor's default.
pub fn apply_snapshot(
    preset: &mut CompressionPreset,
    profile: &[(String, String)],
    live_version: Option<&str>,
) -> SnapshotDiff {
    let before: Vec<(String, String)> = preset
        .knobs
        .iter()
        .map(|(k, s)| (k.clone(), s.value.clone()))
        .collect();
    let kept: Vec<(String, KnobSpec)> = preset
        .knobs
        .iter()
        .filter(|(_, s)| s.source == KnobSource::Policy)
        .map(|(k, s)| (k.clone(), s.clone()))
        .collect();
    let mut merged = OrderedMap::new();
    for (k, v) in profile {
        merged.insert(
            k.clone(),
            KnobSpec::with_source(v.clone(), KnobSource::Profile),
        );
    }
    for (k, spec) in &kept {
        merged.insert(k.clone(), spec.clone());
    }
    let after: Vec<(String, String)> = merged
        .iter()
        .map(|(k, s)| (k.clone(), s.value.clone()))
        .collect();
    let lookup = |list: &[(String, String)], key: &str| {
        list.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
    };
    let mut diff = SnapshotDiff::default();
    for (k, v) in &after {
        match lookup(&before, k) {
            None => diff.added.push((k.clone(), v.clone())),
            Some(old) if &old != v => diff.moved.push((k.clone(), old, v.clone())),
            Some(_) => {}
        }
    }
    for (k, v) in &before {
        if lookup(&after, k).is_none() {
            diff.removed.push((k.clone(), v.clone()));
        }
    }
    diff.added.sort();
    diff.removed.sort();
    diff.moved.sort();
    diff.kept = kept.iter().map(|(k, _)| k.clone()).collect();
    diff.kept.sort();
    preset.knobs = merged;
    preset.snapshot_version = live_version.map(str::to_string);
    diff
}

/// Declares the posture a live proxy already runs as policy knobs; behaviour-preserving.
pub fn seal(preset: &mut CompressionPreset, declarable: &[(String, String)]) -> usize {
    for (k, v) in declarable {
        preset.knobs.insert(k.clone(), KnobSpec::new(v.clone()));
    }
    declarable.len()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PinGuard {
    Match,
    Forced(String),
    Refused(String),
}

/// The launch kill switch: a build other than the pin (newer included) is unverified, and
/// losing compression is cheap while losing the prompt cache is not.
pub fn pin_guard(pin: &str, installed: Option<&str>, force: bool) -> PinGuard {
    if installed == Some(pin) {
        return PinGuard::Match;
    }
    let what = match installed {
        Some(v) => format!("installed {v}"),
        None => "headroom not on PATH".to_string(),
    };
    if force {
        PinGuard::Forced(format!("{what} != pin {pin}; routing anyway (--force)"))
    } else {
        PinGuard::Refused(format!(
            "{what} != pin {pin}; not routing, launching plain claude"
        ))
    }
}
