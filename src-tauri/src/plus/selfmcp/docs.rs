use serde_json::{json, Value};

pub const ARCHITECTURE: &str = "# Toolport+ architecture

## Summary

Toolport+ keeps one registry of MCP servers and profiles. A single gateway daemon
(`toolport-gateway`) serves every enabled server to every client, so a client config only
carries one gateway entry. Per-server proxy modes, the router and bridge workers of the
earlier tooling were replaced by this daemon (D-008).

## Reconcile model

- Registry (`registry.json` in the data directory) is the source of truth for servers,
  profiles, per-client scopes and the entries Toolport wrote into client configs.
- `clients_sync` makes each managed client match the registry: it installs the gateway entry,
  removes direct entries that duplicate registered servers and prunes orphans.
- `client_direct_add` is the opt-in exception: one stdio server gets its own entry in one client
  that runs `toolportctl direct run <server id>` instead of going through the gateway. It
  bypasses profile tool scopes, approvals, receipts and lazy discovery, and each client starts
  its own process. `clients_sync` leaves such an entry alone; `servers_uninstall` removes it.
- Secret values live in the vault (OS keychain or the encrypted file selected by
  `TOOLPORT_SECRET_KEY`). They are never written to the registry or returned by any tool here.

## Skills, agents and styles

- The canonical repository holds `skills/`, `rules/`, `agents/` and `styles/`.
- Transpilers render each entry into the layout of every supported client.
- `skills_sync` and `agents_sync` record what they wrote in `mcpm-skills.lock`; the status and
  drift tools compare content hashes against that lockfile.
- Styles have two tiers: tier 1 clients toggle a style natively and receive every style; tier 2
  clients receive one applied style as an always-on rule (`styles_apply`, `styles_remove`).

## Files

- Data directory: registry, lockfile, auth cache, scripts relocated by the importer.
- Skills repository: the clone named in `skills_sync.json`, or the nearest directory that looks
  like one.
- Client configs: written only by `clients_sync`, `client_direct_add|rm` and the importer, with
  backups.

## Invariants

- Tier 1 tools only read. Tier 2 tools write generated or additive state. Tier 3 and tier 4 tools
  refuse unless `confirm=true`.
- A tool never prints a secret value; sensitive fields are masked before output leaves the
  process.
- Encrypted sync (`sync_push`) publishes an encrypted bundle; the passphrase never travels
  through this server.

## See also

Read `mcpm://paths` for the absolute paths of this installation and `mcpm://workflows` for
step-by-step recipes.
";

pub const WORKFLOWS: &str = "# Toolport+ workflows

Read `mcpm://architecture` first for the model behind these recipes.

## Register a new server

1. `servers_install` with `name` and `config` (command and args, or url). Confirm required.
2. Store any secrets from a shell: `toolportctl secret set <server> <KEY>`.
3. `servers_add_profile_tag` to enable it in a profile.
4. `clients_sync` so every managed client picks up the gateway entry.

## Remove a server cleanly

1. `servers_uninstall` with `propagate_to_clients=true` (tier 4, confirm required).
2. `clients_sync` with `dry_run=true` to confirm nothing still references it.

## Update a source-installed server

1. `servers_check_updates` for one name or all.
2. `servers_git_status` to see whether the checkout is dirty.
3. `servers_apply_update` (tier 3). For a fork, `servers_fork_sync` replays your commits onto
   its upstream on a new branch and pushes nothing.

## Publish skills

1. `skills_lint`, then `skills_sync` with `dry_run=true`, then without it.
2. `skills_git_push` with a commit message (tier 4, confirm required).

## Give one client a server directly

1. `client_direct_ls` to see which clients already have one.
2. `client_direct_add` with `server` and `client` (dry run by default; pass `dry_run=false` to
   write). Only local stdio servers qualify.
3. `client_direct_rm` undoes it.

## Apply an output style

1. `styles_list`, then `styles_sync_tier1` for clients with a native toggle.
2. `styles_apply` for tier 2 clients; `styles_remove` clears it.

## Clean up synced content

1. `skills_diff`, `agents_diff` and `styles_diff` show what changed since the last sync;
   `skills_audit` and `agents_audit` scan for risky content.
2. `skills_clean`, `agents_clean` or `styles_clean` removes the synced client outputs;
   `skills_uninstall` and `agents_uninstall` also delete one skill or agent from the repository.
   Each previews until you pass `dry_run=false` and `confirm=true`.

## Switch compression on

1. `compression_status` for the provider and preset in force.
2. `compression_enable` with a `provider` (dry run by default; `dry_run=false` and
   `confirm=true` apply it). `compression_use` and `compression_set_provider` change one setting
   later, `compression_disable` switches it off.
3. With a proxy running, `compression_seal` records its live settings into the preset.

## Hand off to another machine

1. `sync_push` with `dry_run=true` to list what would be published.
2. `sync_push` with `confirm=true`. On the other machine run `toolportctl sync pull`.

## Diagnose

1. `where_am_i` and `doctor` for paths, registry health and the gateway binary.
2. `mcpm://router/status` explains the gateway state model.
";

pub fn router_status() -> Value {
    json!({
        "router": {
            "present": false,
            "status": "replaced",
            "replacedBy": "toolport-gateway daemon",
            "decision": "D-008",
        },
        "note": "Toolport+ serves every enabled server through one gateway daemon. Per-server proxy modes, router and bridge workers do not exist; use where_am_i for the gateway binary and doctor for health.",
    })
}
