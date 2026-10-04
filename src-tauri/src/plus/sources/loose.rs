//! `loose`: files in `~/.claude/{skills,commands,agents}` that nobody else accounts for. Org
//! commands, library skills, the synced account folder and files carrying Toolport's own
//! "Managed by" header are not loose; a `.md.retired-...` backup is not an item.

use super::fsx::{self, Kind};
use super::item::{self, Placement};
use super::layout;
use super::library;
use super::model::{Item, Origin, SourceMeta};
use super::{DetectorOutput, ScanCtx, SourceDetector};
use std::collections::BTreeSet;

const MARKERS: [&str; 3] = [
    "Managed by `toolportctl",
    "Managed by Toolport",
    "Managed by `mcpm",
];
const HEAD_BYTES: usize = 512;

fn names_of(dir: &std::path::Path, kinds: &[&str]) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for sub in kinds {
        for entry in fsx::list_dir(&dir.join(sub)) {
            names.insert(format!("{sub}/{}", entry.name.trim_end_matches(".md")));
        }
    }
    names
}

pub struct LooseDetector;

impl SourceDetector for LooseDetector {
    fn id(&self) -> &'static str {
        "loose"
    }

    fn scan(&self, ctx: &ScanCtx) -> DetectorOutput {
        let mut out = DetectorOutput::default();
        let home = &ctx.roots.claude_home;
        if !fsx::is_dir(home) {
            return out;
        }
        let library_names: BTreeSet<String> = library::candidates(ctx)
            .first()
            .map(|root| names_of(root, &["skills", "agents"]))
            .unwrap_or_default();
        let clone = ctx.roots.resolve_corp_tools_dir(ctx.config).join("claude");
        let org_commands: BTreeSet<String> = names_of(&clone, &["commands"]);

        let origin = Origin::new("loose", "~/.claude");
        let place = Placement {
            source_id: "loose",
            origin: &origin,
            writable: true,
            audited: false,
            memory_lazy: false,
        };
        let mut items: Vec<Item> = Vec::new();
        let mut managed = 0;
        for found in layout::scan_layout(home, ctx.budget) {
            let key = format!("{}s/{}", found.kind, found.fallback);
            let claimed = match found.kind {
                "skill" => found.fallback == "synced" || library_names.contains(&key),
                "agent" => library_names.contains(&key),
                "command" => org_commands.contains(&key),
                _ => true,
            };
            if claimed {
                continue;
            }
            let head = fsx::read_text(&found.path, HEAD_BYTES).unwrap_or_default();
            if MARKERS.iter().any(|m| head.contains(m)) {
                managed += 1;
                continue;
            }
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
        let retired = fsx::list_dir(&home.join("commands"))
            .iter()
            .filter(|e| e.kind == Kind::File && e.name.contains(".md.") && !e.name.ends_with(".md"))
            .count();
        if items.is_empty() && managed == 0 && retired == 0 {
            return out;
        }
        let mut notes = vec![format!("{} file(s) no other source owns", items.len())];
        if managed > 0 {
            notes.push(format!("{managed} managed by toolportctl, not counted"));
        }
        if retired > 0 {
            notes.push(format!("{retired} retired backup file(s) ignored"));
        }
        let meta = SourceMeta {
            id: "loose".into(),
            origin: origin.clone(),
            detector: "loose",
            root: Some(fsx::display(home)),
            owner: "me",
            writable: true,
            managed_by: None,
            state: "ok",
            detail: notes.join("; "),
            freshness: None,
            warnings: Vec::new(),
            enabled: None,
        };
        out.sources.push(meta.finish(&items, ctx.now));
        out.items = items;
        out
    }
}
