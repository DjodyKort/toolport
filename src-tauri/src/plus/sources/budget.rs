//! The per-detector budget: wall time, directory depth and an optional entry cap (a test seam).
//! A hit is recorded, never raised: the scan stops descending and the engine reports `partial`.

use std::cell::{Cell, RefCell};
use std::time::{Duration, Instant};

pub const DETECTOR_TIME: Duration = Duration::from_secs(2);
pub const TOTAL_TIME: Duration = Duration::from_secs(10);
pub const MAX_DEPTH: usize = 4;

pub struct Budget {
    deadline: Instant,
    max_depth: usize,
    max_entries: Option<usize>,
    entries: Cell<usize>,
    hit: RefCell<Option<String>>,
}

impl Budget {
    pub fn new(time: Duration, max_depth: usize, max_entries: Option<usize>) -> Self {
        Self {
            deadline: Instant::now() + time,
            max_depth,
            max_entries,
            entries: Cell::new(0),
            hit: RefCell::new(None),
        }
    }

    pub fn unlimited() -> Self {
        Self::new(Duration::from_secs(3600), MAX_DEPTH, None)
    }

    pub fn max_depth(&self) -> usize {
        self.max_depth
    }

    fn record(&self, reason: String) {
        let mut hit = self.hit.borrow_mut();
        let soft = |r: &str| r.starts_with("depth");
        if hit.as_deref().is_none_or(soft) && (hit.is_none() || !soft(&reason)) {
            *hit = Some(reason);
        }
    }

    /// True once the time or the entry budget is used up; records why.
    pub fn spent(&self) -> bool {
        if self
            .hit
            .borrow()
            .as_ref()
            .is_some_and(|r| !r.starts_with("depth"))
        {
            return true;
        }
        if Instant::now() >= self.deadline {
            self.record("time budget exhausted".into());
            return true;
        }
        false
    }

    /// Counts one directory entry; false when the walk must stop.
    pub fn tick(&self) -> bool {
        if self.spent() {
            return false;
        }
        let seen = self.entries.get() + 1;
        if self.max_entries.is_some_and(|cap| seen > cap) {
            self.record(format!("entry budget exhausted after {} entries", seen - 1));
            return false;
        }
        self.entries.set(seen);
        true
    }

    /// A content directory sat at the depth limit and was not entered.
    pub fn depth_cut(&self, path: &std::path::Path) {
        self.record(format!("depth limit reached at {}", path.display()));
    }

    pub fn remaining(&self) -> Duration {
        self.deadline.saturating_duration_since(Instant::now())
    }

    pub fn hit(&self) -> Option<String> {
        self.hit.borrow().clone()
    }

    pub fn entries(&self) -> usize {
        self.entries.get()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_zero_time_budget_is_spent_at_once_and_says_why() {
        let budget = Budget::new(Duration::ZERO, 4, None);
        assert!(budget.spent());
        assert!(!budget.tick());
        assert_eq!(budget.hit().as_deref(), Some("time budget exhausted"));
    }

    #[test]
    fn the_entry_cap_stops_the_count_after_the_nth_entry() {
        let budget = Budget::new(Duration::from_secs(60), 4, Some(3));
        assert!(budget.tick() && budget.tick() && budget.tick());
        assert!(!budget.tick());
        assert_eq!(budget.entries(), 3);
        assert!(budget.hit().unwrap().starts_with("entry budget exhausted"));
    }

    #[test]
    fn a_depth_cut_is_recorded_but_does_not_stop_the_scan() {
        let budget = Budget::unlimited();
        budget.depth_cut(std::path::Path::new("/a/b"));
        assert!(!budget.spent());
        assert!(budget.tick());
        assert_eq!(budget.hit().as_deref(), Some("depth limit reached at /a/b"));
    }
}
