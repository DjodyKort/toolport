//! What the login probes and the sign-in flows need of upstream: the stdio launcher behind a
//! server's own `auth` subcommand, the remote OAuth flow and a browser. Probing a Google
//! refresh token goes through `HttpAccess` and `VaultAccess`; nothing here carries a secret.

use std::io::Read;
use std::sync::Arc;

/// Receives the consent URL of an interactive sign-in.
pub type ConsentSink = Arc<dyn Fn(&str) + Send + Sync>;

/// Why a server's `auth` subcommand could not be started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchFault {
    /// Not a stdio server, or no command to run.
    Unsupported,
    /// Upstream's spawn screening or a missing launch input refused it.
    Refused,
    /// The process could not be started.
    Spawn,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchError {
    pub fault: LaunchFault,
    pub message: String,
}

impl LaunchError {
    pub fn new(fault: LaunchFault, message: impl Into<String>) -> Self {
        Self {
            fault,
            message: message.into(),
        }
    }
}

/// A started `<command> <args> auth` process. It leads its own process group, so `kill` reaches
/// wrapper chains such as `npx` to `node`.
pub trait AuthChild: Send {
    /// The child's stderr, handed out once.
    fn take_stderr(&mut self) -> Option<Box<dyn Read + Send>>;

    /// The exit code once the child has ended, `-1` when a signal ended it.
    fn try_exit(&mut self) -> Option<i32>;

    fn kill(&mut self);

    /// Leaves the child running to finish the browser round trip on its own.
    fn detach(self: Box<Self>);

    /// Removes launch inputs and vault values from a line before it is kept or shown.
    fn scrub(&self, line: String) -> String;

    /// A consent URL with the values the launcher bound into the arguments held back; `None`
    /// when the URL would carry one. `client_id` stays: OAuth makes it public.
    fn scrub_url(&self, url: String) -> Option<String>;

    /// True when the server's launch template can echo vault values, so stderr is not kept.
    fn hides_output(&self) -> bool;
}

pub trait AuthAccess {
    /// Starts the `auth` subcommand of a stdio server through upstream's own launch path:
    /// resolved launch inputs, environment, working directory and spawn screening.
    fn start_auth(&self, server_id: &str) -> Result<Box<dyn AuthChild>, LaunchError>;

    /// A team-synced server still waiting for its member review: nothing of it may start.
    fn awaits_review(&self, server_id: &str) -> Result<bool, String>;

    /// Runs upstream's browser sign-in for a remote server and stores the token it returns.
    /// Blocks until the browser flow ends. Upstream opens the browser itself, unless
    /// `on_consent` is given: then the consent URL goes there and the browser stays closed.
    fn sign_in_remote(
        &self,
        server_id: &str,
        on_consent: Option<ConsentSink>,
    ) -> Result<(), String>;

    fn open_url(&self, url: &str) -> Result<(), String>;
}
