//! Fork additions to the router, kept in a child module of `router` so they reach its private
//! slots. The module is declared by one line in `router.rs`; nothing else in that file names it.

use super::*;

/// How long a restart waits for a call that holds the connection (a transport that serialises its
/// calls) before it gives up and keeps the old process.
const SWAP_WAIT: Duration = Duration::from_secs(10);

impl Router {
    pub(crate) fn plus_holds_server(&self, server_id: &str) -> bool {
        self.by_id.contains_key(server_id)
    }

    /// Calls running on or waiting for the server in this process, including calls that wait for the
    /// client's input. `None` when this router does not hold the server.
    pub(crate) fn plus_calls_in_flight(&self, server_id: &str) -> Option<usize> {
        let slot = self.servers.get(*self.by_id.get(server_id)?)?;
        let running = slot.handle_calls.load(Ordering::Acquire);
        let suspended = slot
            .inner
            .try_lock()
            .map_or(0, |server| server.suspended_calls());
        Some(running + suspended)
    }

    /// Replaces the server's connection with a fresh one now, so a server whose code changed runs
    /// it. `None` when this router holds no such server; otherwise whether it restarted and a line
    /// for the result. The old connection stays when a fresh one cannot start.
    pub(crate) fn plus_restart_server(&self, server_id: &str) -> Option<(bool, String)> {
        let slot = self.servers.get(*self.by_id.get(server_id)?)?;
        let Some(supervisor) = &slot.supervisor else {
            let Some(reconnect) = &slot.reconnect else {
                return Some((
                    false,
                    "this connection cannot be restarted on request".into(),
                ));
            };
            return Some(match reconnect() {
                Some(fresh) => swap_connection(slot, None, fresh),
                None => (false, NOT_STARTED.into()),
            });
        };
        let connect = {
            let state = supervisor
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if slot.retired.load(Ordering::Acquire) {
                return Some((
                    false,
                    "it was reconfigured; the registry change replaces it".into(),
                ));
            }
            match state.state {
                SupervisorState::Ready => Arc::clone(&state.connect),
                SupervisorState::Stopped => {
                    return Some((
                        true,
                        "it was stopped for being idle; it starts fresh at its next call".into(),
                    ))
                }
                _ => {
                    drop(state);
                    slot.start(true);
                    return Some((
                        false,
                        format!(
                            "it is not connected; started a new attempt ({})",
                            slot.unavailable()
                        ),
                    ));
                }
            }
        };
        Some(match connect() {
            Ok(fresh) => swap_connection(slot, Some(supervisor), fresh),
            Err(failure) => (
                false,
                format!("{NOT_STARTED}: {}", client_safe_error(&failure.message)),
            ),
        })
    }
}

const NOT_STARTED: &str = "could not bring a fresh process up; the old one is still in place";

fn swap_connection(
    slot: &Arc<ServerSlot>,
    supervisor: Option<&Mutex<Supervisor>>,
    fresh: DownstreamServer,
) -> (bool, String) {
    let deadline = Instant::now() + SWAP_WAIT;
    loop {
        let lifecycle =
            supervisor.map(|s| s.lock().unwrap_or_else(std::sync::PoisonError::into_inner));
        let still_ready = !slot.retired.load(Ordering::Acquire)
            && lifecycle
                .as_ref()
                .is_none_or(|state| state.state == SupervisorState::Ready);
        if !still_ready {
            return (
                false,
                "it changed while the new process started; nothing was replaced".into(),
            );
        }
        // try_lock keeps a serial remote call from blocking the lifecycle lock.
        if let Ok(mut server) = slot.inner.try_lock() {
            *server = fresh;
            drop(server);
            let mut lifecycle = lifecycle;
            if let Some(state) = lifecycle.as_mut() {
                state.last_use = Instant::now();
                state.failures = 0;
            }
            slot.generation.fetch_add(1, Ordering::AcqRel);
            slot.tool_revision.fetch_add(1, Ordering::AcqRel);
            slot.breaker
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .record_success();
            return (true, "restarted".into());
        }
        drop(lifecycle);
        if Instant::now() >= deadline {
            return (
                false,
                "a call that cannot be interrupted is still running; the old process stays".into(),
            );
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::downstream::Transport;

    struct Quiet;

    impl Transport for Quiet {
        fn request(&mut self, method: &str, _params: Value) -> Result<Value, TransportError> {
            Ok(match method {
                "initialize" => json!({"protocolVersion": "2025-06-18", "capabilities": {}}),
                "tools/list" => json!({"tools": []}),
                _ => json!({}),
            })
        }

        fn notify(&mut self, _method: &str, _params: Value) -> Result<(), TransportError> {
            Ok(())
        }
    }

    fn connected(id: &str) -> DownstreamServer {
        DownstreamServer::connect(id.to_string(), Box::new(Quiet)).unwrap()
    }

    fn router_with(connect: Connect, state: SupervisorState) -> Router {
        let mut router = Router::new();
        router.add_supervised(
            "s".to_string(),
            Vec::new(),
            connect,
            ReconnectBackoff::default(),
            json!({}),
        );
        let slot = &router.servers[0];
        *slot.inner.lock().unwrap() = connected("s");
        slot.supervisor.as_ref().unwrap().lock().unwrap().state = state;
        router
    }

    fn counting(spawns: &Arc<AtomicU64>, fail: bool) -> Connect {
        let spawns = Arc::clone(spawns);
        Arc::new(move || {
            spawns.fetch_add(1, Ordering::SeqCst);
            if fail {
                Err(ConnectFailure {
                    message: "exec failed".into(),
                    needs_auth: false,
                    auth_target: None,
                })
            } else {
                Ok(connected("s"))
            }
        })
    }

    #[test]
    fn a_ready_server_gets_a_fresh_connection_and_its_slot_is_marked_changed() {
        let spawns = Arc::new(AtomicU64::new(0));
        let router = router_with(counting(&spawns, false), SupervisorState::Ready);
        let slot = Arc::clone(&router.servers[0]);
        let (generation, revision) = (
            slot.generation.load(Ordering::Acquire),
            slot.tool_revision.load(Ordering::Acquire),
        );
        assert_eq!(
            router.plus_restart_server("s"),
            Some((true, "restarted".into()))
        );
        assert_eq!(spawns.load(Ordering::SeqCst), 1, "one process is started");
        assert_eq!(slot.generation.load(Ordering::Acquire), generation + 1);
        assert_eq!(slot.tool_revision.load(Ordering::Acquire), revision + 1);
        assert_eq!(
            slot.supervisor.as_ref().unwrap().lock().unwrap().state,
            SupervisorState::Ready,
            "no gap for callers"
        );
        assert_eq!(router.plus_restart_server("ghost"), None);
    }

    #[test]
    fn a_fresh_process_that_cannot_start_leaves_the_old_one_in_place() {
        let spawns = Arc::new(AtomicU64::new(0));
        let router = router_with(counting(&spawns, true), SupervisorState::Ready);
        let slot = Arc::clone(&router.servers[0]);
        let generation = slot.generation.load(Ordering::Acquire);
        let (restarted, detail) = router.plus_restart_server("s").unwrap();
        assert!(!restarted);
        assert!(detail.starts_with(NOT_STARTED), "{detail}");
        assert_eq!(slot.generation.load(Ordering::Acquire), generation);
    }

    #[test]
    fn an_idle_stopped_server_has_nothing_to_replace_and_starts_fresh_on_use() {
        let spawns = Arc::new(AtomicU64::new(0));
        let router = router_with(counting(&spawns, false), SupervisorState::Stopped);
        let (restarted, detail) = router.plus_restart_server("s").unwrap();
        assert!(
            restarted && detail.contains("starts fresh at its next call"),
            "{detail}"
        );
        assert_eq!(spawns.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn calls_in_flight_counts_the_calls_running_on_the_slot() {
        let spawns = Arc::new(AtomicU64::new(0));
        let router = router_with(counting(&spawns, false), SupervisorState::Ready);
        assert_eq!(router.plus_calls_in_flight("s"), Some(0));
        router.servers[0].handle_calls.store(2, Ordering::SeqCst);
        assert_eq!(router.plus_calls_in_flight("s"), Some(2));
        assert_eq!(router.plus_calls_in_flight("ghost"), None);
        assert!(router.plus_holds_server("s") && !router.plus_holds_server("ghost"));
    }

    #[test]
    fn a_gateway_tick_answers_a_restart_request_in_the_data_dir_and_marks_the_lists_stale() {
        use std::sync::atomic::AtomicU8;
        use toolport_plus::servers::restart::{request, Kind};
        let data = crate::registry::DataDirTestEnv::new("plus-gateway-tick-restart");
        let spawns = Arc::new(AtomicU64::new(0));
        let router = Arc::new(router_with(
            counting(&spawns, false),
            SupervisorState::Ready,
        ));
        let live = Mutex::new(Arc::clone(&router));
        let dirty = Arc::new(AtomicU8::new(0));
        let elsewhere = request(&data.dir, Kind::Restart, "ghost", "Ghost").unwrap();
        let sent = request(&data.dir, Kind::Restart, "s", "S").unwrap();

        crate::seam::gateway_tick(&live, &dirty);
        crate::seam::gateway_tick(&live, &dirty);

        let answers = |nonce: &str| -> Vec<Value> {
            std::fs::read_dir(data.dir.join("server-restarts"))
                .unwrap()
                .flatten()
                .filter(|entry| {
                    // A file is written under a .tmp name and renamed, so only .json is whole.
                    let name = entry.file_name().to_string_lossy().into_owned();
                    name.starts_with(&format!("res-{nonce}-")) && name.ends_with(".json")
                })
                .map(|entry| serde_json::from_slice(&std::fs::read(entry.path()).unwrap()).unwrap())
                .collect()
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        while answers(&sent.nonce).is_empty() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        let answer = answers(&sent.nonce);
        assert_eq!(answer.len(), 1);
        assert_eq!(answer[0]["restarted"], true);
        assert_eq!(answer[0]["pid"], std::process::id());
        assert_eq!(
            spawns.load(Ordering::SeqCst),
            1,
            "one request restarts it once"
        );
        assert_eq!(
            dirty.load(Ordering::SeqCst),
            7,
            "tools, resources and prompts are read again"
        );
        assert!(
            answers(&elsewhere.nonce).is_empty(),
            "a server held elsewhere is not answered here"
        );
    }
}
