//! `plugin`: the skills, commands and agents of installed plugins. `installed_plugins.json` names
//! each install; whether it is switched on comes from `enabledPlugins` in the user settings and,
//! when a folder is given, the project and project-local settings (the local file wins).

use super::fsx;
use super::item::{self, Placement};
use super::layout;
use super::model::{Item, Origin, SourceMeta};
use super::{DetectorOutput, ScanCtx, SourceDetector};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_str(&fsx::read_text(path, 4 * 1024 * 1024)?).ok()
}

struct Install {
    id: String,
    path: PathBuf,
    version: Option<String>,
}

fn installs(doc: &Value) -> Vec<Install> {
    let Some(plugins) = doc.get("plugins").and_then(Value::as_object) else {
        return Vec::new();
    };
    plugins
        .iter()
        .filter_map(|(id, entry)| {
            let record = match entry {
                Value::Array(list) => list
                    .iter()
                    .find(|r| r["scope"] == "user")
                    .or_else(|| list.first())?,
                other => other,
            };
            Some(Install {
                id: id.clone(),
                path: PathBuf::from(record.get("installPath")?.as_str()?),
                version: record
                    .get("version")
                    .and_then(Value::as_str)
                    .map(String::from),
            })
        })
        .collect()
}

fn enabled_map(path: &Path) -> BTreeMap<String, bool> {
    read_json(path)
        .and_then(|doc| {
            doc.get("enabledPlugins")
                .and_then(Value::as_object)
                .cloned()
        })
        .map(|map| {
            map.into_iter()
                .filter_map(|(k, v)| Some((k, v.as_bool()?)))
                .collect()
        })
        .unwrap_or_default()
}

pub struct PluginDetector;

impl SourceDetector for PluginDetector {
    fn id(&self) -> &'static str {
        "plugin"
    }

    fn scan(&self, ctx: &ScanCtx) -> DetectorOutput {
        let mut out = DetectorOutput::default();
        let Some(doc) = read_json(&ctx.roots.claude_home.join("plugins/installed_plugins.json"))
        else {
            return out;
        };
        let user = enabled_map(&ctx.roots.claude_home.join("settings.json"));
        let (project, local) = match ctx.cwd {
            Some(cwd) => (
                enabled_map(&cwd.join(".claude/settings.json")),
                enabled_map(&cwd.join(".claude/settings.local.json")),
            ),
            None => Default::default(),
        };
        for install in installs(&doc) {
            if ctx.budget.spent() {
                break;
            }
            let (enabled, layer) = [
                (&local, "project-local"),
                (&project, "project"),
                (&user, "user"),
            ]
            .into_iter()
            .find_map(|(map, name)| map.get(&install.id).map(|on| (*on, name)))
            .map_or((false, "no settings file"), |(on, name)| (on, name));
            let id = format!("plugin:{}", install.id);
            let origin = Origin::new("plugin", install.id.clone());
            let place = Placement {
                source_id: &id,
                origin: &origin,
                writable: false,
                audited: false,
                memory_lazy: false,
            };
            let reachable = fsx::is_dir(&install.path);
            let mut items: Vec<Item> = Vec::new();
            if reachable {
                for found in layout::scan_layout(&install.path, ctx.budget) {
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
                    format!("install folder is missing: {}", install.path.display()),
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
                root: Some(fsx::display(&install.path)),
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
