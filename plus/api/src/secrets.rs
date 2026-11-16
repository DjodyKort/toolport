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
}
