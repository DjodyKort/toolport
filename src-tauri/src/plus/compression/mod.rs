//! Compression policy and the `hrclaude` launcher (port of mcpm-compression's policy side).
//! The model, pure operations, shims and the launch plan carry no I/O of their own; the
//! store and `SystemOps` are the only parts that touch disk or processes.

pub mod apply;
pub mod capability;
pub mod engine;
pub mod handlers;
pub mod launch;
pub mod ledger;
pub mod legacy;
pub mod manage;
pub mod mcp_entry;
pub mod model;
pub mod ops;
pub mod provider;
pub mod shims;
pub mod store;
pub mod verify;

#[cfg(test)] mod ledger_prop_tests;
#[cfg(test)] mod verify_prop_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
pub(crate) mod manage_tests;
#[cfg(test)]
mod verify_tests;
