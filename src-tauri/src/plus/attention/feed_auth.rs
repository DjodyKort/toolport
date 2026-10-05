//! Logins: the health `auth probe` stored last for each server. Reads the status file only.

use super::{Ctx, Item, Level};
use crate::plus::auth::surfaces;

pub fn collect(ctx: &Ctx) -> Vec<Item> {
    let Some(dir) = surfaces::auth_dir() else { return Vec::new() };
    let status = surfaces::read_status(&dir);
    let mut items = Vec::new();
    for row in surfaces::rows(&status, ctx.now) {
        let (title, detail) = match row.state.as_str() {
            "needs_reauth" => (
                format!("{} needs a new sign-in", row.server),
                "The saved login has expired. Tools of this server fail until you sign in again.",
            ),
            "revoked" => (
                format!("{} no longer lets Toolport in", row.server),
                "Access was withdrawn. Sign in again and agree to the access request.",
            ),
            "misconfigured" => (
                format!("The sign-in setup of {} is wrong", row.server),
                "The server refused the login settings. Check them before signing in again.",
            ),
            "unreachable" => (
                format!("{} could not be reached", row.server),
                "The last check could not connect. It may be the network or the service.",
            ),
            _ => continue,
        };
        items.push(
            Item::new(ctx, format!("auth:{}", row.server), Level::NeedsYou, "auth", title, detail)
                .target("servers", &[("tab", "logins"), ("server", &row.server)])
                .action("Check again", &["toolportctl", "auth", "probe", "--server", &row.server, "--force"])
                .since_epoch(row.since),
        );
    }
    items
}
