//! Fork hook points. Each function forwards to `toolport-plus` and does nothing
//! without the `plus` feature.

use std::path::PathBuf;

// The child-process bridge behind the `plus_ctl` IPC commands (D-060). Tests build it without
// the desktop feature so the headless seam tests cover it.
#[cfg(all(feature = "plus", any(feature = "desktop", test)))]
mod bridge;
#[cfg(all(feature = "plus", feature = "desktop"))]
mod plus_ctl;

#[cfg(feature = "plus")]
mod handshake;
#[cfg(all(test, feature = "plus"))]
mod interop_tests;

/// Wraps the app's invoke handler so the fork's `plus_ctl*` commands are answered here and every
/// other command still reaches `next`.
#[cfg(feature = "desktop")]
pub(crate) fn ipc(
    next: impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static,
) -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
    #[cfg(feature = "plus")]
    return plus_ctl::handler(next);
    #[cfg(not(feature = "plus"))]
    next
}

/// Refuse a request before the connection phase when the fork blocks its host.
pub(crate) fn egress_check(uri: &ureq::http::Uri) -> Result<(), ureq::Error> {
    #[cfg(feature = "plus")]
    std::hint::black_box(toolport_plus_api::SEAM_MARKER);
    #[cfg(feature = "plus")]
    if let Some(host) = uri.host() {
        toolport_plus::egress::check(host).map_err(|message| ureq::Error::Other(message.into()))?;
    }
    #[cfg(not(feature = "plus"))]
    let _ = uri;
    Ok(())
}

/// Whether the resource a protected-resource document names is the configured server URL.
pub(crate) fn same_resource(expected: &str, advertised: &str) -> bool {
    #[cfg(feature = "plus")]
    return toolport_plus::oauth::same_resource(expected, advertised);
    #[cfg(not(feature = "plus"))]
    return expected == advertised;
}

/// Hands the consent URL of an interactive sign-in to the caller that asked to receive it, and
/// opens the browser only when nobody did.
pub(crate) fn present_consent(url: &str) {
    #[cfg(feature = "plus")]
    if toolport_plus::oauth::consent::present(url) {
        return;
    }
    crate::oauth::open_browser(url);
}

/// A launch the gateway keeps for one project folder: the server id, the folder and the slot.
pub type PooledLaunch = (String, String, crate::router::SharedServerSlot);

/// One watcher tick of a gateway process, which owns the router: answers the server restart and
/// in-flight requests other processes left in the data dir. Cheap when there are none.
/// `pooled` lists the launches the process keeps once per project folder, which the router does not;
/// it is called only when the data dir holds a request to answer.
pub fn gateway_tick(
    router: &std::sync::Mutex<std::sync::Arc<crate::router::Router>>,
    dirty: &std::sync::Arc<std::sync::atomic::AtomicU8>,
    pooled: &dyn Fn() -> Vec<PooledLaunch>,
) {
    #[cfg(feature = "plus")]
    drop(gateway_tick_with(
        crate::plus_adapter::HostAdapter,
        router,
        dirty,
        pooled,
    ));
    #[cfg(not(feature = "plus"))]
    let _ = (router, dirty, pooled);
}

/// [`gateway_tick`] for a given task host. Returns the scheduler thread of this tick, if it
/// started one.
#[cfg(feature = "plus")]
pub(crate) fn gateway_tick_with<H: toolport_plus_api::TaskHost + Send + 'static>(
    host: H,
    router: &std::sync::Mutex<std::sync::Arc<crate::router::Router>>,
    dirty: &std::sync::Arc<std::sync::atomic::AtomicU8>,
    pooled: &dyn Fn() -> Vec<PooledLaunch>,
) -> Option<std::thread::JoinHandle<()>> {
    let scheduled = toolport_plus::tasks::schedule::tick_in_thread(host);
    if let Some(dir) =
        crate::registry::conduit_dir().filter(|dir| toolport_plus::servers::restart::awaiting(dir))
    {
        let live = std::sync::Arc::clone(
            &router
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        let gateway = crate::plus_adapter::RouterHandle {
            router: live,
            pooled: pooled(),
            dirty: std::sync::Arc::clone(dirty),
        };
        // The restarts run on their own threads; this tick never waits for one.
        drop(toolport_plus::servers::restart::serve(
            &dir,
            std::sync::Arc::new(gateway),
        ));
    }
    scheduled
}

/// The desktop app is up: starts the fork's scheduler thread, whose first pass catches up the
/// task runs that came due while nothing ticked.
#[cfg(feature = "desktop")]
pub(crate) fn app_started() {
    #[cfg(feature = "plus")]
    toolport_plus::tasks::schedule::spawn_app_ticker(crate::plus_adapter::HostAdapter);
}

/// The app window exists: starts the thread that notifies about the rows that need the person,
/// which needs the handle to show a notification and to ask whether the window has the focus.
#[cfg(feature = "desktop")]
pub(crate) fn app_ready(handle: &tauri::AppHandle) {
    #[cfg(feature = "plus")]
    toolport_plus::attention::notify::spawn(
        crate::plus_adapter::HostAdapter,
        crate::plus_adapter::AppNotifier::new(handle.clone()),
    );
    #[cfg(not(feature = "plus"))]
    let _ = handle;
}

/// Takes the tools of servers whose mode is `search` out of the surface a Full tools/list sends.
/// Leaves `surface` as it is, and parses nothing, when the list is not a Full one or no server
/// has that mode. `owner_of` names the server behind an exposed tool and `helpers` builds the
/// search and call tools.
pub fn search_only_surface(
    full_list: bool,
    reg: &crate::registry::Registry,
    owner_of: impl Fn(&str) -> Option<String>,
    surface: &mut std::sync::Arc<crate::savings::SerializedSurface>,
    helpers: impl FnOnce() -> [serde_json::Value; 2],
) {
    #[cfg(feature = "plus")]
    {
        use toolport_plus::servers::mode;
        let hidden: std::collections::HashSet<&str> = reg
            .servers
            .iter()
            .filter(|server| mode::is_search_only(&server.unknown_fields))
            .map(|server| server.id.as_str())
            .collect();
        if !full_list || hidden.is_empty() {
            return;
        }
        let Ok(tools) = serde_json::from_str(surface.json.get()) else {
            return;
        };
        let is_hidden =
            |name: &str| owner_of(name).is_some_and(|owner| hidden.contains(owner.as_str()));
        let listed = mode::listing(tools, is_hidden, helpers());
        *surface = std::sync::Arc::new(crate::savings::SerializedSurface::from_tools(listed));
    }
    #[cfg(not(feature = "plus"))]
    let _ = (full_list, reg, owner_of, surface, helpers);
}

/// Whether `uv` with these arguments syncs an environment before it can answer, so its first
/// `initialize` gets the launcher budget. `args` are the arguments after the command.
pub fn is_uv_download_launcher(args: &[String]) -> bool {
    #[cfg(feature = "plus")]
    return toolport_plus::launcher::is_uv_download_launcher(args);
    #[cfg(not(feature = "plus"))]
    {
        let _ = args;
        false
    }
}

/// The deadline of one stdio call. A server that reports progress for the call's own
/// `progressToken` keeps the call alive up to its `maxRequestTimeoutMs`; without the fork it is a
/// fixed deadline.
pub struct CallClock {
    #[cfg(feature = "plus")]
    clock: toolport_plus::progress::Clock,
    #[cfg(feature = "plus")]
    server_id: Option<String>,
    #[cfg(not(feature = "plus"))]
    deadline: std::time::Instant,
}

impl CallClock {
    pub fn start(
        window: std::time::Duration,
        params: &serde_json::Value,
        server_id: Option<&String>,
    ) -> Self {
        #[cfg(feature = "plus")]
        return Self {
            clock: toolport_plus::progress::Clock::start(
                window,
                toolport_plus::progress::request_token(params),
            ),
            server_id: server_id.cloned(),
        };
        #[cfg(not(feature = "plus"))]
        {
            let _ = (params, server_id);
            Self {
                deadline: std::time::Instant::now() + window,
            }
        }
    }

    /// How long to wait for the next message before looking at the deadline again.
    pub fn next_wait(&mut self) -> std::time::Duration {
        #[cfg(feature = "plus")]
        {
            let server_id = self.server_id.clone();
            self.clock.next_wait(move || {
                toolport_plus::progress::cap_from(max_request_timeout_ms(server_id))
            })
        }
        #[cfg(not(feature = "plus"))]
        self.deadline
            .saturating_duration_since(std::time::Instant::now())
    }

    /// Gives back time the call spent waiting for its client to answer the server.
    pub fn charge_back(&mut self, waited: std::time::Duration) {
        #[cfg(feature = "plus")]
        self.clock.charge_back(waited);
        #[cfg(not(feature = "plus"))]
        {
            self.deadline += waited;
        }
    }

    pub fn expired(&self) -> bool {
        #[cfg(feature = "plus")]
        return self.clock.expired();
        #[cfg(not(feature = "plus"))]
        (std::time::Instant::now() >= self.deadline)
    }

    /// Appended to the timeout message when the call ended at its cap.
    pub fn cap_note(&self) -> &'static str {
        #[cfg(feature = "plus")]
        return self.clock.cap_note().unwrap_or("");
        #[cfg(not(feature = "plus"))]
        ""
    }
}

#[cfg(feature = "plus")]
fn max_request_timeout_ms(server_id: Option<String>) -> Option<u64> {
    let id = server_id?;
    crate::registry::load()
        .ok()?
        .servers
        .into_iter()
        .find(|server| server.id == id)?
        .unknown_fields
        .get("maxRequestTimeoutMs")?
        .as_u64()
}

/// A server sent a notification on a stdio connection.
pub fn progress_seen(notification: &serde_json::Value) {
    #[cfg(feature = "plus")]
    toolport_plus::progress::note(notification);
    #[cfg(not(feature = "plus"))]
    let _ = notification;
}

/// What the legacy `initialize` to a server declares as client capabilities: `{}`, unless the
/// server opted in and a client has declared some.
pub fn downstream_capabilities(server_id: &str) -> serde_json::Value {
    #[cfg(feature = "plus")]
    return handshake::downstream_capabilities(server_id);
    #[cfg(not(feature = "plus"))]
    {
        let _ = server_id;
        serde_json::json!({})
    }
}

/// A server answered `initialize` or `server/discover` with `result`.
pub fn handshake_seen(server_id: &str, result: &serde_json::Value) {
    #[cfg(feature = "plus")]
    toolport_plus::handshake::store_instructions(server_id, result);
    #[cfg(not(feature = "plus"))]
    let _ = (server_id, result);
}

/// Adds the instructions of the servers that opted in, within the caller's scope, to the
/// gateway's own text.
pub fn forward_instructions(
    own: Option<String>,
    reg: &crate::registry::Registry,
    allowed: Option<&std::collections::HashSet<String>>,
) -> Option<String> {
    #[cfg(feature = "plus")]
    return handshake::forward_instructions(own, reg, allowed);
    #[cfg(not(feature = "plus"))]
    {
        let _ = (reg, allowed);
        own
    }
}

/// A client's request reached the gateway: notes the capabilities it declares, restarts the
/// servers that opted in to be told, and holds an `initialize` or `server/discover` for the
/// router to connect the servers whose instructions it forwards. The waits are bounded.
pub fn client_handshake(
    req: &serde_json::Value,
    allowed: Option<&std::collections::HashSet<String>>,
    registry: &std::sync::Mutex<crate::registry::Registry>,
    router: &std::sync::Mutex<std::sync::Arc<crate::router::Router>>,
    ready: &std::sync::atomic::AtomicBool,
) {
    #[cfg(feature = "plus")]
    handshake::client_handshake(req, allowed, registry, router, ready);
    #[cfg(not(feature = "plus"))]
    let _ = (req, allowed, registry, router, ready);
}

/// A tool call to `server_id` returned `result`: remembers the `resource_link` URIs in it so a
/// client's read of one reaches the server that returned it.
pub fn links_seen(server_id: &str, result: &serde_json::Value) {
    #[cfg(feature = "plus")]
    toolport_plus::resource_links::remember_result(server_id, result);
    #[cfg(not(feature = "plus"))]
    let _ = (server_id, result);
}

/// The connected server, among `servers`, that returned a link to `uri`.
pub fn linked_owner<'a>(servers: &'a [String], uri: &str) -> Option<&'a str> {
    #[cfg(feature = "plus")]
    {
        let owner = toolport_plus::resource_links::owner_of(uri)?;
        servers.iter().find(|id| **id == owner).map(String::as_str)
    }
    #[cfg(not(feature = "plus"))]
    {
        let _ = (servers, uri);
        None
    }
}

/// How long the stdio adapter keeps one request open: `base`, or for a request that reaches a
/// server the longest deadline a configured server can hold (see `toolport_plus::progress`).
pub fn adapter_ceiling(base: std::time::Duration, body: &str) -> std::time::Duration {
    #[cfg(feature = "plus")]
    {
        use toolport_plus::progress::{adapter_ceiling, ServerTimeouts};
        let call = ["tools/call", "resources/read", "prompts/get"]
            .into_iter()
            .find(|method| body.contains(method));
        let Some(method) = call else {
            return base;
        };
        let Ok(reg) = crate::registry::load() else {
            return base;
        };
        let servers = reg
            .servers
            .iter()
            .filter(|s| s.enabled)
            .map(|s| ServerTimeouts {
                request_timeout_ms: s.request_timeout_ms,
                max_request_timeout_ms: s
                    .unknown_fields
                    .get("maxRequestTimeoutMs")
                    .and_then(serde_json::Value::as_u64),
                stdio: s.command.is_some(),
            });
        adapter_ceiling(base, method, servers)
    }
    #[cfg(not(feature = "plus"))]
    {
        let _ = body;
        base
    }
}

/// The secrets host the hooks below use. Unit tests never reach the real keychain
/// code: they get an inert host unless a test installs its own with `secrets_host_override`.
#[cfg(all(feature = "plus", not(test)))]
fn with_secrets_host<R>(use_host: impl FnOnce(&dyn toolport_plus_api::SecretsHost) -> R) -> R {
    use_host(&crate::plus_adapter::HostAdapter)
}

#[cfg(all(feature = "plus", test))]
thread_local! {
    static SECRETS_HOST: std::cell::RefCell<Option<std::rc::Rc<dyn toolport_plus_api::SecretsHost>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(all(feature = "plus", test))]
fn with_secrets_host<R>(use_host: impl FnOnce(&dyn toolport_plus_api::SecretsHost) -> R) -> R {
    let installed = SECRETS_HOST.with(|host| host.borrow().clone());
    match installed {
        Some(host) => use_host(&*host),
        None => use_host(&toolport_plus_api::MemorySecrets::default()),
    }
}

#[cfg(all(feature = "plus", test))]
pub(crate) struct SecretsHostGuard;

#[cfg(all(feature = "plus", test))]
impl Drop for SecretsHostGuard {
    fn drop(&mut self) {
        SECRETS_HOST.with(|host| *host.borrow_mut() = None);
    }
}

#[cfg(all(feature = "plus", test))]
pub(crate) fn secrets_host_override(
    host: std::rc::Rc<dyn toolport_plus_api::SecretsHost>,
) -> SecretsHostGuard {
    SECRETS_HOST.with(|slot| *slot.borrow_mut() = Some(host));
    SecretsHostGuard
}

/// Whether `secrets.enc` is sealed with the login-keychain master key
/// (the `secrets-backend` marker), so upstream's file backend is active.
pub(crate) fn login_keychain_enabled() -> bool {
    #[cfg(feature = "plus")]
    return with_secrets_host(toolport_plus::login_keychain::enabled);
    #[cfg(not(feature = "plus"))]
    false
}

/// The key that seals `secrets.enc`: `env_key` when set, else the login-keychain
/// master key in that mode. Fails rather than yielding an empty vault.
pub(crate) fn vault_key(env_key: Option<[u8; 32]>, create: bool) -> Result<[u8; 32], String> {
    #[cfg(feature = "plus")]
    return with_secrets_host(|host| {
        toolport_plus::login_keychain::vault_key(host, env_key, create)
    });
    #[cfg(not(feature = "plus"))]
    {
        let _ = create;
        env_key.ok_or_else(|| "TOOLPORT_SECRET_KEY (legacy CONDUIT_SECRET_KEY) is not set".into())
    }
}

/// Before `secrets.enc` is replaced: keeps its current ciphertext as `secrets.enc.bak`.
/// An error means the replace must not happen.
pub(crate) fn keep_vault_backup(vault: &std::path::Path) -> Result<(), String> {
    #[cfg(feature = "plus")]
    return toolport_plus::vault_backup::keep_previous(vault, crate::registry::atomic_write);
    #[cfg(not(feature = "plus"))]
    {
        let _ = vault;
        Ok(())
    }
}

/// App start: true when the login-keychain mode owns the secrets, after
/// re-applying the master key's trusted binaries. The caller then skips
/// upstream's data-protection keychain migration.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn login_keychain_start() -> bool {
    #[cfg(feature = "plus")]
    return match with_secrets_host(toolport_plus::login_keychain::at_start) {
        None => false,
        Some(refreshed) => {
            if let Err(error) = refreshed {
                eprintln!(
                    "toolport: keychain ACL refresh failed, will retry next launch ({error})"
                );
            }
            true
        }
    };
    #[cfg(not(feature = "plus"))]
    false
}

/// Toolport binaries the master key should trust beyond the running binary and
/// the gateway, which upstream already names.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn extra_trusted_binaries() -> Vec<PathBuf> {
    #[cfg(feature = "plus")]
    return with_secrets_host(toolport_plus::login_keychain::extra_trusted_binaries);
    #[cfg(not(feature = "plus"))]
    Vec::new()
}

#[cfg(all(test, feature = "plus"))]
mod tests {
    use super::egress_check;

    #[test]
    fn egress_check_refuses_upstream_service_hosts() {
        let refused = "https://updates.toolport.app/latest.json".parse().unwrap();
        assert!(egress_check(&refused).is_err());
        let allowed = "https://github.com/".parse().unwrap();
        assert!(egress_check(&allowed).is_ok());
    }

    #[test]
    fn http_agent_refuses_toolport_app_before_connecting() {
        let error = crate::http_client::agent()
            .get("https://toolport.app/")
            .call()
            .expect_err("the resolver must refuse the request");
        assert!(
            error.to_string().contains("egress to toolport.app"),
            "{error}"
        );
    }

    mod login_keychain {
        use std::rc::Rc;

        use toolport_plus_api::MemorySecrets;

        use crate::seam::secrets_host_override;
        use crate::{secret_redact, secrets};

        const SERVER: &str = "synthetic-server";
        const KEY: &str = "SYNTHETIC_TOKEN";
        const VALUE: &str = "synthetic-login-keychain-value-0123456789";

        fn vault_path() -> std::path::PathBuf {
            crate::registry::conduit_dir().unwrap().join("secrets.enc")
        }

        /// A data dir with the marker, no key in the environment and an in-memory login
        /// keychain, so nothing here can reach a real keychain.
        fn in_fork_mode(test: impl FnOnce(&Rc<MemorySecrets>)) {
            secrets::tests::with_isolated_vault(|| {
                let saved: Vec<_> = ["TOOLPORT_SECRET_KEY", "CONDUIT_SECRET_KEY"]
                    .into_iter()
                    .map(|name| (name, std::env::var_os(name)))
                    .collect();
                for (name, _) in &saved {
                    std::env::remove_var(name);
                }
                let dir = crate::registry::conduit_dir().unwrap();
                std::fs::write(dir.join("secrets-backend"), "login-keychain\n").unwrap();
                let host = Rc::new(MemorySecrets::macos(dir));
                let guard = secrets_host_override(host.clone());
                test(&host);
                drop(guard);
                for (name, value) in saved {
                    if let Some(value) = value {
                        std::env::set_var(name, value);
                    }
                }
            });
        }

        #[test]
        fn test01_marker_selects_the_vault_and_the_keychain_supplies_its_key() {
            in_fork_mode(|host| {
                assert!(super::super::login_keychain_enabled());
                secrets::set_secret(SERVER, KEY, VALUE).unwrap();
                assert_eq!(host.ensures.get(), 1, "the first write creates the key");
                let sealed = std::fs::read_to_string(vault_path()).unwrap();
                assert!(!sealed.contains(VALUE));
                assert_eq!(
                    secrets::get_secret_result(SERVER, KEY).unwrap().as_deref(),
                    Some(VALUE)
                );
                assert!(
                    host.reads.get() >= 1,
                    "reads take the key from the keychain"
                );
            });
        }

        #[test]
        fn test02_without_the_marker_upstream_default_stays() {
            secrets::tests::with_isolated_vault(|| {
                let dir = crate::registry::conduit_dir().unwrap();
                let _guard = secrets_host_override(Rc::new(MemorySecrets::macos(dir)));
                assert!(!super::super::login_keychain_enabled());
                assert!(!super::super::login_keychain_start());
            });
        }

        #[cfg(target_os = "macos")]
        #[test]
        fn test03_the_data_protection_migration_is_skipped_in_this_mode() {
            in_fork_mode(|_| {
                let dir = crate::registry::conduit_dir().unwrap();
                let report = secrets::migrate_secrets_to_dpk(&[(SERVER.into(), KEY.into())]);
                assert_eq!(report, secrets::MigrationReport::default());
                assert!(
                    !dir.join(".keychain-dpk-migrated").exists(),
                    "the one-time marker must not be written by a pass that did not run"
                );
            });
        }

        #[test]
        fn test04_a_key_the_keychain_cannot_read_is_an_error_and_the_vault_stays() {
            in_fork_mode(|host| {
                secrets::set_secret(SERVER, KEY, VALUE).unwrap();
                let before = std::fs::read(vault_path()).unwrap();

                *host.read_error.borrow_mut() = Some("user interaction is not allowed".into());
                let error = secrets::get_secret_result(SERVER, KEY).unwrap_err();
                assert!(
                    error.contains("could not read the vault master key")
                        && error.contains("user interaction is not allowed"),
                    "{error}"
                );
                assert!(secrets::set_secret(SERVER, "OTHER", VALUE).is_err());
                assert_eq!(std::fs::read(vault_path()).unwrap(), before);
            });
        }

        #[test]
        fn test05_a_missing_master_key_never_becomes_a_new_empty_vault() {
            in_fork_mode(|host| {
                secrets::set_secret(SERVER, KEY, VALUE).unwrap();
                let before = std::fs::read(vault_path()).unwrap();
                let ensures = host.ensures.get();

                host.key.set(None);
                let error = secrets::get_secret_result(SERVER, KEY).unwrap_err();
                assert!(error.contains("no vault master key"), "{error}");
                assert!(secrets::set_secret(SERVER, "OTHER", VALUE).is_err());
                assert_eq!(host.ensures.get(), ensures, "no replacement key was made");
                assert_eq!(host.key.get(), None);
                assert_eq!(std::fs::read(vault_path()).unwrap(), before);
            });
        }

        #[test]
        fn test06_a_value_written_in_this_mode_is_registered_for_redaction() {
            in_fork_mode(|_| {
                secrets::set_secret(SERVER, KEY, VALUE).unwrap();
                let line = format!("launching with token {VALUE}");
                let redacted = secret_redact::redact_text(&line);
                assert!(!redacted.contains(VALUE), "{redacted}");
                assert!(redacted.contains("[secret:"), "{redacted}");
            });
        }

        #[test]
        fn test07_a_value_read_in_this_mode_is_registered_for_redaction() {
            in_fork_mode(|_| {
                let read_value = "synthetic-read-only-value-5566778899";
                secrets::set_backup_secret(SERVER, "backup:unregistered", read_value).unwrap();
                let line = format!("launching with token {read_value}");
                assert!(secret_redact::redact_text(&line).contains(read_value));
                secrets::get_secret_result(SERVER, "backup:unregistered").unwrap();
                assert!(!secret_redact::redact_text(&line).contains(read_value));
            });
        }

        #[test]
        fn test08_client_config_backups_use_the_vault_and_stay_unregistered() {
            in_fork_mode(|host| {
                let backup_value = "synthetic backup body 9876543210 line two";
                secrets::set_backup_secret(SERVER, "backup:abc123", backup_value).unwrap();
                assert_eq!(host.ensures.get(), 1);
                assert_eq!(
                    secrets::get_backup_secret_result(SERVER, "backup:abc123")
                        .unwrap()
                        .as_deref(),
                    Some(backup_value)
                );
                let sealed = std::fs::read_to_string(vault_path()).unwrap();
                assert!(!sealed.contains("synthetic backup body"));
                assert!(secret_redact::redact_text(backup_value).contains("synthetic backup body"));
            });
        }
    }

    /// Mac check only (scripts/fork/MAC-CHECK-1.4.md). Opens a COPY of the data dir with the
    /// real login keychain, through the real adapter, and prints names only. This test
    /// binary is not in the master key's trusted-application list, so macOS asks once.
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "Mac check: reads the real login keychain"]
    fn mac_check_the_login_keychain_opens_a_copied_vault() {
        use std::rc::Rc;

        let copy = std::path::PathBuf::from(
            std::env::var("TOOLPORT_MAC_CHECK_DATA_DIR")
                .expect("set TOOLPORT_MAC_CHECK_DATA_DIR to a copy of the data dir"),
        );
        let live = dirs::home_dir()
            .unwrap()
            .join("Library/Application Support");
        assert!(
            !copy.starts_with(&live),
            "refusing to run against the live data dir; pass a copy outside {}",
            live.display()
        );
        let _lock = crate::registry::data_dir_test_lock();
        let _data = crate::registry::DataDirOverride::set(&copy);
        let _guard = super::secrets_host_override(Rc::new(crate::plus_adapter::HostAdapter));
        assert!(
            super::login_keychain_enabled(),
            "the copy has no secrets-backend marker"
        );

        let registry = crate::registry::load_resolved().unwrap();
        let (mut env_present, mut env_total, mut oauth_present) = (0, 0, 0);
        for server in &registry.servers {
            for var in server.env.iter().filter(|var| var.secret) {
                env_total += 1;
                match crate::secrets::get_secret_result(&server.id, &var.key) {
                    Ok(Some(_)) => {
                        env_present += 1;
                        println!("env present: {}/{}", server.id, var.key);
                    }
                    Ok(None) => println!("env missing: {}/{}", server.id, var.key),
                    Err(error) => panic!("{}/{}: {error}", server.id, var.key),
                }
            }
            if server.url.is_some() {
                let found = [
                    crate::secrets::HTTP_AUTH_KEY,
                    crate::remote::OAUTH_STATE_KEY,
                ]
                .into_iter()
                .any(|key| {
                    matches!(
                        crate::secrets::get_secret_result(&server.id, key),
                        Ok(Some(_))
                    )
                });
                if found {
                    oauth_present += 1;
                }
                println!(
                    "session {}: {}",
                    if found { "present" } else { "missing" },
                    server.id
                );
            }
        }
        let expect = |name: &str, default: usize| {
            std::env::var(name)
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(default)
        };
        println!("env secrets {env_present}/{env_total}, remote sessions {oauth_present}");
        assert_eq!(env_present, expect("TOOLPORT_MAC_CHECK_EXPECT_ENV", 13));
        assert_eq!(
            oauth_present,
            expect("TOOLPORT_MAC_CHECK_EXPECT_SESSIONS", 5)
        );
    }

    mod vault_backup {
        use crate::secrets;

        const SERVER: &str = "synthetic-server";

        fn vault_path() -> std::path::PathBuf {
            crate::registry::conduit_dir().unwrap().join("secrets.enc")
        }

        fn backup_path() -> std::path::PathBuf {
            toolport_plus::vault_backup::backup_path(&vault_path())
        }

        #[test]
        fn test01_the_first_write_has_no_previous_generation_to_back_up() {
            secrets::tests::with_isolated_vault(|| {
                secrets::set_secret(SERVER, "ONE", "synthetic-first").unwrap();
                assert!(!backup_path().exists());
            });
        }

        #[test]
        fn test02_the_backup_is_written_before_each_replace_and_holds_the_previous_ciphertext() {
            secrets::tests::with_isolated_vault(|| {
                secrets::set_secret(SERVER, "ONE", "synthetic-first").unwrap();
                let first = std::fs::read(vault_path()).unwrap();

                secrets::set_secret(SERVER, "TWO", "synthetic-second").unwrap();
                let second = std::fs::read(vault_path()).unwrap();
                assert_ne!(second, first);
                assert_eq!(std::fs::read(backup_path()).unwrap(), first);

                secrets::delete_secret(SERVER, "ONE").unwrap();
                assert_eq!(
                    std::fs::read(backup_path()).unwrap(),
                    second,
                    "the backup rolls forward with every replace"
                );
            });
        }

        #[cfg(unix)]
        #[test]
        fn test03_the_backup_is_owner_only_and_never_holds_plaintext() {
            use std::os::unix::fs::PermissionsExt;
            secrets::tests::with_isolated_vault(|| {
                secrets::set_secret(SERVER, "ONE", "synthetic-plaintext-marker-first").unwrap();
                // A looser file from before must come out 0600 too.
                std::fs::write(backup_path(), "stale").unwrap();
                std::fs::set_permissions(backup_path(), std::fs::Permissions::from_mode(0o644))
                    .unwrap();
                secrets::set_secret(SERVER, "TWO", "synthetic-plaintext-marker-second").unwrap();

                let mode = std::fs::metadata(backup_path())
                    .unwrap()
                    .permissions()
                    .mode();
                assert_eq!(mode & 0o777, 0o600, "mode {mode:o}");
                let text = std::fs::read_to_string(backup_path()).unwrap();
                assert!(!text.contains("synthetic-plaintext-marker"), "{text}");
                let leftovers: Vec<_> = std::fs::read_dir(crate::registry::conduit_dir().unwrap())
                    .unwrap()
                    .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                    .filter(|name| name.ends_with(".conduit-tmp"))
                    .collect();
                assert!(leftovers.is_empty(), "{leftovers:?}");
            });
        }

        #[test]
        fn test04_copying_the_backup_back_restores_the_previous_generation() {
            secrets::tests::with_isolated_vault(|| {
                secrets::set_secret(SERVER, "ONE", "synthetic-first").unwrap();
                secrets::set_secret(SERVER, "TWO", "synthetic-second").unwrap();

                std::fs::copy(backup_path(), vault_path()).unwrap();
                assert_eq!(
                    secrets::get_secret_result(SERVER, "ONE")
                        .unwrap()
                        .as_deref(),
                    Some("synthetic-first")
                );
                assert_eq!(secrets::get_secret_result(SERVER, "TWO").unwrap(), None);
            });
        }

        #[test]
        fn test05_a_failed_backup_write_aborts_the_replace_and_keeps_the_vault() {
            secrets::tests::with_isolated_vault(|| {
                secrets::set_secret(SERVER, "ONE", "synthetic-first").unwrap();
                let before = std::fs::read(vault_path()).unwrap();
                // A directory in the backup's place makes the backup write fail.
                std::fs::create_dir(backup_path()).unwrap();

                let error = secrets::set_secret(SERVER, "TWO", "synthetic-second").unwrap_err();
                assert!(error.contains("secrets.enc.bak"), "{error}");
                assert!(error.contains("left unchanged"), "{error}");
                assert_eq!(std::fs::read(vault_path()).unwrap(), before);

                let error = secrets::delete_secret(SERVER, "ONE").unwrap_err();
                assert!(error.contains("secrets.enc.bak"), "{error}");
                assert_eq!(std::fs::read(vault_path()).unwrap(), before);
                assert_eq!(
                    secrets::get_secret_result(SERVER, "ONE")
                        .unwrap()
                        .as_deref(),
                    Some("synthetic-first")
                );
            });
        }

        #[test]
        fn test06_a_damaged_vault_is_not_rolled_into_the_backup() {
            secrets::tests::with_isolated_vault(|| {
                secrets::set_secret(SERVER, "ONE", "synthetic-first").unwrap();
                secrets::set_secret(SERVER, "TWO", "synthetic-second").unwrap();
                let good_backup = std::fs::read(backup_path()).unwrap();

                std::fs::write(vault_path(), "@@@ damaged @@@").unwrap();
                assert!(secrets::set_secret(SERVER, "THREE", "synthetic-third").is_err());
                assert_eq!(std::fs::read(backup_path()).unwrap(), good_backup);
                assert_eq!(std::fs::read(vault_path()).unwrap(), b"@@@ damaged @@@");
            });
        }
    }
}

#[cfg(all(test, not(feature = "plus")))]
mod tests_without_plus {
    #[test]
    fn a_replace_of_the_vault_writes_no_backup() {
        crate::secrets::tests::with_isolated_vault(|| {
            crate::secrets::set_secret("synthetic-server", "ONE", "synthetic-first").unwrap();
            crate::secrets::set_secret("synthetic-server", "TWO", "synthetic-second").unwrap();
            let dir = crate::registry::conduit_dir().unwrap();
            assert!(dir.join("secrets.enc").exists());
            assert!(!dir.join("secrets.enc.bak").exists());
        });
    }
}

#[cfg(test)]
mod uv_launcher_tests {
    use crate::downstream::is_download_launcher;
    #[cfg(feature = "plus")]
    use crate::downstream::{stdio_connect_timeout, LEADER_OPEN_BUDGET};

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| (*arg).to_string()).collect()
    }

    #[cfg(feature = "plus")]
    #[test]
    fn test01_uv_run_gets_the_launcher_connect_budget_in_every_spelling() {
        let run = args(&["run", "--directory", "/srv/mcp-server", "server"]);
        for command in ["uv", "/opt/homebrew/bin/uv", "UV.EXE", r"C:\tools\uv.exe"] {
            assert!(is_download_launcher(command, &run), "{command}");
            assert_eq!(stdio_connect_timeout(command, &run), LEADER_OPEN_BUDGET);
        }
        assert!(is_download_launcher("uv", &args(&["tool", "run", "x"])));
        assert_eq!(
            stdio_connect_timeout("uv run --directory /srv server", &[]),
            LEADER_OPEN_BUDGET,
            "a launch line packed into the command is split before it is matched"
        );
    }

    #[cfg(feature = "plus")]
    #[test]
    fn test02_uv_subcommands_that_start_no_server_keep_the_tight_budget() {
        for line in [&["sync"][..], &["pip", "list"], &["tool", "install", "x"]] {
            assert!(!is_download_launcher("uv", &args(line)), "{line:?}");
            assert_ne!(stdio_connect_timeout("uv", &args(line)), LEADER_OPEN_BUDGET);
        }
    }

    #[cfg(not(feature = "plus"))]
    #[test]
    fn test03_without_the_fork_uv_run_is_not_a_launcher() {
        assert!(!is_download_launcher("uv", &args(&["run", "server"])));
    }
}
