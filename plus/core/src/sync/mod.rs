//! Sync (S12): the setup file kept in step between machines. Everything that is written, read,
//! planned and applied is the setup module's; this module adds the sealed whole-file form, where
//! the file travels (a private Git remote or a folder), what both machines last agreed on, and
//! the rule that nothing is ever merged: a row both machines changed waits for a person.

pub mod config;
pub mod engine;
pub mod transport;

#[cfg(test)]
mod tests;

pub use engine::{pull, push, setup, status, undo};
