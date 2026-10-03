//! The reconcile behind `enable`, `disable`, `set-provider`, `use` and `sync`: make the data
//! directory, the registry and the engine match one policy. Safe to run repeatedly; a dry run
//! reports the same steps in the conditional mood and writes nothing.

use super::engine::EngineOps;
use super::mcp_entry::{McpHost, MCP_NAME};
use super::model::*;
use super::provider::{activation_artifacts, mcp_server_config, GeneratedFile};
use super::store::{self, Paths};
use crate::registry;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const LEGACY_PLIST_LABEL: &str = "sh.mcpm.compression.proxy";

/// The launchd job mcpm used to generate; a leftover is removed by the next apply.
pub fn legacy_plist_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| {
        home.join("Library")
            .join("LaunchAgents")
            .join(format!("{LEGACY_PLIST_LABEL}.plist"))
    })
}

#[derive(Debug, Default)]
pub struct Report {
    pub dry_run: bool,
    pub actions: Vec<String>,
    pub warnings: Vec<String>,
    pub written: Vec<PathBuf>,
    pub removed: Vec<PathBuf>,
}

impl Report {
    pub fn new(dry_run: bool) -> Self {
        Self {
            dry_run,
            ..Self::default()
        }
    }

    pub fn did(&mut self, done: impl Into<String>, planned: impl Into<String>) {
        self.actions.push(if self.dry_run {
            planned.into()
        } else {
            done.into()
        });
    }

    pub fn warn(&mut self, text: impl Into<String>) {
        self.warnings.push(text.into());
    }

    pub fn put(&self, data: &mut Value) {
        let paths = |list: &[PathBuf]| -> Vec<String> {
            list.iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect()
        };
        data["dryRun"] = json!(self.dry_run);
        data["actions"] = json!(self.actions);
        data["warnings"] = json!(self.warnings);
        data["written"] = json!(paths(&self.written));
        data["removed"] = json!(paths(&self.removed));
    }
}

pub struct ApplyCtx<'a> {
    pub paths: &'a Paths,
    pub legacy_plist: Option<PathBuf>,
    pub dry_run: bool,
    pub teardown: bool,
}

fn routes_headroom(config: &CompressionConfig) -> bool {
    config.provider == ProviderName::Headroom
        || config
            .contexts
            .iter()
            .any(|r| r.provider.unwrap_or(config.provider) == ProviderName::Headroom)
}

/// Fills empty preset snapshots from the pinned build. An existing snapshot is never rewritten
/// here: `apply` runs on unrelated edits, and a knob change should always have a diff and a
/// decision behind it (`presets --refresh`).
fn materialize_presets(
    config: &mut CompressionConfig,
    engine: &mut dyn EngineOps,
    report: &mut Report,
) {
    if !routes_headroom(config) {
        return;
    }
    for (name, preset) in config.presets.iter_mut() {
        let Some(profile) = preset.savings_profile.clone() else {
            continue;
        };
        if !preset.knobs.is_empty() {
            continue;
        }
        match engine.agent_savings(&profile) {
            Ok(env) => {
                preset.knobs = OrderedMap::new();
                for (k, v) in env {
                    preset
                        .knobs
                        .insert(k, KnobSpec::with_source(v, KnobSource::Profile));
                }
                preset.snapshot_version = engine.installed_version();
                let version = preset.snapshot_version.as_deref().unwrap_or("unknown");
                report.did(
                    format!(
                        "snapshot preset '{name}' knobs from headroom {version} (profile '{profile}')"
                    ),
                    format!(
                        "would snapshot preset '{name}' knobs from headroom {version} (profile '{profile}')"
                    ),
                );
            }
            Err(why) => report.warn(format!("preset '{name}': {why}")),
        }
    }
}

fn mode_of(path: &Path) -> Option<u32> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata().ok().map(|m| m.permissions().mode() & 0o777)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        None
    }
}

fn write_artifact(file: &GeneratedFile) -> Result<(), String> {
    let unchanged = std::fs::read_to_string(&file.path).is_ok_and(|t| t == file.content)
        && mode_of(&file.path).is_none_or(|m| m == file.mode);
    if unchanged {
        return Ok(());
    }
    registry::atomic_write(&file.path, &file.content)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&file.path, std::fs::Permissions::from_mode(file.mode))
            .map_err(|e| format!("{}: {e}", file.path.display()))?;
    }
    Ok(())
}

fn mcp_step(
    config: &CompressionConfig,
    ctx: &ApplyCtx,
    mcp: &mut dyn McpHost,
    report: &mut Report,
) {
    if mcp_server_config(config.provider).is_some() {
        match mcp.register(ctx.dry_run) {
            Ok(done) => report.did(
                format!(
                    "registered MCP server '{MCP_NAME}' in the registry (enabled in profile '{}')",
                    done.profile
                ),
                format!(
                    "would register MCP server '{MCP_NAME}' in the registry (enabled in profile '{}')",
                    done.profile
                ),
            ),
            Err(why) => report.warn(format!("MCP registration failed ({why})")),
        }
        return;
    }
    match mcp.unregister(ctx.dry_run) {
        Ok(Some(_)) => report.did(
            format!("removed MCP server '{MCP_NAME}' from the registry"),
            format!("would remove MCP server '{MCP_NAME}' from the registry"),
        ),
        Ok(None) => {}
        Err(why) => report.warn(format!("MCP teardown failed ({why})")),
    }
}

/// Stale artifacts go first, then the current set is written. A file that is about to be
/// rewritten is not removed in between.
fn artifact_step(config: &CompressionConfig, ctx: &ApplyCtx, report: &mut Report) {
    let current = activation_artifacts(config.provider, config, ctx.paths);
    let stale = [Some(ctx.paths.env_snippet()), ctx.legacy_plist.clone()];
    for path in stale.into_iter().flatten() {
        if current.iter().any(|f| f.path == path) || !path.exists() {
            continue;
        }
        if !ctx.dry_run {
            if let Err(e) = std::fs::remove_file(&path) {
                report.warn(format!("could not remove {}: {e}", path.display()));
                continue;
            }
        }
        report.removed.push(path.clone());
        report.did(
            format!("removed artifact {}", path.display()),
            format!("would remove artifact {}", path.display()),
        );
    }
    for file in &current {
        if !ctx.dry_run {
            if let Err(why) = write_artifact(file) {
                report.warn(format!("could not write {}: {why}", file.path.display()));
                continue;
            }
        }
        report.written.push(file.path.clone());
        let note = if file.note.is_empty() {
            String::new()
        } else {
            format!("  ({})", file.note)
        };
        report.did(
            format!("wrote {}{note}", file.path.display()),
            format!("would write {}{note}", file.path.display()),
        );
    }
}

fn selected(config: &CompressionConfig) -> Vec<ProviderName> {
    let mut keep = vec![config.provider];
    keep.extend(config.contexts.iter().filter_map(|r| r.provider));
    keep
}

fn engine_step(
    engine: &mut dyn EngineOps,
    ctx: &ApplyCtx,
    report: &mut Report,
    label: &str,
    args: &[&str],
) {
    let text = format!("headroom {label}");
    if ctx.dry_run {
        report.did(String::new(), format!("would run `{text}`"));
        return;
    }
    match engine.run_headroom(args) {
        Ok(detail) => report.did(format!("{text} \u{2014} {detail}"), String::new()),
        Err(detail) => report.warn(format!("{text} \u{2014} {detail}")),
    }
}

/// Engages the selected provider and disengages the others. Only headroom has a deeper
/// removal (`--teardown`); parsec's plugin lifecycle belongs to Claude Code and is reported,
/// never driven from here.
fn provider_step(
    config: &CompressionConfig,
    ctx: &ApplyCtx,
    engine: &mut dyn EngineOps,
    report: &mut Report,
) {
    if config.provider == ProviderName::Parsec {
        report.warn(
            "parsec is a Claude Code plugin; install and enable it with `claude plugin`, \
             Toolport+ does not drive it",
        );
    }
    let keep = selected(config);
    if ctx.teardown && !keep.contains(&ProviderName::Headroom) {
        engine_step(engine, ctx, report, "mcp uninstall", &["mcp", "uninstall"]);
        engine_step(engine, ctx, report, "unwrap claude", &["unwrap", "claude"]);
    }
}

/// Makes the world match `config`. `config` is updated in place (snapshots), and persisted
/// unless this is a dry run.
pub fn apply(
    config: &mut CompressionConfig,
    ctx: &ApplyCtx,
    engine: &mut dyn EngineOps,
    mcp: &mut dyn McpHost,
    report: &mut Report,
) -> Result<(), String> {
    materialize_presets(config, engine, report);
    if !ctx.dry_run {
        store::save(ctx.paths, config)?;
    }
    let provider = config.provider.as_str();
    report.did(
        format!("saved config (provider={provider})"),
        format!("would save config (provider={provider})"),
    );
    mcp_step(config, ctx, mcp, report);
    artifact_step(config, ctx, report);
    provider_step(config, ctx, engine, report);
    Ok(())
}
