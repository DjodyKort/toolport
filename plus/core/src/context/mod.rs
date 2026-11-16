//! Context features: folder bundles, leave-out, measuring and what loads. Every operation takes
//! explicit roots, a data directory and the host, so tests run in temp dirs and nothing reads `$HOME`.

pub mod bundle;
pub mod bundle_apply;
pub mod bundle_controls;
pub mod bundle_io;
pub mod bundle_json;
pub mod bundle_ledger;
pub mod bundle_store;
pub mod bundle_use;
pub mod config;
pub mod fsx;
pub mod globs;
pub mod hash;
pub mod layers;
pub mod leave_out;
pub mod loads;
pub mod ops;
pub mod plan;
pub mod roots;

pub use roots::Roots;

#[cfg(test)]
mod testkit;
#[cfg(test)]
mod tests;
