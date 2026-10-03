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
    ("plus.import_mcpm.run", import_mcpm::run_handler),
    ("plus.import_mcpm.nameMap", import_mcpm::name_map_handler),
    ("plus.import_mcpm.renameRefs", import_mcpm::rename_refs_handler),
    ("plus.context.whatLoads", context::what_loads_handler),
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

#[cfg(feature = "desktop")]
#[tauri::command]
pub fn plus_invoke(command: String, args: Value) -> Result<Value, String> {
    dispatch(&command, args)
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

    #[test]
    fn handler_names_are_unique_and_namespaced() {
        let mut seen = std::collections::HashSet::new();
        for (name, _) in HANDLERS {
            assert!(name.starts_with("plus."), "{name}");
            assert!(seen.insert(*name), "duplicate {name}");
        }
    }
}
