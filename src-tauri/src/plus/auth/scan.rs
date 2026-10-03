//! Probe runs: the one core behind `toolportctl auth probe`, `plus.auth.probe` and the gateway
//! due-scan. Probes only read credentials; every outcome lands in the 0600 status cache.

use std::collections::VecDeque;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, Mutex, OnceLock};

use serde::Serialize;
use serde_json::{json, Value};

use super::http_probes::{combined_registry, CompositeProbe};
use super::prober::{AuthProber, ProbeReport, Trigger};
use super::surfaces;
use super::{AuthStore, StatusFile, SystemClock};
use crate::plus::args::{flag, str_nonempty};
use crate::registry::Registry;

pub const MAX_PARALLEL: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selector {
    Due,
    All,
    One(String),
}

impl Selector {
    pub fn name(&self) -> &'static str {
        match self {
            Selector::Due => "due",
            Selector::All => "all",
            Selector::One(_) => "server",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeFailure {
    pub server: String,
    pub error: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProbeRun {
    pub reports: Vec<ProbeReport>,
    pub failures: Vec<ProbeFailure>,
}

impl ProbeRun {
    pub fn probed(&self) -> usize {
        self.reports.iter().filter(|r| r.ran).count()
    }

    pub fn servers(&self) -> Vec<String> {
        let reported = self.reports.iter().map(|r| r.server.clone());
        reported
            .chain(self.failures.iter().map(|f| f.server.clone()))
            .collect()
    }
}

pub fn shared_probe() -> Arc<CompositeProbe> {
    static PROBE: OnceLock<Arc<CompositeProbe>> = OnceLock::new();
    PROBE
        .get_or_init(|| Arc::new(CompositeProbe::default()))
        .clone()
}

/// A read-only registry view for probes and `toolportctl`: no lock file, and none of the
/// loader's recovery or migration writes.
pub fn read_registry() -> Result<Registry, String> {
    match crate::registry::resolved_path() {
        Some(path) => Ok(crate::plus::registry_ro::read_at(&path)?.unwrap_or_default()),
        None => Ok(Registry::default()),
    }
}

pub fn default_prober() -> Result<AuthProber, String> {
    let dir = surfaces::auth_dir().ok_or_else(|| "data directory unavailable".to_string())?;
    let registry = combined_registry(&read_registry()?);
    Ok(AuthProber::new(
        AuthStore::new(&dir),
        registry,
        shared_probe(),
        Arc::new(SystemClock),
    ))
}

fn request_guarded(
    prober: &AuthProber,
    server: &str,
    trigger: Trigger,
) -> Result<ProbeReport, String> {
    catch_unwind(AssertUnwindSafe(|| prober.request(server, trigger)))
        .unwrap_or_else(|_| Err("probe panicked".to_string()))
}

fn locked<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn run_targets(
    prober: &AuthProber,
    targets: Vec<String>,
    trigger: Trigger,
    parallel: usize,
) -> ProbeRun {
    let workers = parallel.clamp(1, targets.len().max(1));
    let queue = Mutex::new(targets.into_iter().enumerate().collect::<VecDeque<_>>());
    let done: Mutex<Vec<(usize, String, Result<ProbeReport, String>)>> = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                let next = locked(&queue).pop_front();
                let Some((index, server)) = next else { break };
                let result = request_guarded(prober, &server, trigger);
                locked(&done).push((index, server, result));
            });
        }
    });
    let mut done = done
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    done.sort_by_key(|(index, _, _)| *index);
    let mut run = ProbeRun::default();
    for (_, server, result) in done {
        match result {
            Ok(report) => run.reports.push(report),
            Err(error) => run.failures.push(ProbeFailure { server, error }),
        }
    }
    run
}

/// `Due` runs what the cache says is due, `All` forces every registered probe, `One` is a single
/// request that fails as a whole. A registry without probes touches nothing on disk.
pub fn run(
    prober: &AuthProber,
    selector: &Selector,
    force: bool,
    parallel: usize,
) -> Result<ProbeRun, String> {
    let trigger = if force || *selector == Selector::All {
        Trigger::UserForce
    } else {
        Trigger::Scheduled
    };
    let targets = match selector {
        Selector::One(server) => {
            return Ok(ProbeRun {
                reports: vec![prober.request(server, trigger)?],
                failures: Vec::new(),
            })
        }
        Selector::Due => prober.due()?,
        Selector::All => prober.registered(),
    };
    Ok(run_targets(prober, targets, trigger, parallel))
}

pub fn report_value(run: &ProbeRun, mode: &str, status: &StatusFile, now: i64) -> Value {
    let rows = surfaces::rows_for(status, now, &run.servers());
    json!({
        "mode": mode,
        "probes": run.reports,
        "failures": run.failures,
        "counts": surfaces::counts(&rows),
        "servers": rows,
    })
}

pub fn probe_handler(args: Value) -> Result<Value, String> {
    let server = str_nonempty(&args, "server").ok_or_else(|| "server is required".to_string())?;
    let run = run(
        &default_prober()?,
        &Selector::One(server.to_string()),
        flag(&args, "force"),
        1,
    )?;
    serde_json::to_value(&run.reports[0]).map_err(|e| e.to_string())
}
