//! `client`: one source per client repository under the clients root (D-040), the same git-tree
//! read as `repo`. A repository without CLAUDE.md or `.claude` content is not a source.

use super::fsx::{self, Kind};
use super::repo::{dir_name, scan_checkout, Spec};
use super::scope::is_checkout;
use super::{DetectorOutput, ScanCtx, SourceDetector};

pub struct ClientDetector;

impl SourceDetector for ClientDetector {
    fn id(&self) -> &'static str {
        "client"
    }

    fn scan(&self, ctx: &ScanCtx) -> DetectorOutput {
        let mut out = DetectorOutput::default();
        for child in fsx::list_dir(&ctx.scope.clients_root) {
            if !ctx.budget.tick() {
                break;
            }
            if child.kind != Kind::Dir || child.symlink || !is_checkout(&child.path) {
                continue;
            }
            let name = dir_name(&child.path);
            let spec = Spec {
                root: child.path,
                id: format!("client:{name}"),
                name,
                detector: "client",
                kind: "client",
                keep_empty: false,
            };
            if let Some((source, items)) = scan_checkout(ctx, &spec) {
                out.sources.push(source);
                out.items.extend(items);
            }
        }
        out
    }
}
