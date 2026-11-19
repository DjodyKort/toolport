//! `tap`: skills of third-party taps, cloned under `<data dir>/taps/<name>`. One source per tap.

use super::item::{self, Placement};
use super::layout;
use super::layout::Trust;
use super::library::scan_skills_repo_as;
use super::model::{Item, Origin, SourceMeta};
use super::{DetectorOutput, ScanCtx, SourceDetector};
use crate::context::fsx;
use crate::plugins::marketplace;
use crate::skills::taps::{load_taps, tap_dir, taps_root};

pub struct TapDetector;

impl SourceDetector for TapDetector {
    fn id(&self) -> &'static str {
        "tap"
    }

    fn scan(&self, ctx: &ScanCtx) -> DetectorOutput {
        let mut out = DetectorOutput::default();
        let Some(data) = ctx.data_dir else { return out };
        let root = taps_root(data);
        for tap in load_taps(data) {
            if ctx.budget.spent() {
                break;
            }
            let id = format!("tap:{}", tap.name);
            let origin = Origin::new("tap", tap.name.clone());
            let place = Placement {
                source_id: &id,
                origin: &origin,
                writable: false,
                audited: false,
                memory_lazy: false,
            };
            let dir = tap_dir(&root, &tap);
            let cloned = fsx::is_dir(&dir);
            let mut items: Vec<Item> = Vec::new();
            let mut links: Vec<String> = Vec::new();
            if cloned {
                let plugin_files: Vec<_> = match marketplace::read(&dir) {
                    Some(Ok(market)) => market
                        .plugins
                        .iter()
                        .flat_map(|p| layout::scan_layout_untrusted(&p.dir, ctx.budget, &mut links))
                        .filter(|f| f.kind != "rule")
                        .collect(),
                    _ => Vec::new(),
                };
                let files = scan_skills_repo_as(&dir, ctx, Trust::Untrusted, &mut links)
                    .into_iter()
                    .filter(|f| f.kind != "agent")
                    .chain(plugin_files);
                for file in files {
                    if let Some(parsed) = item::parse_file(ctx.cache, file.kind, &file.path) {
                        let kind = if file.kind == "rule" {
                            "rule"
                        } else {
                            file.kind
                        };
                        items.push(item::make_item(
                            &place,
                            kind,
                            &file.fallback,
                            fsx::display(&file.path),
                            &parsed,
                        ));
                    }
                }
            }
            let (state, detail) = if cloned {
                ("ok", format!("{} item(s) from {}", items.len(), tap.repo))
            } else {
                (
                    "unreachable",
                    format!(
                        "the clone of {} is missing; run `toolportctl skills tap update`",
                        tap.repo
                    ),
                )
            };
            let meta = SourceMeta {
                id: id.clone(),
                origin,
                detector: "tap",
                root: Some(fsx::display(&dir)),
                owner: "third-party",
                writable: false,
                managed_by: Some("toolportctl skills tap".into()),
                state,
                detail,
                freshness: None,
                warnings: links
                    .into_iter()
                    .map(|rel| format!("{rel} is a symbolic link, so it is not read"))
                    .collect(),
                enabled: None,
            };
            out.sources.push(meta.finish(&items, ctx.now));
            out.items.extend(items);
        }
        out
    }
}
