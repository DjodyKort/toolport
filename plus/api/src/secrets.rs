//! What the login-keychain secrets mode needs from upstream.
//!
//! The macOS keychain calls stay in upstream's platform code; conduit
//! implements this trait over them in `src/plus_adapter.rs`. The fork logic
//! decides which backend and which key source apply and never touches a
//! keychain itself.

use std::path::PathBuf;

/// Where the binaries that read the vault master key live.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrustInputs {
    /// The running binary.
    pub exe: PathBuf,
    /// The gateway upstream publishes or finds, when it resolves.
    pub gateway: Option<PathBuf>,
    /// `bin` inside the data dir, where Toolport publishes copies.
    pub data_bin: Option<PathBuf>,
    /// Further directories that may hold a Toolport binary (`~/.local/bin`).
    pub extra_dirs: Vec<PathBuf>,
}

pub trait SecretsHost {
    /// The Toolport data dir; the `secrets-backend` marker lives in it.
    fn data_dir(&self) -> Option<PathBuf>;

    /// A process environment variable, `None` when unset.
    fn env_var(&self, name: &str) -> Option<String>;

    /// Whether this OS has a login keychain the mode can use. Only macOS.
    fn login_keychain_available(&self) -> bool;

    /// The 32-byte vault master key in the login keychain; `Ok(None)` when no
    /// such item exists. Never creates one.
    fn read_master_key(&self) -> Result<Option<[u8; 32]>, String>;

    /// The master key, created first when it does not exist yet.
    fn ensure_master_key(&self) -> Result<[u8; 32], String>;

    /// Rewrites the trusted-application list of the existing master key item.
    /// `Ok(false)` when there is no item to rewrite.
    fn rewrite_master_key_acl(&self) -> Result<bool, String>;

    fn trust_inputs(&self) -> Result<TrustInputs, String>;

    /// Removes one entry of a server from the vault. A missing entry is not an error.
    fn delete_server_secret(&self, server_id: &str, key: &str) -> Result<(), String>;
}

/// Key under which a remote server's access token sits in the vault. Upstream's own constant is
/// `secrets::HTTP_AUTH_KEY`; a test in `plus_adapter.rs` keeps the two equal.
pub const HTTP_AUTH_KEY: &str = "__http_auth__";

/// The vault upstream keeps server secrets in, through upstream's own controllers: a write also
/// updates the server's entry in the registry and tells running gateways to reload.
pub trait VaultAccess {
    /// What the vault holds for the key. `Ok(None)` when nothing is stored; an error when the
    /// vault cannot be read, never an empty answer.
    fn read_vault_secret(&self, server_id: &str, key: &str) -> Result<Option<String>, String>;

    /// Vaults an environment secret and declares it on the server.
    fn store_env_secret(&self, server_id: &str, key: &str, value: &str) -> Result<(), String>;

    /// Vaults a launch argument input; no environment variable is declared.
    fn store_launch_secret(&self, server_id: &str, key: &str, value: &str) -> Result<(), String>;

    /// Stores the access token of a remote server and clears its previous sign-in state.
    fn store_access_token(&self, server_id: &str, token: &str) -> Result<(), String>;

    /// Removes an environment secret from the vault and from the server's entry.
    fn remove_env_secret(&self, server_id: &str, key: &str) -> Result<(), String>;

    /// Removes the access token and the sign-in state of a remote server.
    fn clear_access_token(&self, server_id: &str) -> Result<(), String>;
}
