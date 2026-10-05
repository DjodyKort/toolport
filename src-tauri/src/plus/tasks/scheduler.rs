//! The gateway watch loop calls [`tick`] on every pass: at most once a minute it hands the due
//! schedules to a thread that claims them and spawns a runner for each, and never waits.

use super::host::{Host, RealHost};
use super::triggers;
use std::sync::atomic::{AtomicI64, Ordering};

static LAST_MINUTE: AtomicI64 = AtomicI64::new(i64::MIN);

pub fn disabled() -> bool {
    std::env::var("TOOLPORT_TASKS_SCHEDULER").is_ok_and(|v| matches!(v.trim(), "0" | "off" | "false"))
}

pub fn claim_minute(epoch: i64) -> bool {
    let minute = epoch.div_euclid(60);
    LAST_MINUTE.swap(minute, Ordering::SeqCst) != minute
}

pub fn tick_at(epoch: i64, host: &dyn Host) -> Vec<triggers::Raised> {
    if disabled() || !claim_minute(epoch) {
        return Vec::new();
    }
    triggers::run_due(epoch, host)
}

pub fn tick() {
    if disabled() || !claim_minute(super::cron::now()) {
        return;
    }
    let epoch = super::cron::now();
    let _ = std::thread::Builder::new().name("task-schedule".into()).spawn(move || {
        let _ = std::panic::catch_unwind(|| triggers::run_due(epoch, &RealHost));
    });
}
