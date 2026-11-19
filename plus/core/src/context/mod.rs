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
pub mod checkpoint;
pub mod config;
pub mod fsx;
pub mod gitlayout;
pub mod globs;
pub mod hash;
pub mod launch_profile;
pub mod layer_defs;
pub mod layer_deploy;
pub mod layer_list;
pub mod layers;
pub mod leave_out;
pub mod loads;
pub mod measure;
pub mod ops;
pub mod plan;
pub mod plugin_adapters;
pub mod plugin_config;
pub mod plugin_switch;
pub mod presync;
pub mod regenerate;
pub mod roots;
pub mod shell_rc;
pub mod shims;
pub mod verify;
pub mod wiring;

pub use roots::Roots;

#[cfg(test)]
mod testkit;
#[cfg(test)]
mod tests;
