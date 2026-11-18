//! A server that cannot interleave its calls gets them one at a time, in the order they arrived.
//!
//! The wait for a turn is the gateway's own: a call's deadline starts when it is sent to the
//! server, so time spent in the queue never runs it down.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Duration;

/// How often a queued call looks at whether its client gave up.
const POLL_SLICE: Duration = Duration::from_millis(50);

#[derive(Default)]
struct State {
    next: u64,
    queue: VecDeque<u64>,
    running: bool,
}

#[derive(Default)]
struct Gate {
    state: Mutex<State>,
    changed: Condvar,
}

static GATES: Mutex<Option<HashMap<String, Arc<Gate>>>> = Mutex::new(None);

fn gate_of(server_id: &str) -> Arc<Gate> {
    let mut gates = GATES.lock().unwrap_or_else(PoisonError::into_inner);
    gates
        .get_or_insert_with(HashMap::new)
        .entry(server_id.to_string())
        .or_default()
        .clone()
}

/// The right to send one call to the server; dropping it hands the next queued call its turn.
pub struct Turn {
    held: Option<Arc<Gate>>,
}

impl Turn {
    /// False when the call was cancelled while it queued: it was never given the server.
    pub fn granted(&self) -> bool {
        self.held.is_some()
    }
}

impl Drop for Turn {
    fn drop(&mut self) {
        let Some(gate) = self.held.take() else {
            return;
        };
        gate.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .running = false;
        gate.changed.notify_all();
    }
}

/// Waits for this call's turn on `server_id`. A call whose `cancelled` turns true while it waits
/// leaves the queue without a turn.
pub fn enter(server_id: &str, cancelled: impl Fn() -> bool) -> Turn {
    let gate = gate_of(server_id);
    let mut state = gate.state.lock().unwrap_or_else(PoisonError::into_inner);
    let ticket = state.next;
    state.next += 1;
    state.queue.push_back(ticket);
    loop {
        if !state.running && state.queue.front() == Some(&ticket) {
            state.queue.pop_front();
            state.running = true;
            drop(state);
            return Turn { held: Some(gate) };
        }
        if cancelled() {
            state.queue.retain(|queued| *queued != ticket);
            drop(state);
            gate.changed.notify_all();
            return Turn { held: None };
        }
        state = gate
            .changed
            .wait_timeout(state, POLL_SLICE)
            .unwrap_or_else(PoisonError::into_inner)
            .0;
    }
}

/// Calls queued for the server that have not been sent yet.
pub fn waiting(server_id: &str) -> usize {
    let gates = GATES.lock().unwrap_or_else(PoisonError::into_inner);
    gates
        .as_ref()
        .and_then(|gates| gates.get(server_id))
        .map_or(0, |gate| {
            gate.state
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .queue
                .len()
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Instant;

    fn wait_for(mut predicate: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !predicate() {
            assert!(Instant::now() < deadline, "condition never held");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn test01_calls_take_their_turn_in_arrival_order() {
        let first = enter("t01", || false);
        assert!(first.granted());
        let order = Arc::new(Mutex::new(Vec::new()));
        let mut workers = Vec::new();
        for n in 0..3 {
            let order = Arc::clone(&order);
            workers.push(std::thread::spawn(move || {
                let _turn = enter("t01", || false);
                order.lock().unwrap().push(n);
            }));
            wait_for(|| waiting("t01") == n + 1);
        }
        drop(first);
        for worker in workers {
            worker.join().unwrap();
        }
        assert_eq!(*order.lock().unwrap(), vec![0, 1, 2]);
        assert_eq!(waiting("t01"), 0);
    }

    #[test]
    fn test02_a_call_cancelled_while_queued_leaves_without_a_turn() {
        let first = enter("t02", || false);
        let cancelled = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancelled);
        let queued = std::thread::spawn(move || enter("t02", || flag.load(Ordering::SeqCst)));
        wait_for(|| waiting("t02") == 1);
        cancelled.store(true, Ordering::SeqCst);
        let turn = queued.join().unwrap();
        assert!(!turn.granted());
        assert_eq!(waiting("t02"), 0);
        drop(first);
        assert!(enter("t02", || false).granted(), "the gate is free again");
    }

    #[test]
    fn test03_a_cancelled_call_ahead_does_not_hold_up_the_one_behind() {
        let first = enter("t03", || false);
        let cancelled = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancelled);
        let ahead = std::thread::spawn(move || enter("t03", || flag.load(Ordering::SeqCst)));
        wait_for(|| waiting("t03") == 1);
        let behind = std::thread::spawn(|| enter("t03", || false).granted());
        wait_for(|| waiting("t03") == 2);
        cancelled.store(true, Ordering::SeqCst);
        assert!(!ahead.join().unwrap().granted());
        drop(first);
        assert!(behind.join().unwrap());
    }

    #[test]
    fn test04_servers_do_not_queue_behind_each_other() {
        let _a = enter("t04-a", || false);
        assert!(enter("t04-b", || false).granted());
    }
}
