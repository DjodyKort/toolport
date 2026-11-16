//! Skills core: SKILL.md parser, content hash, lockfile, sync with a file plan, collision backups
//! and the transpilers.

pub mod api;
pub mod assets;
pub mod audit;
pub mod client_scope;
pub mod clock;
pub mod collisions;
pub mod frontmatter;
pub mod json;
pub mod kind;
pub mod lint;
pub mod lock;
pub mod ops;
pub mod parser;
pub mod pyfs;
pub mod repo;
pub mod repo_handlers;
pub(crate) mod scalar;
pub mod state;
pub mod sync;
pub mod sync_report;
pub mod transpiler;
pub mod transpilers;
pub mod verify;

pub use clock::{Clock, FixedClock, Instant, SystemClock};
pub use lock::{load_lockfile, save_lockfile, LOCKFILE_NAME};
pub use parser::{discover_skills, Skill, SkillType};
pub use sync::{sync_skills, SyncOptions};
pub use transpiler::{Transpiler, TranspilerRegistry};

#[cfg(test)]
mod frontmatter_tests;
#[cfg(test)]
mod sync_files_tests;
#[cfg(test)]
mod tests;

#[cfg(test)]
pub use {assets::compute_skill_hash, transpiler::TranspileResult};
