//! The login-keychain secrets mode.
//!
//! Upstream seals `secrets.enc` with a key derived from `TOOLPORT_SECRET_KEY`
//! (headless) or stores each secret in the data-protection keychain, which a
//! locally signed build cannot use. This mode keeps upstream's file backend
//! and takes its 32-byte key from the login keychain instead (service
//! `conduit-mcp`, account `__conduit_master_key__`). The marker file
//! `secrets-backend` in the data dir selects it, and the data-protection
//! migration then does not run.

use std::path::PathBuf;

use toolport_plus_api::SecretsHost;

use crate::keychain_trust;

pub const BACKEND_ENV: &str = "TOOLPORT_SECRETS_BACKEND";
pub const BACKEND_MARKER: &str = "secrets-backend";
/// `off` skips the trusted-application rewrite at app start.
pub const ACL_REFRESH_ENV: &str = "TOOLPORT_KEYCHAIN_ACL_REFRESH";

const LOGIN_KEYCHAIN: &str = "login-keychain";
const KEYCHAIN_SERVICE: &str = "conduit-mcp";
const MASTER_KEY_ACCOUNT: &str = "__conduit_master_key__";

/// Upstream's wording for a file vault with no key, kept so the default
/// behaviour reads the same with or without this mode.
pub const NO_KEY_MESSAGE: &str = "TOOLPORT_SECRET_KEY (legacy CONDUIT_SECRET_KEY) is not set";

/// Either source may ask for the mode; the value is compared trimmed and
/// ignoring case.
pub fn login_keychain_requested(env: Option<&str>, marker: Option<&str>) -> bool {
    [env, marker]
        .into_iter()
        .flatten()
        .any(|value| value.trim().eq_ignore_ascii_case(LOGIN_KEYCHAIN))
}

/// True only where the host has a login keychain and the env var or the marker asks for it.
pub fn enabled(host: &dyn SecretsHost) -> bool {
    if !host.login_keychain_available() {
        return false;
    }
    let env = host.env_var(BACKEND_ENV);
    let marker = host
        .data_dir()
        .and_then(|dir| std::fs::read_to_string(dir.join(BACKEND_MARKER)).ok());
    login_keychain_requested(env.as_deref(), marker.as_deref())
}

/// The key that seals `secrets.enc`.
///
/// `env_key` (upstream's `TOOLPORT_SECRET_KEY`) wins when set. Otherwise, in this
/// mode, the master key comes from the login keychain: a writer (`create`) makes
/// it when it does not exist, a reader never does. Every other outcome is an
/// error, never an empty vault: a key that cannot be read, or one that does not
/// exist while `secrets.enc` does, must not look like "no secrets".
pub fn vault_key(
    host: &dyn SecretsHost,
    env_key: Option<[u8; 32]>,
    create: bool,
) -> Result<[u8; 32], String> {
    if let Some(key) = env_key {
        return Ok(key);
    }
    if !enabled(host) {
        return Err(NO_KEY_MESSAGE.to_string());
    }
    if create {
        return host.ensure_master_key().map_err(|error| {
            format!(
                "login-keychain mode: could not get the vault master key from the login keychain ({error})"
            )
        });
    }
    match host.read_master_key() {
        Ok(Some(key)) => Ok(key),
        Ok(None) => Err(format!(
            "login-keychain mode: secrets.enc exists but the login keychain has no vault master key (service {KEYCHAIN_SERVICE}, account {MASTER_KEY_ACCOUNT}); not creating a new one over it"
        )),
        Err(error) => Err(format!(
            "login-keychain mode: could not read the vault master key from the login keychain ({error}); secrets.enc was left untouched"
        )),
    }
}

/// What app start does in this mode: `None` when the mode is off, so upstream's
/// own start-up migration runs. Otherwise `Some` with the result of re-applying
/// the master key's trusted-application list, which is the only start-up work
/// the mode does. A failure to refresh is not fatal and retries next launch.
pub fn at_start(host: &dyn SecretsHost) -> Option<Result<bool, String>> {
    if !enabled(host) {
        return None;
    }
    if host
        .env_var(ACL_REFRESH_ENV)
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("off"))
    {
        return Some(Ok(false));
    }
    Some(refresh_acl(host))
}

/// Writes the trusted-application list again when a Toolport binary was added,
/// removed or rebuilt since the last write. Returns whether it rewrote.
pub fn refresh_acl(host: &dyn SecretsHost) -> Result<bool, String> {
    let dir = host
        .data_dir()
        .ok_or_else(|| "no Toolport data directory".to_string())?;
    let inputs = host.trust_inputs()?;
    let paths = keychain_trust::trusted_binaries(
        &inputs.exe,
        inputs.gateway.as_deref(),
        inputs.data_bin.as_deref(),
        &inputs.extra_dirs,
    );
    let current = keychain_trust::fingerprint(&paths);
    keychain_trust::refresh_with(
        keychain_trust::stored_fingerprint(&dir).as_deref(),
        &current,
        || host.rewrite_master_key_acl(),
        |value| {
            if keychain_trust::store_fingerprint(&dir, value) {
                Ok(())
            } else {
                Err("could not record the keychain ACL fingerprint".to_string())
            }
        },
    )
}

/// Trusted binaries beyond the two upstream already names (the running binary
/// and the gateway). Empty when the host cannot say where its binaries are.
pub fn extra_trusted_binaries(host: &dyn SecretsHost) -> Vec<PathBuf> {
    let Ok(inputs) = host.trust_inputs() else {
        return Vec::new();
    };
    keychain_trust::beyond_app_and_gateway(
        &inputs.exe,
        inputs.gateway.as_deref(),
        inputs.data_bin.as_deref(),
        &inputs.extra_dirs,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use toolport_plus_api::{MemorySecrets, TrustInputs};

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "toolport-plus-keychain-{name}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir.canonicalize().unwrap())
        }

        fn marker(&self, content: &str) {
            std::fs::write(self.0.join(BACKEND_MARKER), content).unwrap();
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn the_env_var_or_the_marker_requests_the_mode() {
        assert!(!login_keychain_requested(None, None));
        assert!(!login_keychain_requested(Some(""), Some("other")));
        assert!(login_keychain_requested(Some("login-keychain"), None));
        assert!(login_keychain_requested(None, Some(" Login-Keychain\n")));
    }

    #[test]
    fn a_marker_in_the_data_dir_enables_the_mode_where_the_keychain_exists() {
        let dir = Scratch::new("marker");
        let host = MemorySecrets::macos(&dir.0);
        assert!(!enabled(&host), "no marker: upstream's default stays");
        dir.marker("login-keychain\n");
        assert!(enabled(&host));
        dir.marker("something-else");
        assert!(!enabled(&host));
    }

    #[test]
    fn the_env_var_enables_the_mode_without_a_marker() {
        let dir = Scratch::new("env");
        let host = MemorySecrets::macos(&dir.0);
        host.set_env(BACKEND_ENV, "login-keychain");
        assert!(enabled(&host));
    }

    #[test]
    fn the_mode_is_inert_where_there_is_no_login_keychain() {
        let dir = Scratch::new("inert");
        dir.marker("login-keychain");
        let host = MemorySecrets {
            data_dir: Some(dir.0.clone()),
            ..Default::default()
        };
        host.set_env(BACKEND_ENV, "login-keychain");
        assert!(!enabled(&host));
        let error = vault_key(&host, None, false).unwrap_err();
        assert_eq!(error, NO_KEY_MESSAGE);
        assert_eq!(host.reads.get(), 0);
    }

    #[test]
    fn a_marker_with_no_data_dir_does_not_enable_the_mode() {
        let host = MemorySecrets {
            available: true,
            ..Default::default()
        };
        assert!(!enabled(&host));
    }

    #[test]
    fn the_env_key_wins_and_the_keychain_is_not_asked() {
        let dir = Scratch::new("env-key");
        dir.marker("login-keychain");
        let host = MemorySecrets::macos(&dir.0);
        host.key.set(Some([9; 32]));
        assert_eq!(vault_key(&host, Some([1; 32]), false), Ok([1; 32]));
        assert_eq!(vault_key(&host, Some([1; 32]), true), Ok([1; 32]));
        assert_eq!((host.reads.get(), host.ensures.get()), (0, 0));
    }

    #[test]
    fn the_mode_reads_the_login_keychain_key() {
        let dir = Scratch::new("read");
        dir.marker("login-keychain");
        let host = MemorySecrets::macos(&dir.0);
        host.key.set(Some([7; 32]));
        assert_eq!(vault_key(&host, None, false), Ok([7; 32]));
        assert_eq!((host.reads.get(), host.ensures.get()), (1, 0));
    }

    #[test]
    fn only_a_writer_creates_the_master_key() {
        let dir = Scratch::new("create");
        dir.marker("login-keychain");
        let host = MemorySecrets::macos(&dir.0);
        let error = vault_key(&host, None, false).unwrap_err();
        assert!(error.contains("no vault master key"), "{error}");
        assert_eq!(host.ensures.get(), 0);
        assert_eq!(host.key.get(), None);

        let created = vault_key(&host, None, true).unwrap();
        assert_eq!(host.key.get(), Some(created));
        assert_eq!(vault_key(&host, None, false), Ok(created));
    }

    #[test]
    fn a_key_the_keychain_cannot_give_is_an_error_not_an_empty_vault() {
        let dir = Scratch::new("failure");
        dir.marker("login-keychain");
        let host = MemorySecrets::macos(&dir.0);
        *host.read_error.borrow_mut() = Some("user interaction is not allowed".into());
        let error = vault_key(&host, None, false).unwrap_err();
        assert!(
            error.contains("could not read the vault master key")
                && error.contains("user interaction is not allowed")
                && error.contains("left untouched"),
            "{error}"
        );

        *host.ensure_error.borrow_mut() = Some("keychain is locked".into());
        let error = vault_key(&host, None, true).unwrap_err();
        assert!(
            error.contains("could not get the vault master key") && error.contains("locked"),
            "{error}"
        );
        assert_eq!(host.key.get(), None);
    }

    #[test]
    fn the_default_error_is_upstreams_wording() {
        let dir = Scratch::new("default");
        let host = MemorySecrets::macos(&dir.0);
        assert_eq!(
            vault_key(&host, None, true).unwrap_err(),
            "TOOLPORT_SECRET_KEY (legacy CONDUIT_SECRET_KEY) is not set"
        );
        assert_eq!(host.ensures.get(), 0);
    }

    fn host_with_binaries(dir: &Scratch) -> MemorySecrets {
        let macos = dir.0.join("Toolport.app/Contents/MacOS");
        std::fs::create_dir_all(&macos).unwrap();
        for name in [
            "conduit",
            "toolport-gateway",
            "toolportctl",
            "toolport-selfmcp",
        ] {
            std::fs::write(macos.join(name), b"bin").unwrap();
        }
        let data = dir.0.join("data");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(data.join(BACKEND_MARKER), "login-keychain").unwrap();
        let host = MemorySecrets::macos(&data);
        host.key.set(Some([3; 32]));
        *host.trust_inputs.borrow_mut() = TrustInputs {
            exe: macos.join("conduit"),
            gateway: Some(macos.join("toolport-gateway")),
            ..Default::default()
        };
        host
    }

    #[test]
    fn the_acl_is_rewritten_once_per_binary_set() {
        let dir = Scratch::new("acl");
        let host = host_with_binaries(&dir);
        assert_eq!(at_start(&host), Some(Ok(true)));
        assert_eq!(at_start(&host), Some(Ok(false)));
        assert_eq!(host.acl_rewrites.get(), 1);

        let ctl = dir.0.join("Toolport.app/Contents/MacOS/toolportctl");
        std::fs::write(&ctl, b"rebuilt").unwrap();
        assert_eq!(at_start(&host), Some(Ok(true)));
        assert_eq!(host.acl_rewrites.get(), 2);
    }

    #[test]
    fn a_failed_acl_rewrite_is_reported_and_retried_next_start() {
        let dir = Scratch::new("acl-failure");
        let host = host_with_binaries(&dir);
        *host.acl_error.borrow_mut() = Some("denied".into());
        assert_eq!(at_start(&host), Some(Err("denied".to_string())));
        *host.acl_error.borrow_mut() = None;
        assert_eq!(at_start(&host), Some(Ok(true)));
    }

    #[test]
    fn no_master_key_means_nothing_to_rewrite_and_no_fingerprint() {
        let dir = Scratch::new("acl-no-key");
        let host = host_with_binaries(&dir);
        host.key.set(None);
        assert_eq!(at_start(&host), Some(Ok(false)));
        host.key.set(Some([3; 32]));
        assert_eq!(at_start(&host), Some(Ok(true)));
    }

    #[test]
    fn the_acl_refresh_can_be_switched_off() {
        let dir = Scratch::new("acl-off");
        let host = host_with_binaries(&dir);
        host.set_env(ACL_REFRESH_ENV, " OFF ");
        assert_eq!(at_start(&host), Some(Ok(false)));
        assert_eq!(host.acl_rewrites.get(), 0);
    }

    #[test]
    fn start_does_nothing_when_the_mode_is_off() {
        let dir = Scratch::new("start-off");
        let host = MemorySecrets::macos(&dir.0);
        assert_eq!(at_start(&host), None);
        assert_eq!((host.reads.get(), host.acl_rewrites.get()), (0, 0));
    }

    #[test]
    fn the_extra_list_comes_from_the_host_inputs() {
        let dir = Scratch::new("extras");
        let host = host_with_binaries(&dir);
        let names: Vec<String> = extra_trusted_binaries(&host)
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"toolportctl".to_string()));
        assert!(names.contains(&"toolport-selfmcp".to_string()));
    }
}
