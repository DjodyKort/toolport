//! Fork logic. Depends only on `toolport-plus-api`; upstream reaches it through
//! `seam.rs` in conduit and the binaries in `toolport-plus-bins` call it directly.

mod args;
pub mod auth;
pub mod context;
pub mod ctl;
pub mod egress;
pub mod health;
pub mod http;
pub mod keychain_trust;
pub mod login_keychain;
pub mod op;
mod redact;
pub mod selfmcp;
pub mod servers;
pub mod status;
pub mod vault_backup;
