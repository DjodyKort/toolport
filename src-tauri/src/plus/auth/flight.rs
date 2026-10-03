use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex};

enum Phase<T> {
    Running,
    Done(T),
    Abandoned,
}

struct Slot<T> {
    phase: Mutex<(Phase<T>, usize)>,
    cv: Condvar,
}

pub struct SingleFlight<T> {
    slots: Mutex<HashMap<String, Arc<Slot<T>>>>,
}

impl<T> Default for SingleFlight<T> {
    fn default() -> Self {
        SingleFlight {
            slots: Mutex::new(HashMap::new()),
        }
    }
}

struct LeaderGuard<'a, T> {
    flight: &'a SingleFlight<T>,
    key: &'a str,
    slot: Arc<Slot<T>>,
    finished: bool,
}

impl<T> Drop for LeaderGuard<'_, T> {
    fn drop(&mut self) {
        if !self.finished {
            let mut state = lock(&self.slot.phase);
            state.0 = Phase::Abandoned;
            drop(state);
            lock(&self.flight.slots).remove(self.key);
            self.slot.cv.notify_all();
        }
    }
}

fn lock<X>(m: &Mutex<X>) -> std::sync::MutexGuard<'_, X> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl<T: Clone> SingleFlight<T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn waiters(&self, key: &str) -> usize {
        let slot = lock(&self.slots).get(key).cloned();
        slot.map(|s| lock(&s.phase).1).unwrap_or(0)
    }

    pub fn run(&self, key: &str, f: impl FnOnce() -> T) -> T {
        let mut f = Some(f);
        loop {
            let existing = {
                let mut slots = lock(&self.slots);
                match slots.get(key) {
                    Some(slot) => Some(slot.clone()),
                    None => {
                        let slot = Arc::new(Slot {
                            phase: Mutex::new((Phase::Running, 0)),
                            cv: Condvar::new(),
                        });
                        slots.insert(key.to_string(), slot.clone());
                        drop(slots);
                        let mut guard = LeaderGuard {
                            flight: self,
                            key,
                            slot,
                            finished: false,
                        };
                        let value = (f.take().expect("leader runs once"))();
                        {
                            let mut state = lock(&guard.slot.phase);
                            state.0 = Phase::Done(value.clone());
                        }
                        lock(&self.slots).remove(key);
                        guard.finished = true;
                        guard.slot.cv.notify_all();
                        return value;
                    }
                }
            };
            let slot = existing.expect("follower has a slot");
            let mut state = lock(&slot.phase);
            state.1 += 1;
            loop {
                match &state.0 {
                    Phase::Done(value) => return value.clone(),
                    Phase::Abandoned => break,
                    Phase::Running => {
                        state = slot
                            .cv
                            .wait(state)
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                    }
                }
            }
        }
    }
}
