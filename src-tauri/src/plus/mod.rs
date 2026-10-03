//! Toolport+ extensions (D-010). Every ported feature registers its handlers
//! here and is reached through the single generic `plus_invoke` IPC command, so
//! upstream merges never touch the desktop command list again.

pub mod ctl;

use serde_json::{json, Value};

pub mod auth;
pub mod obs;

pub type Handler = fn(Value) -> Result<Value, String>;

const HANDLERS: &[(&str, Handler)] = &[
    ("plus.ping", ping),
    ("plus.obs.summary", obs::summary_handler),
    ("plus.auth.status", auth::status_handler),
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
