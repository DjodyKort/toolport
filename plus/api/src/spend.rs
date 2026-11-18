//! Upstream's per-session catalog savings rows, read-only, for the Spend page's MCP part.

use serde_json::Value;

pub trait SavingsAccess {
    /// The rows of upstream's savings log (`savings-v4.jsonl` and its older files) as raw values;
    /// the fork reads the few fields it needs and leaves the rest alone.
    fn catalog_sessions(&self) -> Result<Vec<Value>, String>;

    /// Upstream's saved-tokens summary, shown beside spend and never added to it.
    fn catalog_summary(&self) -> Result<Value, String>;
}
