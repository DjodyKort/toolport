use super::transpilers::{
    all_style_transpilers, StyleTranspiler, OUTPUT_STYLE_RULE, STYLE_APPEND_MODE,
};
use super::{Style, Tier};
use crate::plus::skills::clock::Clock;
use crate::plus::skills::lock::{get_entry, get_entry_mut, set_entry, LockEntry, LockFile};
use crate::plus::skills::pyfs::{text_hash, write_text};
use std::path::{Path, PathBuf};

pub struct StyleOptions<'a> {
    pub client_keys: Option<Vec<String>>,
    pub dry_run: bool,
    pub clock: &'a dyn Clock,
}

pub(super) fn pick(tier: Tier, keys: &Option<Vec<String>>) -> Vec<Box<dyn StyleTranspiler>> {
    all_style_transpilers()
        .into_iter()
        .filter(|t| t.tier() == tier)
        .filter(|t| {
            keys.as_ref()
                .is_none_or(|k| k.is_empty() || k.iter().any(|c| c == t.client_key()))
        })
        .collect()
}

fn fresh(lockfile: Option<LockFile>, clock: &dyn Clock) -> LockFile {
    lockfile.unwrap_or_else(|| LockFile::new(clock.now().isoformat()))
}

fn set_active(lock: &mut LockFile, client: &str, style: &str) {
    match lock.active_styles.iter_mut().find(|(k, _)| k == client) {
        Some(slot) => slot.1 = style.to_string(),
        None => lock
            .active_styles
            .push((client.to_string(), style.to_string())),
    }
}

/// Tier 1: every style is written for clients with a native toggle (claude-code, roomodes-style).
pub fn sync_styles(
    styles: &[Style],
    root: &Path,
    lockfile: Option<LockFile>,
    opts: &StyleOptions<'_>,
) -> Result<LockFile, String> {
    let mut lock = fresh(lockfile, opts.clock);
    let selected = pick(Tier::Native, &opts.client_keys);
    let (append, per_file): (Vec<_>, Vec<_>) = selected
        .iter()
        .partition(|t| STYLE_APPEND_MODE.contains(&t.client_key()));

    for style in styles {
        let mut entry = LockEntry::new(style.version(), text_hash(&style.source_path)?);
        for t in &per_file {
            let key = t.client_key();
            match t.transpile(style, root) {
                Ok(result) => {
                    entry.clients_synced.push(key.to_string());
                    entry.warnings.extend(result.warnings.iter().cloned());
                    if !opts.dry_run {
                        if let Err(e) = write_text(&result.output_path, &result.content) {
                            entry
                                .warnings
                                .push(format!("{key}: transpilation failed: {e}"));
                        }
                    }
                }
                Err(e) => entry
                    .warnings
                    .push(format!("{key}: transpilation failed: {e}")),
            }
        }
        set_entry(&mut lock.styles, style.name(), entry);
    }

    for t in &append {
        let Some(Ok(result)) = t.transpile_all(styles, root) else {
            continue;
        };
        for style in styles {
            if let Some(entry) = get_entry_mut(&mut lock.styles, style.name()) {
                entry.clients_synced.push(t.client_key().to_string());
                entry.warnings.extend(result.warnings.iter().cloned());
            }
        }
        if !opts.dry_run && !result.content.is_empty() {
            write_text(&result.output_path, &result.content)?;
        }
    }
    Ok(lock)
}

/// Tier 2: injects one style as an always-on rule per client; a second apply replaces the first.
pub fn apply_style(
    style: &Style,
    root: &Path,
    lockfile: Option<LockFile>,
    opts: &StyleOptions<'_>,
) -> Result<LockFile, String> {
    let mut lock = fresh(lockfile, opts.clock);
    let mut entry = match get_entry(&lock.styles, style.name()) {
        Some(e) => e.clone(),
        None => LockEntry::new(style.version(), text_hash(&style.source_path)?),
    };
    for t in pick(Tier::ApplyRemove, &opts.client_keys) {
        let key = t.client_key();
        let outcome = t.transpile(style, root).and_then(|result| {
            if !entry.clients_synced.iter().any(|c| c == key) {
                entry.clients_synced.push(key.to_string());
            }
            entry.warnings.extend(result.warnings.iter().cloned());
            if !opts.dry_run {
                write_text(&result.output_path, &result.content)?;
            }
            Ok(())
        });
        match outcome {
            Ok(()) => set_active(&mut lock, key, style.name()),
            Err(e) => entry.warnings.push(format!("{key}: apply failed: {e}")),
        }
    }
    set_entry(&mut lock.styles, style.name(), entry);
    Ok(lock)
}

/// Deletes the `mcpm-output-style` files of the clients that have an active style and clears
/// their `active_styles` entries; zed keeps its file and only loses the managed block.
pub fn remove_style(
    root: &Path,
    lockfile: Option<LockFile>,
    opts: &StyleOptions<'_>,
) -> Result<LockFile, String> {
    let mut lock = fresh(lockfile, opts.clock);
    for t in pick(Tier::ApplyRemove, &opts.client_keys) {
        let key = t.client_key();
        if !lock.active_styles.iter().any(|(k, _)| k == key) {
            continue;
        }
        if !opts.dry_run {
            let removed = if key == "zed" {
                t.clean(root, &[])
            } else {
                t.clean(root, &[OUTPUT_STYLE_RULE.to_string()])
            };
            if removed.is_err() {
                continue;
            }
        }
        lock.active_styles.retain(|(k, _)| k != key);
    }
    Ok(lock)
}

/// Absolute outputs a style apply would touch, used by status to show drift.
pub fn tier2_output_paths(root: &Path) -> Vec<(String, PathBuf)> {
    let probe = Style::placeholder(OUTPUT_STYLE_RULE);
    all_style_transpilers()
        .into_iter()
        .filter(|t| t.tier() == Tier::ApplyRemove)
        .map(|t| (t.client_key().to_string(), t.get_output_path(&probe, root)))
        .collect()
}
