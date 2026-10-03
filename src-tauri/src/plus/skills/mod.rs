//! Skills core: SKILL.md parser, content hash, lockfile, stale cleanup, collision backups and the
//! transpiler abstraction. Per-client transpilers are layered on top in later items.

pub mod assets;
pub mod clock;
pub mod collisions;
pub mod json;
pub mod lock;
pub mod parser;
pub mod sync;
pub mod transpiler;

pub use assets::{compute_skill_hash, discover_assets, ASSET_ALLOWLIST};
pub use clock::{Clock, FixedClock, Instant, SystemClock};
pub use lock::{load_lockfile, save_lockfile, LockEntry, LockFile, LOCKFILE_NAME};
pub use parser::{discover_skills, parse_skill_file, Skill, SkillType};
pub use sync::{sync_skills, SyncOptions, SyncResult};
pub use transpiler::{TranspileResult, Transpiler, TranspilerRegistry};

#[cfg(test)]
mod tests;
