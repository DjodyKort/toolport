//! Compression policy and the `hrclaude` launcher (port of mcpm-compression's policy side).
//! The model, pure operations, shims and the launch plan carry no I/O of their own; the
//! store and `SystemOps` are the only parts that touch disk or processes.

pub mod capability;
pub mod launch;
pub mod model;
pub mod ops;
pub mod provider;
pub mod shims;
pub mod store;

#[cfg(test)]
mod tests;

pub use model::{CompressionConfig, ProviderName};
