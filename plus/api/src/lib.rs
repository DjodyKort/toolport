//! Contract between the fork logic (`toolport-plus`) and upstream Toolport.
//!
//! This crate depends on nothing from upstream. Conduit implements the traits
//! in `src/plus_adapter.rs`; the fork logic only ever sees these items.

use std::path::PathBuf;

pub mod mirror;

pub use mirror::{convert, PlusData, ServerRecord};

/// Registry access for the fork logic.
///
/// Reads return mirror types. Writes are operations, never a whole mirrored
/// registry handed back: the adapter mutates upstream's own entry in place, so
/// fields the fork does not know survive.
pub trait RegistryAccess {
    fn registry_path(&self) -> Option<PathBuf>;

    fn list_servers(&self) -> Result<Vec<ServerRecord>, String>;

    fn profile_count(&self) -> Result<usize, String>;

    /// Fails when no server has `server_id`.
    fn set_server_enabled(&self, server_id: &str, enabled: bool) -> Result<(), String>;
}
