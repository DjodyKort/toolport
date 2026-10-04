//! Conduit → Toolport brand dual-compat.
//!
//! After the product rename, user-visible identifiers prefer `toolport` /
//! `TOOLPORT_*`. Legacy `conduit` / `CONDUIT_*` names stay accepted so upgrades
//! never break existing installs, env files, or still-running gateways.
//!
//! Intentionally **not** renamed here (breaking identity surfaces):
//! - app bundle id `com.tsout.conduit`
//! - macOS keychain access group `…com.tsout.conduit.shared`
//! - Cargo package / lib names (internal)

use std::path::{Path, PathBuf};

/// Env key written into client MCP configs for the client identity.
pub const CLIENT_ID: &str = "TOOLPORT_CLIENT_ID";
/// Pre-rename client identity env key still accepted by the gateway.
pub const CLIENT_ID_LEGACY: &str = "CONDUIT_CLIENT_ID";

/// Env key written for the initial profile scope of a client install.
pub const PROFILE: &str = "TOOLPORT_PROFILE";
/// Pre-rename profile env key still accepted by the gateway.
pub const PROFILE_LEGACY: &str = "CONDUIT_PROFILE";

/// Passphrase that activates the encrypted-file secrets backend.
pub const SECRET_KEY: &str = "TOOLPORT_SECRET_KEY";
/// Pre-rename passphrase env key still accepted by the secrets backend.
pub const SECRET_KEY_LEGACY: &str = "CONDUIT_SECRET_KEY";

/// Prefer a non-empty `TOOLPORT_*` value, else fall back to `CONDUIT_*`.
pub fn env_var(new_key: &str, legacy_key: &str) -> Option<String> {
    for key in [new_key, legacy_key] {
        if let Ok(v) = std::env::var(key) {
            if !v.trim().is_empty() {
                return Some(v);
            }
        }
    }
    None
}

/// Prefer a non-empty `TOOLPORT_*` OS string, else fall back to `CONDUIT_*`.
pub fn env_var_os(new_key: &str, legacy_key: &str) -> Option<std::ffi::OsString> {
    for key in [new_key, legacy_key] {
        if let Some(v) = std::env::var_os(key) {
            if !v.is_empty() {
                return Some(v);
            }
        }
    }
    None
}

/// Truthy flag (`1` / `true` / `yes`), preferring the new key.
pub fn env_flag(new_key: &str, legacy_key: &str) -> bool {
    match env_var(new_key, legacy_key) {
        Some(v) => {
            let t = v.trim();
            t == "1" || t.eq_ignore_ascii_case("true") || t.eq_ignore_ascii_case("yes")
        }
        None => false,
    }
}

/// Leaf directory name under the OS config root (`Toolport` release,
/// `Toolport-dev` for debug/`tauri dev` builds). Override the full path with
/// `TOOLPORT_DATA_DIR` (legacy: `CONDUIT_DATA_DIR`). Existing `Conduit` /
/// `Conduit-dev` dirs are migrated in place by [`resolve_data_dir_under`] when
/// safe.
pub fn data_dir_leaf_name() -> &'static str {
    if cfg!(debug_assertions) {
        "Toolport-dev"
    } else {
        "Toolport"
    }
}

/// Pre-rename data-dir leaf still used by existing installs until migrated.
pub fn legacy_data_dir_leaf_name() -> &'static str {
    if cfg!(debug_assertions) {
        "Conduit-dev"
    } else {
        "Conduit"
    }
}

/// Pick the data directory under `config_base` (e.g. `…/AppData/Roaming` or
/// `dirs::config_dir()`).
///
/// Preference order (read path; does **not** rename):
/// 1. New leaf if it already exists
/// 2. Legacy leaf if it still exists (pre-migration install)
/// 3. New leaf for a fresh install
///
/// Call [`migrate_legacy_data_dir_under`] once from desktop launch (when no
/// gateway processes hold files open) to rename legacy → new.
pub fn resolve_data_dir_under(config_base: &Path) -> PathBuf {
    let new_dir = config_base.join(data_dir_leaf_name());
    let legacy_dir = config_base.join(legacy_data_dir_leaf_name());

    if new_dir.exists() {
        return new_dir;
    }
    if legacy_dir.exists() {
        return legacy_dir;
    }
    new_dir
}

/// Best-effort one-shot rename of the legacy data-dir leaf to the new leaf.
///
/// Returns `Some(new_path)` when a rename happened, `None` when there was
/// nothing to do or the rename failed (files locked). Never deletes data.
/// Safe to call repeatedly.
pub fn migrate_legacy_data_dir_under(config_base: &Path) -> Option<PathBuf> {
    let new_dir = config_base.join(data_dir_leaf_name());
    let legacy_dir = config_base.join(legacy_data_dir_leaf_name());
    if new_dir.exists() || !legacy_dir.exists() {
        return None;
    }
    match std::fs::rename(&legacy_dir, &new_dir) {
        Ok(()) => Some(new_dir),
        Err(_) => None,
    }
}

/// Windows roaming config parent: `%USERPROFILE%\AppData\Roaming`.
#[cfg(windows)]
pub fn windows_roaming_base(home: &Path) -> PathBuf {
    home.join("AppData").join("Roaming")
}

/// Fork switch: the Toolport+ fork never talks to the upstream product's
/// hosted services (share links, hosted Teams, release pages). The OAuth CIMD
/// document is fetched by authorization servers, not by this app, and stays.
pub const FORK_EGRESS_DISABLED: bool = true;

pub fn is_upstream_egress_url(url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(url.trim()) else {
        return false;
    };
    let host = parsed.host_str().unwrap_or("").to_ascii_lowercase();
    host == "toolport.app"
        || host.ends_with(".toolport.app")
        || (host == "github.com"
            && parsed
                .path()
                .trim_start_matches('/')
                .to_ascii_lowercase()
                .starts_with("btsouth/"))
}

pub fn refuse_upstream_egress(url: &str) -> Result<(), String> {
    if FORK_EGRESS_DISABLED && is_upstream_egress_url(url) {
        return Err("upstream hosted services are disabled in this fork".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn fork_blocks_upstream_hosted_urls_only() {
        assert!(FORK_EGRESS_DISABLED);
        for blocked in [
            "https://toolport.app/api/share?id=abc",
            "https://teams.toolport.app/",
            "https://TOOLPORT.APP/teams#pricing",
            "https://github.com/btsouth/toolport/releases/latest",
        ] {
            assert!(refuse_upstream_egress(blocked).is_err(), "{blocked}");
        }
        for allowed in [
            "https://github.com/DjodyKort/toolport/releases",
            "https://teams.example.com",
            "https://notoolport.app/",
            "not a url",
        ] {
            assert!(refuse_upstream_egress(allowed).is_ok(), "{allowed}");
        }
    }

    #[test]
    fn share_service_is_unreachable_in_the_fork() {
        assert!(crate::sharing_controller::share_setup("{}").is_err());
        assert!(crate::sharing_controller::fetch_shared_setup("abc123").is_err());
        assert!(crate::oauth::open_web_url("https://toolport.app/teams").is_err());
    }

    use super::*;
    use std::fs;
    use std::sync::Mutex;

    // Serialize tests that touch process env.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn env_var_prefers_new_over_legacy() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let new_k = "TOOLPORT_BRAND_TEST_NEW";
        let old_k = "CONDUIT_BRAND_TEST_LEGACY";
        // SAFETY: serialized by ENV_LOCK; test-only keys.
        unsafe {
            std::env::remove_var(new_k);
            std::env::remove_var(old_k);
            std::env::set_var(old_k, "legacy");
        }
        assert_eq!(env_var(new_k, old_k).as_deref(), Some("legacy"));
        unsafe {
            std::env::set_var(new_k, "new");
        }
        assert_eq!(env_var(new_k, old_k).as_deref(), Some("new"));
        unsafe {
            std::env::remove_var(new_k);
            std::env::remove_var(old_k);
        }
    }

    #[test]
    fn env_flag_accepts_common_truthy() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let new_k = "TOOLPORT_BRAND_FLAG";
        let old_k = "CONDUIT_BRAND_FLAG";
        unsafe {
            std::env::remove_var(new_k);
            std::env::remove_var(old_k);
            std::env::set_var(old_k, "yes");
        }
        assert!(env_flag(new_k, old_k));
        unsafe {
            std::env::set_var(new_k, "0");
            std::env::remove_var(old_k);
        }
        assert!(!env_flag(new_k, old_k));
        unsafe {
            std::env::remove_var(new_k);
        }
    }

    #[test]
    fn data_dir_uses_new_leaf_when_present() {
        let root = std::env::temp_dir().join(format!("toolport-brand-new-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(data_dir_leaf_name())).unwrap();
        fs::create_dir_all(root.join(legacy_data_dir_leaf_name())).unwrap();
        let resolved = resolve_data_dir_under(&root);
        assert_eq!(resolved, root.join(data_dir_leaf_name()));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn data_dir_keeps_legacy_until_migrated() {
        let root =
            std::env::temp_dir().join(format!("toolport-brand-legacy-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let legacy = root.join(legacy_data_dir_leaf_name());
        fs::create_dir_all(&legacy).unwrap();
        fs::write(legacy.join("registry.json"), "{}").unwrap();
        assert_eq!(resolve_data_dir_under(&root), legacy);

        let migrated = migrate_legacy_data_dir_under(&root).expect("rename");
        assert_eq!(migrated, root.join(data_dir_leaf_name()));
        assert!(migrated.join("registry.json").exists());
        assert!(!legacy.exists());
        // Second call is a no-op.
        assert!(migrate_legacy_data_dir_under(&root).is_none());
        assert_eq!(resolve_data_dir_under(&root), migrated);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn data_dir_fresh_install_targets_new_leaf() {
        let root =
            std::env::temp_dir().join(format!("toolport-brand-fresh-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let resolved = resolve_data_dir_under(&root);
        assert_eq!(resolved, root.join(data_dir_leaf_name()));
        assert!(!resolved.exists());
        let _ = fs::remove_dir_all(&root);
    }
}
