//! Logins: the health `auth probe` stored last for each server. Reads the status file only.

use super::{Ctx, Item, Level};
use crate::plus::auth::surfaces;
use crate::plus::registry_ro;
use crate::registry::Registry;

/// A server no profile enables is not served by the gateway, so its login cannot fail anything
/// there; a server the registry does not know stays reported.
fn unused(reg: Option<&Registry>, server: &str) -> bool {
    reg.is_some_and(|reg| {
        reg.servers.iter().any(|s| s.id == server)
            && !reg.profiles.iter().any(|p| p.enabled_server_ids.iter().any(|id| id == server))
    })
}

pub fn collect(ctx: &Ctx) -> Vec<Item> {
    let Some(dir) = surfaces::auth_dir() else { return Vec::new() };
    let status = surfaces::read_visible_status(&dir);
    let reg = registry_ro::read_opt();
    let mut items = Vec::new();
    for row in surfaces::rows(&status, ctx.now) {
        if unused(reg.as_ref(), &row.server) {
            continue;
        }
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
