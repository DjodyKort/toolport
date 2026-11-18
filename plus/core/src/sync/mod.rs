//! Sync (S12): the setup file kept in step between machines. Everything that is written, read,
//! planned and applied is the setup module's; this module adds the sealed whole-file form, where
//! the file travels (a private Git remote or a folder), what both machines last agreed on, and
//! the rule that nothing is ever merged: a row both machines changed waits for a person.

pub mod config;
pub mod engine;
pub mod peers;
pub mod repos;
pub mod transport;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_repos;

pub use engine::{pull, push, setup, status, undo};

pub fn publishing(host: &dyn toolport_plus_api::Host) -> bool {
    host.data_dir()
        .is_some_and(|base| config::publishes_repos(&base))
}
