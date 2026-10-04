//! Where every skill, command, agent, rule and CLAUDE.md comes from (D-063).
//!
//! One [`SourceDetector`] per kind of place. Detectors are read-only (they only get the helpers
//! in [`fsx`] and [`gitx`]), bounded by a [`Budget`] and cached by [`Cache`]; a budget hit makes
//! the scan `partial` and is reported in `skipped`, never raised as an error.

pub mod budget;
pub mod cache;
pub mod fsx;
pub mod gitx;
pub mod item;
pub mod layout;
pub mod model;
pub mod render;
pub mod roots;
pub mod scope;

mod account;
mod client;
mod inert;
mod library;
mod loose;
mod org;
mod plugin;
mod repo;
mod tap;
mod vendored;

#[cfg(test)]
mod tests;

use crate::plus::context::{ContextConfig, Roots};
use budget::Budget;
use cache::Cache;
pub use library::invisible_reason;
pub use model::{Item, Source};
use scope::RootSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub struct ScanCtx<'a> {
    pub roots: &'a Roots,
    pub config: &'a ContextConfig,
    pub data_dir: Option<&'a Path>,
    pub cwd: Option<&'a Path>,
    pub scope: &'a RootSet,
    pub budget: &'a Budget,
    pub cache: &'a Cache,
    pub now: &'a str,
}

#[derive(Default)]
pub struct DetectorOutput {
    pub sources: Vec<Source>,
    pub items: Vec<Item>,
}

pub trait SourceDetector {
    fn id(&self) -> &'static str;
    fn scan(&self, ctx: &ScanCtx) -> DetectorOutput;
}

/// In result order: repo-native first, then plugin, org, account, library, tap, loose, inert.
pub fn detectors() -> Vec<Box<dyn SourceDetector>> {
    vec![
        Box::new(repo::RepoDetector),
        Box::new(client::ClientDetector),
        Box::new(vendored::VendoredDetector),
        Box::new(plugin::PluginDetector),
        Box::new(org::OrgDetector),
        Box::new(account::AccountDetector),
        Box::new(library::LibraryDetector),
        Box::new(tap::TapDetector),
        Box::new(loose::LooseDetector),
        Box::new(inert::InertDetector),
    ]
}

#[derive(Clone, Debug, Default)]
pub struct ScanOptions {
    pub source: Option<String>,
    pub kind: Option<String>,
    pub items: bool,
    pub cwd: Option<PathBuf>,
    pub roots: Vec<PathBuf>,
    pub deep: bool,
    pub refresh: bool,
    pub budgets_ms: Vec<(String, u64)>,
    pub max_entries: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Skipped {
    pub detector: String,
    pub reason: String,
}

#[derive(Debug)]
pub struct ScanReport {
    pub generated_at: String,
    pub partial: bool,
    pub skipped: Vec<Skipped>,
    pub sources: Vec<Source>,
    pub items: Vec<Item>,
    pub cache_hits: usize,
    pub cache_misses: usize,
}

/// A selector names a detector (`plugin`), a source (`plugin:ecc@ecc`) or both at once
/// (`library`).
fn selector_detector(selector: &str) -> &str {
    selector.split_once(':').map_or(selector, |(head, _)| head)
}

fn selected(source: &Source, selector: &str) -> bool {
    source.id == selector || source.detector == selector || source.origin.kind == selector
}

/// The roots and `context.json` of this machine, as every caller of the scan sees them.
pub fn host_world() -> Option<(Roots, ContextConfig)> {
    let home = crate::clients::home()?;
    let mut roots = Roots::from_home(&home);
    roots.read_env();
    if let Some(dir) = std::env::var("CLAUDE_CONFIG_DIR")
        .ok()
        .filter(|d| !d.is_empty())
    {
        roots.claude_home = roots.expand_user(&dir);
        roots.env_claude_config_dir = Some(dir);
    }
    let config = crate::plus::context::load_config(&roots.context_config_path());
    Some((roots, config))
}

/// [`scan`] over [`host_world`] and the data directory; `None` when there is no home directory.
pub fn scan_host(opts: &ScanOptions) -> Option<ScanReport> {
    let (roots, config) = host_world()?;
    let data_dir = crate::registry::conduit_dir();
    Some(scan(&roots, &config, data_dir.as_deref(), opts))
}

pub fn scan(
    roots: &Roots,
    config: &ContextConfig,
    data_dir: Option<&Path>,
    opts: &ScanOptions,
) -> ScanReport {
    let started = Instant::now();
    let scale = if opts.deep { 2 } else { 1 };
    let time_scale = scale * budget::time_scale();
    let cache = match data_dir {
        Some(dir) => Cache::open(cache::path_in(dir), opts.refresh),
        None => Cache::memory(),
    };
    let now = fsx::now_zulu();
    let probe = Budget::new(
        budget::DETECTOR_TIME * time_scale,
        budget::MAX_DEPTH * scale as usize,
        None,
    );
    let scope = scope::compute(roots, config, opts, &probe);
    let mut sources = Vec::new();
    let mut items = Vec::new();
    let mut skipped = Vec::new();
    for detector in detectors() {
        let id = detector.id();
        if let Some(selector) = &opts.source {
            if selector_detector(selector) != id && selector != id {
                continue;
            }
        }
        let elapsed = started.elapsed();
        let total = budget::TOTAL_TIME * time_scale;
        let own = opts
            .budgets_ms
            .iter()
            .find(|(name, _)| name == id)
            .map(|(_, ms)| Duration::from_millis(*ms))
            .unwrap_or(budget::DETECTOR_TIME * time_scale);
        let allowed = own.min(total.saturating_sub(elapsed));
        let budget = Budget::new(
            allowed,
            budget::MAX_DEPTH * scale as usize,
            opts.max_entries,
        );
        if allowed.is_zero() || budget.spent() {
            skipped.push(Skipped {
                detector: id.to_string(),
                reason: if own.is_zero() {
                    "time budget exhausted".into()
                } else {
                    "total time budget exhausted".into()
                },
            });
            continue;
        }
        let ctx = ScanCtx {
            roots,
            config,
            data_dir,
            cwd: opts.cwd.as_deref(),
            scope: &scope,
            budget: &budget,
            cache: &cache,
            now: &now,
        };
        let mut out = detector.scan(&ctx);
        if let Some(reason) = budget.hit() {
            for source in &mut out.sources {
                if source.status.state == "ok" {
                    source.status.state = "partial";
                    source.status.detail = format!("scan stopped early: {reason}");
                }
            }
            skipped.push(Skipped {
                detector: id.to_string(),
                reason,
            });
        }
        sources.append(&mut out.sources);
        items.append(&mut out.items);
    }
    shadow_pass(&mut sources, &mut items);
    let partial = !skipped.is_empty();
    cache.flush(!partial);
    finish(opts, &now, partial, skipped, sources, items, &cache)
}

fn finish(
    opts: &ScanOptions,
    now: &str,
    partial: bool,
    skipped: Vec<Skipped>,
    mut sources: Vec<Source>,
    mut items: Vec<Item>,
    cache: &Cache,
) -> ScanReport {
    if let Some(selector) = &opts.source {
        sources.retain(|s| selected(s, selector));
        items.retain(|i| sources.iter().any(|s| s.id == i.source_id));
    }
    if let Some(kind) = &opts.kind {
        items.retain(|i| i.kind == kind);
        sources.retain(|s| match kind.as_str() {
            "skill" => s.counts.skill > 0,
            "command" => s.counts.command > 0,
            "agent" => s.counts.agent > 0,
            "rule" => s.counts.rule > 0,
            _ => s.counts.memory > 0,
        });
    }
    items.sort_by(|a, b| {
        let order = |s: &str| sources.iter().position(|x| x.id == s).unwrap_or(usize::MAX);
        order(&a.source_id)
            .cmp(&order(&b.source_id))
            .then(model::kind_rank(a.kind).cmp(&model::kind_rank(b.kind)))
            .then(a.name.cmp(&b.name))
    });
    ScanReport {
        generated_at: now.to_string(),
        partial,
        skipped,
        sources,
        items: if opts.items { items } else { Vec::new() },
        cache_hits: cache.hits(),
        cache_misses: cache.misses(),
    }
}

/// D-073: an org command that shares its name with a library skill wins; neither file is touched.
/// The skill says who shadows it and both sources carry a warning.
fn shadow_pass(sources: &mut [Source], items: &mut [Item]) {
    let commands: Vec<(String, String)> = items
        .iter()
        .filter(|i| i.kind == "command" && i.origin.kind == "org")
        .map(|i| (i.name.clone(), i.source_id.clone()))
        .collect();
    let mut warnings: Vec<(String, String)> = Vec::new();
    for item in items
        .iter_mut()
        .filter(|i| i.kind == "skill" && i.origin.kind == "library")
    {
        let Some((name, org_id)) = commands.iter().find(|(n, _)| *n == item.name) else {
            continue;
        };
        item.shadowed_by = Some(format!("{org_id}:command:{name}"));
        warnings.push((
            item.source_id.clone(),
            format!("skill {name} is shadowed by the org command of the same name"),
        ));
        warnings.push((
            org_id.clone(),
            format!(
                "command {name} shadows the library skill of the same name; both are left alone"
            ),
        ));
    }
    for (id, text) in warnings {
        if let Some(source) = sources.iter_mut().find(|s| s.id == id) {
            if !source.warnings.contains(&text) {
                source.warnings.push(text);
            }
        }
    }
}

/// Runs one detector by id against a prepared context; the unit tests use it.
pub fn scan_one(id: &str, ctx: &ScanCtx) -> DetectorOutput {
    detectors()
        .into_iter()
        .find(|d| d.id() == id)
        .map(|d| d.scan(ctx))
        .unwrap_or_default()
}
