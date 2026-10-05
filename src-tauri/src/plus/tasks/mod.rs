//! Tasks (MIG-AUTO-1, D-067): named, repeatable jobs of steps, with a runner, triggers and run
//! records. The `task` command and the `tasks_*` self-MCP tools are thin renderers over `api`.

pub mod api;
pub mod approval;
pub mod builtin;
pub mod cron;
pub mod model;
pub mod redactor;
pub mod store;
pub mod host;
pub mod runner;
pub mod scheduler;
pub mod triggers;
#[cfg(test)]
mod api_tests;
#[cfg(test)]
mod builtin_tests;
#[cfg(test)]
mod tests;
