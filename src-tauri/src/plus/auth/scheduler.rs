//! Gateway due-scan. The gateway calls [`tick`] from its one-second registry-watch loop, which is
//! the only loop that runs in every long-lived mode (stdio, http and daemon). `tick` never waits:
//! at most once per [`TICK_SECS`] it hands the scan to its own thread, so request handling and the
//! registry watcher are never held up by a slow probe. Each probe keeps its own cadence and
//! backoff in the status cache, so several gateways on one data dir do not double-probe.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, Mutex, OnceLock};

use super::probe::{Clock, SystemClock};
use super::prober::AuthProber;
use super::scan::{self, ProbeRun, Selector};

pub const STARTUP_DELAY_SECS: i64 = 60;
pub const TICK_SECS: i64 = 60;
pub const DISABLE_ENV: &str = "TOOLPORT_AUTH_SCAN";

type ProberFactory = Box<dyn Fn() -> Result<AuthProber, String> + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tick {
    Waiting,
    Busy,
    Started,
}

struct State {
    next_at: i64,
    running: bool,
}

pub struct Scheduler {
    clock: Arc<dyn Clock>,
    prober: ProberFactory,
    parallel: usize,
    state: Mutex<State>,
}

impl Scheduler {
    pub fn new(clock: Arc<dyn Clock>, prober: ProberFactory) -> Arc<Self> {
        let next_at = clock.now() + STARTUP_DELAY_SECS;
        Arc::new(Scheduler {
            clock,
            prober,
            parallel: scan::MAX_PARALLEL,
            state: Mutex::new(State {
                next_at,
                running: false,
            }),
        })
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    #[cfg(test)]
    pub fn running(&self) -> bool {
        self.state().running
    }

    pub fn begin(&self) -> Tick {
        let now = self.clock.now();
        let mut state = self.state();
        if state.running {
            return Tick::Busy;
        }
        if now < state.next_at {
            return Tick::Waiting;
        }
        state.running = true;
        state.next_at = now + TICK_SECS;
        Tick::Started
    }

    fn finish(&self) {
        self.state().running = false;
    }

    pub fn scan_once(&self) -> Result<ProbeRun, String> {
        let prober = (self.prober)()?;
        scan::run(&prober, &Selector::Due, false, self.parallel)
    }

    fn scan_logged(&self) {
        let outcome = catch_unwind(AssertUnwindSafe(|| self.scan_once()))
            .unwrap_or_else(|_| Err("scan panicked".to_string()));
        match outcome {
            Err(error) => eprintln!("toolport: auth scan: {error}"),
            Ok(run) => {
                for failure in &run.failures {
                    eprintln!("toolport: auth scan: {}: {}", failure.server, failure.error);
                }
                let _ = catch_unwind(AssertUnwindSafe(|| hand_failed_logins_to_tasks(&run)));
            }
        }
        self.finish();
    }

    pub fn tick(self: &Arc<Self>) -> Tick {
        let began = self.begin();
        if began == Tick::Started {
            let scheduler = Arc::clone(self);
            let spawned = std::thread::Builder::new()
                .name("auth-scan".into())
                .spawn(move || scheduler.scan_logged());
            if spawned.is_err() {
                self.finish();
            }
        }
        began
    }
}

fn hand_failed_logins_to_tasks(run: &ProbeRun) {
    use super::types::AuthState;
    for report in &run.reports {
        if report.ran && matches!(report.tracked.state, AuthState::NeedsReauth | AuthState::Revoked) {
            crate::plus::tasks::triggers::on_auth_failure_once(&report.server, report.tracked.since, &crate::plus::tasks::host::RealHost);
        }
    }
}

pub(super) fn disabled() -> bool {
    std::env::var(DISABLE_ENV).is_ok_and(|v| matches!(v.trim(), "0" | "off" | "false"))
}

pub fn tick() -> Tick {
    static SCHEDULER: OnceLock<Arc<Scheduler>> = OnceLock::new();
    if disabled() {
        return Tick::Waiting;
    }
    SCHEDULER
        .get_or_init(|| Scheduler::new(Arc::new(SystemClock), Box::new(scan::default_prober)))
        .tick()
}
