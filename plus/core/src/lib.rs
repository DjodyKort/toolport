//! Fork logic. Depends only on `toolport-plus-api`; upstream reaches it through
//! `seam.rs` in conduit and the binaries in `toolport-plus-bins` call it directly.

pub mod access;
mod args;
pub mod attention;
pub mod auth;
pub mod catalog_index;
pub mod compression;
pub mod context;
pub mod ctl;
pub mod direct;
pub mod egress;
pub mod fsx;
pub mod handshake;
mod hashing;
pub mod health;
pub mod http;
pub mod keychain_trust;
pub mod launcher;
pub mod library;
pub mod login_keychain;
pub mod memory;
pub mod oauth;
pub mod op;
pub mod plugins;
pub mod progress;
#[cfg(test)]
mod randutil;
pub mod reach;
mod redact;
pub mod resource_links;
pub mod selective;
pub mod selfmcp;
pub mod servers;
pub mod setup;
pub mod skills;
pub mod sources;
pub mod spend;
pub mod status;
pub mod sync;
pub mod tasks;
#[cfg(test)]
mod testutil;
pub mod vault_backup;
