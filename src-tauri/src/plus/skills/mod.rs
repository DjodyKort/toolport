//! Skills core: SKILL.md parser, content hash, lockfile, stale cleanup, collision backups and the
//! transpiler abstraction. Per-client transpilers are layered on top in later items.

pub mod agents;
pub mod api;
pub mod assets;
pub mod audit;
pub mod bundle;
pub mod clock;
pub mod collisions;
pub mod git;
pub mod handlers;
pub mod json;
pub mod kind;
pub mod lint;
pub mod lock;
pub mod ops;
pub mod parser;
pub mod pyfs;
pub mod repo;
pub mod repo_handlers;
pub(crate) mod schema;
pub mod state_handlers;
pub mod styles;
pub mod sync;
pub mod sync_report;
pub mod tap_handlers;
pub mod tap_ops;
pub mod taps;
pub mod transpilers;
pub mod transpiler;

pub use assets::{with_asset_policy, AssetPolicy};
pub use clock::{Clock, FixedClock, Instant, SystemClock};
pub use lock::{load_lockfile, save_lockfile, LOCKFILE_NAME};
pub use parser::{discover_skills, Skill, SkillType};
pub use sync::{sync_skills, SyncOptions};
pub use transpiler::{Transpiler, TranspilerRegistry};

#[cfg(test)]
pub use {assets::{compute_skill_hash, discover_assets}, transpiler::TranspileResult};

#[cfg(test)]
pub(crate) mod tap_fixtures;
#[cfg(test)]
mod tap_ops_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_features;
#[cfg(test)] mod lock_sync_prop_tests;
#[cfg(test)] mod parser_prop_tests;
#[cfg(test)] mod transpilers_prop_tests;
