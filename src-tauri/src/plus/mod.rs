//! Toolport+ extensions (D-010). Every ported feature registers its handlers
//! here and is reached through the single generic `plus_invoke` IPC command, so
//! upstream merges never touch the desktop command list again.

pub mod ctl;

use serde_json::{json, Value};

pub mod auth;
pub mod cc;
pub mod compression;
pub mod council;
pub mod context;
pub mod import_mcpm;
pub mod obs;
pub mod selfmcp;
pub mod skills;
pub mod sync;
pub mod update;

pub(crate) mod args;
pub(crate) mod exec;
pub(crate) mod fswalk;
pub(crate) mod hashing;
pub(crate) mod jsonfs;
pub(crate) mod registry_ro;

#[cfg(test)]
pub(crate) mod testutil;
#[cfg(test)] pub(crate) mod randutil;

pub type Handler = fn(Value) -> Result<Value, String>;

const HANDLERS: &[(&str, Handler)] = &[
    ("plus.ping", ping),
    ("plus.obs.summary", obs::summary_handler),
    ("plus.obs.importMonitor", obs::import_monitor_handler),
    ("plus.auth.status", auth::status_handler),
    ("plus.auth.probe", auth::probe_handler),
    ("plus.auth.rows", auth::rows_handler),
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
    ("plus.compression.status", ctl::compression::status_handler),
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
