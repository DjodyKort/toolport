//! Skills core: SKILL.md parser, content hash, lockfile, sync with a file plan, collision backups
//! and the transpilers.

pub mod agents;
pub mod api;
pub mod assets;
pub mod audit;
pub mod auto;
pub mod bundle;
pub mod client_scope;
pub mod clock;
pub mod collisions;
pub mod compare;
pub mod files;
pub mod fix;
pub mod frontmatter;
pub mod git;
pub(crate) mod gitexec;
pub mod json;
pub mod keep;
pub mod kind;
pub mod lint;
pub mod lock;
pub mod ops;
pub mod origin;
pub mod parser;
pub mod pyfs;
pub mod repo;
pub mod repo_handlers;
pub(crate) mod scalar;
pub(crate) mod schema;
pub mod snapshot;
pub mod state;
pub mod styles;
pub mod sync;
pub mod sync_extras;
pub mod sync_report;
pub mod tap_handlers;
pub mod tap_ops;
pub mod taps;
pub mod textdiff;
pub mod transpiler;
pub mod transpilers;
pub mod verify;

pub use clock::{Clock, FixedClock, Instant, SystemClock};
pub use lock::{load_lockfile, save_lockfile, LOCKFILE_NAME};
pub use parser::{discover_skills, Skill, SkillType};
pub use sync::{sync_skills, SyncOptions};
pub use transpiler::{Transpiler, TranspilerRegistry};

#[cfg(test)]
mod checks_tests;
#[cfg(test)]
mod compare_tests;
#[cfg(test)]
mod extras_tests;
#[cfg(test)]
mod frontmatter_tests;
#[cfg(test)]
mod sync_files_tests;
#[cfg(test)]
mod tap_fixtures;
#[cfg(test)]
mod tap_ops_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_features;
#[cfg(test)]
mod undo_tests;

#[cfg(test)]
pub use {assets::compute_skill_hash, transpiler::TranspileResult};
