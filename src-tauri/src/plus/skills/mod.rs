//! Skills core: SKILL.md parser, content hash, lockfile, stale cleanup, collision backups and the
//! transpiler abstraction. Per-client transpilers are layered on top in later items.

pub mod agents;
pub mod assets;
pub mod audit;
pub mod bundle;
pub mod clock;
pub mod collisions;
pub mod git;
pub mod handlers;
pub mod json;
pub mod lint;
pub mod lock;
pub mod ops;
pub mod parser;
pub mod pyfs;
pub(crate) mod schema;
pub mod styles;
pub mod sync;
pub mod taps;
pub mod transpilers;
pub mod transpiler;

pub use assets::{
    compute_skill_hash, discover_assets, with_asset_policy, AssetPolicy, ASSET_ALLOWLIST,
    EXTRA_EXTENSIONS_ENV, MCPM_ASSET_ALLOWLIST,
};
pub use clock::{Clock, FixedClock, Instant, SystemClock};
pub use lock::{load_lockfile, save_lockfile, LockEntry, LockFile, LOCKFILE_NAME};
pub use parser::{discover_skills, parse_skill_file, Skill, SkillType};
pub use sync::{sync_skills, SyncOptions, SyncResult};
pub use transpiler::{TranspileResult, Transpiler, TranspilerRegistry};

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_features;
