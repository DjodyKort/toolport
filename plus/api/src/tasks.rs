//! What a task run needs from the outside world: the clock, the data dir, upstream's tool calls,
//! the vault, process spawning and the approval broker. The runner and the scheduler only see
//! this trait, so tests run them against [`FakeTaskHost`] and execute nothing real.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;

use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessSpec {
    pub program: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessOutcome {
    Exited { code: Option<i32>, output: String },
    Cancelled,
    SpawnFailed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunDecision {
    Approved,
    Denied,
    Timeout,
    Unreachable,
    Stale,
}

impl RunDecision {
    pub fn is_approved(self) -> bool {
        self == Self::Approved
    }
}

/// What the person is shown before a self-MCP client starts a task.
#[derive(Debug, Clone, PartialEq)]
pub struct RunApproval {
    pub task: String,
    pub steps: Vec<String>,
    pub writes_secrets: Value,
    pub requires: Value,
}

pub trait TaskHost {
    /// The data dir the task files, run records and the schedule ledger live under.
    fn tasks_base(&self) -> Option<PathBuf>;

    /// Seconds since the Unix epoch.
    fn now(&self) -> i64;

    fn task_call_tool(&self, server: &str, tool: &str, args: Value) -> Result<Value, String>;

    /// Runs an inline script whose calls may only reach `servers`.
    fn task_run_script(
        &self,
        source: &str,
        args: Value,
        servers: &[String],
    ) -> Result<Value, String>;

    fn task_set_secret(&self, server: &str, key: &str, value: &str) -> Result<(), String>;

    fn task_secret_is_set(&self, server: &str, key: &str) -> bool;

    fn task_restart_server(&self, server: &str) -> Result<String, String>;

    fn task_server_installed(&self, server: &str) -> bool;

    /// Runs a child to its end. `cancelled` is polled; once it answers true the child's process
    /// group is killed. `started` gets the child's pid as soon as it exists.
    fn task_run_process(
        &self,
        spec: &ProcessSpec,
        cancelled: &dyn Fn() -> bool,
        started: &dyn Fn(u32),
    ) -> ProcessOutcome;

    fn task_claude_program(&self) -> String;

    /// Starts the detached runner for a run record; returns at once.
    fn task_spawn_runner(&self, run_id: &str) -> Result<(), String>;

    fn task_process_alive(&self, pid: u32) -> bool;

    fn task_kill_group(&self, pid: u32);

    fn task_approve_run(&self, ask: &RunApproval) -> RunDecision;
}

/// A scripted [`TaskHost`]: a clock the test sets, calls it records and outcomes it was given.
#[derive(Debug)]
pub struct FakeTaskHost {
    pub base: RefCell<Option<PathBuf>>,
    pub clock: Cell<i64>,
    pub spawned: RefCell<Vec<String>>,
    pub spawn_error: RefCell<Option<String>>,
    pub tool_calls: RefCell<Vec<(String, String, Value)>>,
    pub tool_results: RefCell<VecDeque<Result<Value, String>>>,
    pub script_results: RefCell<VecDeque<Result<Value, String>>>,
    pub secrets: RefCell<BTreeMap<(String, String), String>>,
    pub restarts: RefCell<Vec<String>>,
    pub processes: RefCell<Vec<ProcessSpec>>,
    pub process_results: RefCell<VecDeque<ProcessOutcome>>,
    pub installed: RefCell<Option<Vec<String>>>,
    pub decision: Cell<RunDecision>,
    pub approvals: RefCell<Vec<RunApproval>>,
    pub alive: Cell<bool>,
    pub killed: RefCell<Vec<u32>>,
}

impl Default for FakeTaskHost {
    fn default() -> Self {
        Self {
            base: RefCell::new(None),
            clock: Cell::new(0),
            spawned: RefCell::default(),
            spawn_error: RefCell::default(),
            tool_calls: RefCell::default(),
            tool_results: RefCell::default(),
            script_results: RefCell::default(),
            secrets: RefCell::default(),
            restarts: RefCell::default(),
            processes: RefCell::default(),
            process_results: RefCell::default(),
            installed: RefCell::default(),
            decision: Cell::new(RunDecision::Unreachable),
            approvals: RefCell::default(),
            alive: Cell::new(true),
            killed: RefCell::default(),
        }
    }
}

impl FakeTaskHost {
    pub fn at(base: PathBuf, now: i64) -> Self {
        let host = Self::default();
        *host.base.borrow_mut() = Some(base);
        host.clock.set(now);
        host
    }

    pub fn advance(&self, seconds: i64) {
        self.clock.set(self.clock.get() + seconds);
    }
}

impl TaskHost for FakeTaskHost {
    fn tasks_base(&self) -> Option<PathBuf> {
        self.base.borrow().clone()
    }

    fn now(&self) -> i64 {
        self.clock.get()
    }

    fn task_call_tool(&self, server: &str, tool: &str, args: Value) -> Result<Value, String> {
        self.tool_calls
            .borrow_mut()
            .push((server.into(), tool.into(), args));
        self.tool_results
            .borrow_mut()
            .pop_front()
            .unwrap_or_else(|| Ok(Value::Null))
    }

    fn task_run_script(
        &self,
        _source: &str,
        _args: Value,
        _servers: &[String],
    ) -> Result<Value, String> {
        self.script_results
            .borrow_mut()
            .pop_front()
            .unwrap_or_else(|| Ok(Value::Null))
    }

    fn task_set_secret(&self, server: &str, key: &str, value: &str) -> Result<(), String> {
        self.secrets
            .borrow_mut()
            .insert((server.into(), key.into()), value.into());
        Ok(())
    }

    fn task_secret_is_set(&self, server: &str, key: &str) -> bool {
        self.secrets
            .borrow()
            .contains_key(&(server.to_string(), key.to_string()))
    }

    fn task_restart_server(&self, server: &str) -> Result<String, String> {
        self.restarts.borrow_mut().push(server.into());
        Ok(format!("restarted {server}"))
    }

    fn task_server_installed(&self, server: &str) -> bool {
        self.installed
            .borrow()
            .as_ref()
            .is_none_or(|known| known.iter().any(|id| id == server))
    }

    fn task_run_process(
        &self,
        spec: &ProcessSpec,
        _cancelled: &dyn Fn() -> bool,
        started: &dyn Fn(u32),
    ) -> ProcessOutcome {
        self.processes.borrow_mut().push(spec.clone());
        started(4242);
        self.process_results
            .borrow_mut()
            .pop_front()
            .unwrap_or(ProcessOutcome::Exited {
                code: Some(0),
                output: String::new(),
            })
    }

    fn task_claude_program(&self) -> String {
        "claude-fake".into()
    }

    fn task_spawn_runner(&self, run_id: &str) -> Result<(), String> {
        if let Some(error) = self.spawn_error.borrow().clone() {
            return Err(error);
        }
        self.spawned.borrow_mut().push(run_id.into());
        Ok(())
    }

    fn task_process_alive(&self, _pid: u32) -> bool {
        self.alive.get()
    }

    fn task_kill_group(&self, pid: u32) {
        self.killed.borrow_mut().push(pid);
    }

    fn task_approve_run(&self, ask: &RunApproval) -> RunDecision {
        self.approvals.borrow_mut().push(ask.clone());
        self.decision.get()
    }
}
