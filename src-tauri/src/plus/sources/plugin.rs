//! `plugin`: the skills, commands and agents of installed plugins. The installs and whether each
//! is switched on (`enabledPlugins` in the user settings and, when a folder is given, the project
//! and project-local settings; the local file wins) come from `plus::plugins`, the reader that
//! `cc` and `plugins ls` share. This detector reads files only and starts no process.

use super::fsx;
use super::item::{self, Placement};
use super::layout;
use super::model::{Item, Origin, SourceMeta};
use super::{DetectorOutput, ScanCtx, SourceDetector};
use crate::plus::plugins::installed;
use crate::plus::plugins::settings::Layers;

pub struct PluginDetector;

impl SourceDetector for PluginDetector {
    fn id(&self) -> &'static str {
        "plugin"
    }

    fn scan(&self, ctx: &ScanCtx) -> DetectorOutput {
        let mut out = DetectorOutput::default();
        let installs = installed::file_list(&ctx.roots.claude_home);
        if installs.is_empty() {
            return out;
        }
        let layers = Layers::load(
            &ctx.roots.claude_home,
            ctx.cwd,
            ctx.roots.managed_settings.as_deref(),
            Some(&ctx.roots.home),
        );
        for install in installs {
            if ctx.budget.spent() {
                break;
            }
            let Some(path) = install.install_path.clone() else {
                continue;
            };
            let on = layers.enabled(&install.id);
            let (enabled, layer) = (on.effective, on.layer());
            let id = format!("plugin:{}", install.id);
            let origin = Origin::new("plugin", install.id.clone());
            let place = Placement {
                source_id: &id,
                origin: &origin,
                writable: false,
                audited: false,
                memory_lazy: false,
            };
            let reachable = fsx::is_dir(&path);
            let mut items: Vec<Item> = Vec::new();
            if reachable {
                for found in layout::scan_layout(&path, ctx.budget) {
                    if let Some(parsed) = item::parse_file(ctx.cache, found.kind, &found.path) {
                        items.push(item::make_item(
                            &place,
                            found.kind,
                            &found.fallback,
                            fsx::display(&found.path),
                            &parsed,
                        ));
                    }
                }
            }
            let version = install.version.as_deref().unwrap_or("unknown version");
            let (state, detail) = if !reachable {
                (
                    "unreachable",
                    format!("install folder is missing: {}", path.display()),
                )
            } else if enabled {
                ("ok", format!("{version}; enabled in {layer} settings"))
            } else {
                ("ok", format!("{version}; off ({layer})"))
            };
            let meta = SourceMeta {
                id: id.clone(),
                origin,
                detector: "plugin",
                root: Some(fsx::display(&path)),
                owner: "third-party",
                writable: false,
                managed_by: Some("marketplace".into()),
                state,
                detail,
                freshness: None,
                warnings: Vec::new(),
                enabled: Some(enabled),
            };
            out.sources.push(meta.finish(&items, ctx.now));
            out.items.extend(items);
        }
        out
    }
}
