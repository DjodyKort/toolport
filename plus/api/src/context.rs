//! What the context features need from the process they run in, beyond the registry.

use std::path::PathBuf;

pub trait ContextHost {
    fn home_dir(&self) -> Option<PathBuf>;

    /// Upstream's offline token count (`cl100k_base`). It is not Claude's tokenizer, so callers
    /// present the result as an estimate.
    fn count_tokens(&self, text: &str) -> u64;
}
