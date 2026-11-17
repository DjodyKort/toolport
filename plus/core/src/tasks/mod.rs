//! Tasks (D-067): named, repeatable jobs of steps, with a runner, triggers, a schedule ledger
//! and run records. The `task` command and the `tasks_*` self-MCP tools are thin renderers over
//! `api`; everything outside the data dir goes through `toolport_plus_api::TaskHost`.

pub mod api;
pub mod approval;
pub mod builtin;
pub mod cron;
pub mod model;
pub mod proc;
pub mod redactor;
pub mod runner;
pub mod schedule;
pub mod store;
pub mod triggers;

#[cfg(test)]
mod tests;
