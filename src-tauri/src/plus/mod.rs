//! Toolport+ extensions (D-010). Every ported feature registers its handlers
//! here and is reached through the single generic `plus_invoke` IPC command, so
//! upstream merges never touch the desktop command list again.

pub mod ctl;

use serde_json::{json, Value};

pub mod attention;
pub mod auth;
pub mod bridge;
pub mod cc;
pub mod compression;
pub mod council;
pub mod context;
pub mod direct;
pub mod gateway_build;
pub mod import_mcpm;
pub mod obs;
pub mod plugins;
pub mod profiles;
pub mod selfmcp;
pub mod skills;
pub mod sources;
pub mod sync;
pub mod tasks;
pub mod update;

pub(crate) mod args;
pub(crate) mod client_sync;
pub(crate) mod exec;
pub(crate) mod fswalk;
pub(crate) mod hashing;
pub(crate) mod health;
pub(crate) mod jsonfs;
pub(crate) mod op;
pub(crate) mod plan;
pub(crate) mod redact;
pub(crate) mod registry_ro;
pub(crate) mod servers;
pub(crate) mod status;
pub(crate) mod tags;

#[cfg(test)]
pub(crate) mod testutil;
#[cfg(test)] pub(crate) mod randutil;

pub type Handler = fn(Value) -> Result<Value, String>;

const HANDLERS: &[(&str, Handler)] = &[
    ("plus.ping", ping),
    ("plus.obs.summary", obs::summary_handler),
    ("plus.obs.importMonitor", obs::import_monitor_handler),
    ("plus.obs.otel.enable", obs::otel_setup::enable_handler),
    ("plus.obs.otel.disable", obs::otel_setup::disable_handler),
    ("plus.obs.otel.status", obs::otel_setup::status_handler),
    ("plus.auth.status", auth::status_handler),
    ("plus.auth.probe", auth::probe_handler),
    ("plus.auth.rows", auth::rows_handler),
    ("plus.auth.login", auth::login_handler),
    ("plus.auth.notifications", auth::notifications_handler),
    ("plus.context.plan", context::plan_handler),
    ("plus.context.apply", context::apply_handler),
    ("plus.skills.sync", skills::handlers::sync_handler),
    ("plus.skills.list", skills::handlers::list_handler),
    ("plus.skills.lint", skills::handlers::lint_handler),
    ("plus.skills.diff", skills::handlers::diff_handler),
    ("plus.skills.status", skills::handlers::status_handler),
    ("plus.skills.clean", skills::state_handlers::clean_handler),
    ("plus.skills.uninstall", skills::state_handlers::uninstall_handler),
    ("plus.skills.resolve", skills::state_handlers::resolve_handler),
    ("plus.skills.init", skills::repo_handlers::init_handler),
    ("plus.skills.add", skills::repo_handlers::add_handler),
    ("plus.skills.audit", skills::repo_handlers::audit_handler),
    ("plus.skills.bundle", skills::repo_handlers::bundle_handler),
    ("plus.skills.unbundle", skills::repo_handlers::unbundle_handler),
    ("plus.skills.tapAdd", skills::tap_handlers::tap_add_handler),
    ("plus.skills.tapList", skills::tap_handlers::tap_list_handler),
    ("plus.skills.tapRemove", skills::tap_handlers::tap_remove_handler),
    ("plus.skills.tapUpdate", skills::tap_handlers::tap_update_handler),
    ("plus.skills.search", skills::tap_handlers::search_handler),
    ("plus.skills.install", skills::tap_handlers::install_handler),
    ("plus.agents.list", skills::agents::handlers::list_handler),
    ("plus.agents.lint", skills::agents::handlers::lint_handler),
    ("plus.agents.audit", skills::agents::handlers::audit_handler),
    ("plus.agents.diff", skills::agents::handlers::diff_handler),
    ("plus.agents.status", skills::agents::handlers::status_handler),
    ("plus.agents.clean", skills::agents::handlers::clean_handler),
    ("plus.agents.uninstall", skills::agents::handlers::uninstall_handler),
    ("plus.agents.add", skills::agents::handlers::add_handler),
    ("plus.agents.sync", skills::agents::handlers::sync_handler),
    ("plus.styles.list", skills::styles::handlers::list_handler),
    ("plus.styles.lint", skills::styles::handlers::lint_handler),
    ("plus.styles.diff", skills::styles::handlers::diff_handler),
    ("plus.styles.status", skills::styles::handlers::status_handler),
    ("plus.styles.clean", skills::styles::handlers::clean_handler),
    ("plus.styles.add", skills::styles::handlers::add_handler),
    ("plus.styles.sync", skills::styles::handlers::sync_handler),
    ("plus.styles.apply", skills::styles::handlers::apply_handler),
    ("plus.styles.remove", skills::styles::handlers::remove_handler),
    ("plus.compression.status", compression::handlers::status_handler),
    ("plus.compression.verify", ctl::compression::verify_handler),
    ("plus.compression.ledger", ctl::compression::ledger_handler),
    ("plus.compression.plan", ctl::compression::plan_handler),
    ("plus.compression.enable", compression::handlers::enable_handler),
    ("plus.compression.disable", compression::handlers::disable_handler),
    ("plus.compression.setProvider", compression::handlers::set_provider_handler),
    ("plus.compression.use", compression::handlers::use_handler),
    ("plus.compression.sync", compression::handlers::sync_handler),
    ("plus.compression.pin", compression::handlers::pin_handler),
    ("plus.compression.seal", compression::handlers::seal_handler),
    ("plus.compression.env", compression::handlers::env_handler),
    ("plus.compression.doctor", compression::handlers::doctor_handler),
    ("plus.import_mcpm.run", import_mcpm::run_handler),
    ("plus.import_mcpm.nameMap", import_mcpm::name_map_handler),
    ("plus.import_mcpm.renameRefs", import_mcpm::rename_refs_handler),
    ("plus.context.init", context::manage::init_handler),
    ("plus.context.status", context::manage::status_handler),
    ("plus.context.clientAdd", context::manage::client_add_handler),
    ("plus.context.clientList", context::manage::client_list_handler),
    ("plus.context.profileAdd", context::manage::profile_add_handler),
    ("plus.context.profileList", context::manage::profile_list_handler),
    ("plus.context.profileRemove", context::manage::profile_remove_handler),
    ("plus.context.disable", context::manage::disable_handler),
    ("plus.context.whatLoads", context::what_loads_handler),
    ("plus.context.folderProfiles", context::folders::folder_profiles_handler),
    ("plus.context.folderProfilesSet", context::folders::folder_profiles_set_handler),
    ("plus.sync.init", sync::handlers::init_handler),
    ("plus.sync.push", sync::handlers::push_handler),
    ("plus.sync.pull", sync::handlers::pull_handler),
    ("plus.sync.diff", sync::handlers::diff_handler),
    ("plus.sync.status", sync::handlers::status_handler),
    ("plus.sync.reset", sync::handlers::reset_handler),
    ("plus.sync.rotatePassphrase", sync::handlers::rotate_handler),
    ("plus.sync.addProject", sync::handlers::add_project_handler),
    ("plus.sync.removeProject", sync::handlers::remove_project_handler),
    ("plus.sync.gitSync", sync::handlers::git_sync_handler),
    ("plus.sync.migrate", sync::handlers::migrate_handler),
    ("plus.profile.list", profiles::handlers::list_handler),
    ("plus.profile.create", profiles::handlers::create_handler),
    ("plus.profile.edit", profiles::handlers::edit_handler),
    ("plus.profile.remove", profiles::handlers::remove_handler),
    ("plus.client.edit", profiles::handlers::client_edit_handler),
    ("plus.client.import", profiles::handlers::client_import_handler),
    ("plus.client.directAdd", direct::handlers::add_handler),
    ("plus.client.directRm", direct::handlers::remove_handler),
    ("plus.client.directLs", direct::handlers::list_handler),
    ("plus.cc.list", cc::list_handler),
    ("plus.cc.update", cc::update_handler),
    ("plus.selfmcp.ensure", selfmcp::register::ensure_handler),
    ("plus.update.check", update::check_handler),
    ("plus.update.apply", update::apply_handler),
];

pub fn dispatch(command: &str, args: Value) -> Result<Value, String> {
    match HANDLERS.iter().find(|(name, _)| *name == command) {
        Some((_, handler)) => handler(args),
        None => Err(format!("unknown plus command: {command}")),
    }
}

fn ping(_args: Value) -> Result<Value, String> {
    Ok(json!({
        "name": "toolport-plus",
        "version": env!("CARGO_PKG_VERSION"),
        "forkEgressDisabled": crate::brand::FORK_EGRESS_DISABLED,
    }))
}

/// Async so Tauri runs it off the main thread; handlers block on files, locks and subprocesses.
#[cfg(feature = "desktop")]
#[tauri::command]
pub async fn plus_invoke(command: String, args: Value) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || dispatch(&command, args))
        .await
        .map_err(|e| format!("plus task join failed: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ping_reports_version_info() {
        let value = dispatch("plus.ping", Value::Null).unwrap();
        assert_eq!(value["name"], "toolport-plus");
        assert_eq!(value["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(value["forkEgressDisabled"], true);
    }

    #[test]
    fn unknown_commands_are_rejected() {
        let error = dispatch("plus.nope", json!({})).unwrap_err();
        assert!(error.contains("plus.nope"));
    }

    #[cfg(feature = "desktop")]
    #[test]
    fn plus_invoke_answers_like_dispatch() {
        let run = |command: &str, args: Value| {
            tauri::async_runtime::block_on(plus_invoke(command.into(), args))
        };
        assert_eq!(
            run("plus.ping", Value::Null),
            dispatch("plus.ping", Value::Null)
        );
        assert_eq!(
            run("plus.nope", json!({})),
            Err("unknown plus command: plus.nope".to_string())
        );
        let tasks: Vec<_> = (0..8)
            .map(|_| tauri::async_runtime::spawn(plus_invoke("plus.ping".into(), Value::Null)))
            .collect();
        for task in tasks {
            let value = tauri::async_runtime::block_on(task).unwrap().unwrap();
            assert_eq!(value["name"], "toolport-plus");
        }
    }

    #[test]
    fn handler_names_are_unique_and_namespaced() {
        let mut seen = std::collections::HashSet::new();
        for (name, _) in HANDLERS {
            assert!(name.starts_with("plus."), "{name}");
            assert!(seen.insert(*name), "duplicate {name}");
        }
    }
}
