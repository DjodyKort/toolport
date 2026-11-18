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
        Some(calls_in_flight(slot))
    }

    /// Replaces the server's connection with a fresh one now, so a server whose code changed runs
    /// it. `None` when this router holds no such server; otherwise whether it restarted and a line
    /// for the result. The old connection stays when a fresh one cannot start.
    pub(crate) fn plus_restart_server(&self, server_id: &str) -> Option<(bool, String)> {
        let slot = self.servers.get(*self.by_id.get(server_id)?)?;
        Some(restart_slot(slot))
    }
}

/// A launch the gateway keeps for one project folder, as `plus_calls_in_flight` and
/// `plus_restart_server` do for the router's own servers.
impl SharedServerSlot {
    pub(crate) fn plus_calls_in_flight(&self) -> usize {
        calls_in_flight(&self.0)
    }

    pub(crate) fn plus_restart(&self) -> (bool, String) {
        restart_slot(&self.0)
    }
}

fn calls_in_flight(slot: &ServerSlot) -> usize {
    let running = slot.handle_calls.load(Ordering::Acquire);
    let suspended = slot
        .inner
        .try_lock()
        .map_or(0, |server| server.suspended_calls());
    running + suspended + crate::seam::serial_waiting(&slot.id)
}

fn restart_slot(slot: &Arc<ServerSlot>) -> (bool, String) {
    let Some(supervisor) = &slot.supervisor else {
        let Some(reconnect) = &slot.reconnect else {
            return (
                false,
                "this connection cannot be restarted on request".into(),
            );
        };
        return match reconnect() {
            Some(fresh) => swap_connection(slot, None, fresh),
            None => (false, NOT_STARTED.into()),
        };
    };
    let connect = {
        let state = supervisor
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if slot.retired.load(Ordering::Acquire) {
            return (
                false,
                "it was reconfigured; the registry change replaces it".into(),
            );
        }
        match state.state {
            SupervisorState::Ready => Arc::clone(&state.connect),
            SupervisorState::Stopped => {
                return (
                    true,
                    "it was stopped for being idle; it starts fresh at its next call".into(),
                )
            }
            _ => {
                drop(state);
                slot.start(true);
                return (
                    false,
                    format!(
                        "it is not connected; started a new attempt ({})",
                        slot.unavailable()
                    ),
                );
            }
        }
    };
    match connect() {
        Ok(fresh) => swap_connection(slot, Some(supervisor), fresh),
        Err(failure) => (
            false,
            format!("{NOT_STARTED}: {}", client_safe_error(&failure.message)),
        ),
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

    fn folder_launch(spawns: &Arc<AtomicU64>, fail: bool) -> SharedServerSlot {
        let router = router_with(counting(spawns, fail), SupervisorState::Ready);
        router.server_slot("s").unwrap()
    }

    fn handle(
        pooled: Vec<(String, String, SharedServerSlot)>,
    ) -> crate::plus_adapter::RouterHandle {
        crate::plus_adapter::RouterHandle {
            router: Arc::new(Router::new()),
            pooled,
            dirty: Arc::new(std::sync::atomic::AtomicU8::new(0)),
        }
    }

    #[test]
    fn a_launch_kept_for_a_folder_restarts_and_counts_its_calls_like_a_router_slot() {
        let spawns = Arc::new(AtomicU64::new(0));
        let launch = folder_launch(&spawns, false);
        assert_eq!(launch.plus_calls_in_flight(), 0);
        launch.0.handle_calls.store(3, Ordering::SeqCst);
        assert_eq!(launch.plus_calls_in_flight(), 3);
        let generation = launch.0.generation.load(Ordering::Acquire);
        assert_eq!(launch.plus_restart(), (true, "restarted".into()));
        assert_eq!(spawns.load(Ordering::SeqCst), 1);
        assert_eq!(launch.0.generation.load(Ordering::Acquire), generation + 1);
    }

    #[test]
    fn a_restart_reaches_every_folder_instance_and_names_the_one_that_failed() {
        use toolport_plus_api::GatewayRouter;
        let (spawns, failing_spawns) = (Arc::new(AtomicU64::new(0)), Arc::new(AtomicU64::new(0)));
        let handle = handle(vec![
            (
                "s".into(),
                "/work/b".into(),
                folder_launch(&failing_spawns, true),
            ),
            ("s".into(), "/work/a".into(), folder_launch(&spawns, false)),
            (
                "other".into(),
                "/work/a".into(),
                folder_launch(&spawns, false),
            ),
        ]);
        assert!(handle.holds_server("s") && !handle.holds_server("ghost"));
        let outcomes = handle.restart_server("s").unwrap();
        let by_root = |root: &str| {
            outcomes
                .iter()
                .find(|o| o.root.as_deref() == Some(root))
                .unwrap()
        };
        assert_eq!(outcomes.len(), 2, "the other server's folder is left alone");
        assert!(by_root("/work/a").restarted);
        assert!(!by_root("/work/b").restarted);
        assert!(by_root("/work/b").detail.starts_with(NOT_STARTED));
        assert_eq!(spawns.load(Ordering::SeqCst), 1);
        assert_eq!(failing_spawns.load(Ordering::SeqCst), 1);
        assert_eq!(handle.restart_server("ghost"), None);
    }

    #[test]
    fn calls_in_flight_are_counted_per_folder_and_beside_the_shared_connection() {
        use toolport_plus_api::{CallsInFlight, GatewayRouter};
        let spawns = Arc::new(AtomicU64::new(0));
        let (busy, quiet) = (folder_launch(&spawns, false), folder_launch(&spawns, false));
        busy.0.handle_calls.store(2, Ordering::SeqCst);
        let mut handle = handle(vec![
            ("s".into(), "/work/busy".into(), busy),
            ("s".into(), "/work/quiet".into(), quiet),
        ]);
        let calls = |root: Option<&str>, calls| CallsInFlight {
            root: root.map(String::from),
            calls,
        };
        assert_eq!(
            handle.calls_in_flight("s"),
            Some(vec![
                calls(Some("/work/busy"), 2),
                calls(Some("/work/quiet"), 0)
            ])
        );
        handle.router = Arc::new(router_with(
            counting(&spawns, false),
            SupervisorState::Ready,
        ));
        assert_eq!(
            handle.calls_in_flight("s").unwrap()[0],
            calls(None, 0),
            "the shared connection comes first"
        );
        assert_eq!(handle.calls_in_flight("ghost"), None);
    }

    #[test]
    fn a_gateway_tick_reads_the_pool_only_while_a_request_waits() {
        use std::sync::atomic::AtomicU8;
        use toolport_plus::servers::restart::{request, Kind};
        let data = crate::registry::DataDirTestEnv::new("plus-gateway-tick-lazy-pool");
        let live = Mutex::new(Arc::new(Router::new()));
        let dirty = Arc::new(AtomicU8::new(0));
        let reads = AtomicU64::new(0);
        let pool = || {
            reads.fetch_add(1, Ordering::SeqCst);
            Vec::new()
        };

        crate::seam::gateway_tick(&live, &dirty, &pool);
        crate::seam::gateway_tick(&live, &dirty, &pool);
        assert_eq!(reads.load(Ordering::SeqCst), 0, "nothing waits");

        request(&data.dir, Kind::Probe, "s", "S").unwrap();
        crate::seam::gateway_tick(&live, &dirty, &pool);
        assert_eq!(reads.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_gateway_tick_restarts_the_folder_instances_the_pool_hands_it() {
        use std::sync::atomic::AtomicU8;
        use toolport_plus::servers::restart::{request, Kind};
        let data = crate::registry::DataDirTestEnv::new("plus-gateway-tick-folders");
        let spawns = Arc::new(AtomicU64::new(0));
        let launches = [
            (
                "s".to_string(),
                "/work/a".to_string(),
                folder_launch(&spawns, false),
            ),
            (
                "s".to_string(),
                "/work/b".to_string(),
                folder_launch(&spawns, false),
            ),
        ];
        let live = Mutex::new(Arc::new(Router::new()));
        let dirty = Arc::new(AtomicU8::new(0));
        let sent = request(&data.dir, Kind::Restart, "s", "S").unwrap();

        crate::seam::gateway_tick(&live, &dirty, &|| launches.to_vec());

        let deadline = Instant::now() + Duration::from_secs(5);
        let answer = loop {
            let found = std::fs::read_dir(data.dir.join("server-restarts"))
                .unwrap()
                .flatten()
                .find(|entry| {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    name.starts_with(&format!("res-{}-", sent.nonce)) && name.ends_with(".json")
                });
            if let Some(entry) = found {
                break serde_json::from_slice::<Value>(&std::fs::read(entry.path()).unwrap())
                    .unwrap();
            }
            assert!(Instant::now() < deadline, "no answer");
            std::thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(answer["restarted"], true);
        assert_eq!(answer["detail"], "restarted in 2 project folders");
        assert_eq!(answer["roots"][0]["root"], "/work/a");
        assert_eq!(answer["roots"][1]["root"], "/work/b");
        assert_eq!(
            spawns.load(Ordering::SeqCst),
            2,
            "one fresh process per folder"
        );
        assert_eq!(dirty.load(Ordering::SeqCst), 7);
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

        crate::seam::gateway_tick(&live, &dirty, &Vec::new);
        crate::seam::gateway_tick(&live, &dirty, &Vec::new);

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

    struct Recording {
        initializes: Arc<Mutex<Vec<Value>>>,
    }

    impl Transport for Recording {
        fn request(&mut self, method: &str, params: Value) -> Result<Value, TransportError> {
            Ok(match method {
                "initialize" => {
                    self.initializes.lock().unwrap().push(params);
                    json!({"protocolVersion": "2025-06-18", "capabilities": {}})
                }
                "tools/list" => json!({"tools": []}),
                _ => json!({}),
            })
        }

        fn notify(&mut self, _method: &str, _params: Value) -> Result<(), TransportError> {
            Ok(())
        }
    }

    fn recorded_slot(router: &mut Router, id: &str) -> Arc<Mutex<Vec<Value>>> {
        let initializes = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&initializes);
        let name = id.to_string();
        let connect: Connect = Arc::new(move || {
            Ok(DownstreamServer::connect(
                name.clone(),
                Box::new(Recording {
                    initializes: Arc::clone(&seen),
                }),
            )
            .unwrap())
        });
        router.add_supervised(
            id.to_string(),
            Vec::new(),
            Arc::clone(&connect),
            ReconnectBackoff::default(),
            json!({}),
        );
        let slot = Arc::clone(router.servers.last().unwrap());
        *slot.inner.lock().unwrap() = connect().ok().unwrap();
        slot.supervisor.as_ref().unwrap().lock().unwrap().state = SupervisorState::Ready;
        initializes
    }

    fn declared(initializes: &Mutex<Vec<Value>>) -> Vec<Value> {
        initializes
            .lock()
            .unwrap()
            .iter()
            .map(|params| params["capabilities"].clone())
            .collect()
    }

    fn handshake(
        capabilities: Value,
        registry: &Mutex<crate::registry::Registry>,
        router: &Mutex<Arc<Router>>,
    ) {
        crate::seam::client_handshake(
            &json!({ "method": "initialize", "params": { "capabilities": capabilities } }),
            None,
            registry,
            router,
            &std::sync::atomic::AtomicBool::new(true),
        );
    }

    #[test]
    fn test01_a_server_that_opted_in_restarts_once_per_new_client_declaration() {
        let _env = crate::registry::DataDirTestEnv::new("s8-redeclare");
        toolport_plus::handshake::DECLARATIONS.reset();
        let mut reg = crate::registry::Registry::default();
        for (id, on) in [("odh", true), ("plain", false)] {
            let mut entry: crate::registry::ServerEntry = serde_json::from_value(json!({
                "id": id, "name": id, "transport": "stdio", "command": "node", "enabled": true
            }))
            .unwrap();
            if on {
                entry
                    .unknown_fields
                    .insert("declareClientCapabilities".into(), json!(true));
            }
            reg.servers.push(entry);
        }
        crate::registry::save(&reg).unwrap();
        let mut router = Router::new();
        let odh = recorded_slot(&mut router, "odh");
        let plain = recorded_slot(&mut router, "plain");
        let (registry, router) = (Mutex::new(reg), Mutex::new(Arc::new(router)));

        handshake(json!({ "tasks": {} }), &registry, &router);
        assert_eq!(
            declared(&odh),
            [json!({})],
            "nothing relayable was declared"
        );

        handshake(
            json!({ "elicitation": {}, "tasks": {} }),
            &registry,
            &router,
        );
        assert_eq!(declared(&odh), [json!({}), json!({ "elicitation": {} })]);

        handshake(json!({ "elicitation": {} }), &registry, &router);
        assert_eq!(
            declared(&odh).len(),
            2,
            "the same declaration restarts nothing"
        );

        handshake(json!({ "roots": {} }), &registry, &router);
        assert_eq!(
            declared(&odh).last().unwrap(),
            &json!({ "elicitation": {}, "roots": {} }),
            "the declaration is the union of the clients seen"
        );
        assert_eq!(
            declared(&plain),
            [json!({})],
            "a server that did not opt in is left alone"
        );
        toolport_plus::handshake::DECLARATIONS.reset();
    }

    #[test]
    fn test05_a_stale_server_with_a_call_in_flight_is_left_alone_until_the_next_client_connects() {
        let _env = crate::registry::DataDirTestEnv::new("s8-redeclare-busy");
        toolport_plus::handshake::DECLARATIONS.reset();
        let mut reg = crate::registry::Registry::default();
        let mut entry: crate::registry::ServerEntry = serde_json::from_value(json!({
            "id": "odh", "name": "odh", "transport": "stdio", "command": "node", "enabled": true
        }))
        .unwrap();
        entry
            .unknown_fields
            .insert("declareClientCapabilities".into(), json!(true));
        reg.servers.push(entry);
        crate::registry::save(&reg).unwrap();
        let mut router = Router::new();
        let odh = recorded_slot(&mut router, "odh");
        let (registry, router) = (Mutex::new(reg), Mutex::new(Arc::new(router)));
        let calls = |count: usize| {
            router.lock().unwrap().servers[0]
                .handle_calls
                .store(count, Ordering::SeqCst)
        };

        handshake(json!({ "tasks": {} }), &registry, &router);
        calls(1);
        handshake(json!({ "elicitation": {} }), &registry, &router);
        assert_eq!(declared(&odh), [json!({})], "a running call is not cut");

        calls(0);
        handshake(json!({ "elicitation": {} }), &registry, &router);
        assert_eq!(
            declared(&odh),
            [json!({}), json!({ "elicitation": {} })],
            "the next connect restarts it once the server is idle"
        );
        toolport_plus::handshake::DECLARATIONS.reset();
    }

    struct Linking {
        id: &'static str,
        listed: Option<&'static str>,
    }

    impl Transport for Linking {
        fn request(&mut self, method: &str, params: Value) -> Result<Value, TransportError> {
            Ok(match method {
                "initialize" => json!({
                    "protocolVersion": "2025-06-18",
                    "capabilities": { "tools": {}, "resources": {} }
                }),
                "tools/list" => json!({"tools": [{
                    "name": "link",
                    "description": "Links a resource",
                    "inputSchema": {"type": "object"}
                }]}),
                "resources/list" => json!({"resources": self.listed.iter()
                    .map(|uri| json!({"uri": uri, "name": "listed"})).collect::<Vec<_>>()}),
                "tools/call" => json!({"content": [
                    {"type": "text", "text": "see links"},
                    {"type": "resource_link", "uri": params["arguments"]["uri"], "name": "link"}
                ]}),
                "resources/read" => json!({"contents": [{
                    "uri": params["uri"],
                    "text": format!("{} read {}", self.id, params["uri"].as_str().unwrap())
                }]}),
                _ => json!({}),
            })
        }

        fn notify(&mut self, _method: &str, _params: Value) -> Result<(), TransportError> {
            Ok(())
        }
    }

    fn linking_router() -> Router {
        let mut router = Router::new();
        for (id, listed) in [("odh", None), ("git", Some("s8://listed/by-git"))] {
            let mut server =
                DownstreamServer::connect(id.to_string(), Box::new(Linking { id, listed }))
                    .unwrap();
            server.load_resources_prompts();
            router.add(server);
        }
        router
    }

    #[test]
    fn test02_a_resource_link_in_a_tool_result_makes_its_uri_readable_from_that_server() {
        let router = linking_router();
        let uri = "s8://dyn/test02";
        assert_eq!(router.resource_server(uri), None);
        assert!(router.read_resource(uri).is_err());

        let reply = router
            .route_call("odh__link", json!({ "uri": uri }))
            .unwrap();
        assert_eq!(
            reply["content"][1]["uri"], uri,
            "the result is passed on whole"
        );
        assert_eq!(router.resource_server(uri), Some("odh"));
        let read = router.read_resource(uri).unwrap();
        assert_eq!(read["contents"][0]["text"], format!("odh read {uri}"));
        assert_eq!(router.resource_server("s8://dyn/never-linked"), None);
    }

    #[test]
    fn test03_a_listed_uri_keeps_its_owner_whatever_a_link_says_and_the_first_link_wins() {
        let router = linking_router();
        router
            .route_call("odh__link", json!({ "uri": "s8://listed/by-git" }))
            .unwrap();
        assert_eq!(router.resource_server("s8://listed/by-git"), Some("git"));

        router
            .route_call("git__link", json!({ "uri": "s8://dyn/test03" }))
            .unwrap();
        router
            .route_call("odh__link", json!({ "uri": "s8://dyn/test03" }))
            .unwrap();
        assert_eq!(router.resource_server("s8://dyn/test03"), Some("git"));
    }

    #[test]
    fn test04_a_link_of_a_server_this_router_no_longer_holds_is_refused() {
        let router = linking_router();
        router
            .route_call("odh__link", json!({ "uri": "s8://dyn/test04" }))
            .unwrap();
        let mut other = Router::new();
        let mut git = DownstreamServer::connect(
            "git".to_string(),
            Box::new(Linking {
                id: "git",
                listed: None,
            }),
        )
        .unwrap();
        git.load_resources_prompts();
        other.add(git);
        assert_eq!(other.resource_server("s8://dyn/test04"), None);
        assert!(other.read_resource("s8://dyn/test04").is_err());
    }

    #[test]
    fn a_gateway_tick_starts_a_due_task_once_and_the_next_tick_in_its_slot_starts_nothing() {
        use std::sync::atomic::{AtomicI64, AtomicU8};
        use toolport_plus::tasks::{model, store};
        use toolport_plus_api::SharedClockTaskHost;
        /// 07:06:00 UTC, a minute before the slot of the hourly task below; a date no test
        /// that ticks on the real clock can share a minute with.
        const BEFORE_THE_SLOT: i64 = 1922511960;
        let data = crate::registry::DataDirTestEnv::new("plus-gateway-tick-tasks");
        let task = model::parse(
            &json!({
                "id": "nightly", "title": "Nightly", "enabled": true,
                "requires": {"servers": [], "commands": ["echo"]},
                "writesSecrets": [],
                "steps": [{"id": "say", "title": "Say", "type": "exec",
                           "program": "echo", "args": ["hi"]}],
                "triggers": {"manual": true, "cli": true,
                    "selfMcp": {"enabled": false, "approval": "every-run"},
                    "schedule": {"cron": "7 * * * *", "autoRun": true}, "onAuthFailure": []}
            })
            .to_string(),
        )
        .unwrap();
        store::save_task(&data.dir, &task).unwrap();
        let live = Mutex::new(Arc::new(Router::new()));
        let dirty = Arc::new(AtomicU8::new(0));
        let clock = Arc::new(AtomicI64::new(BEFORE_THE_SLOT));
        let tick = |seconds_in: i64| {
            clock.store(BEFORE_THE_SLOT + seconds_in, Ordering::SeqCst);
            let host = SharedClockTaskHost::new(data.dir.clone(), &clock);
            crate::seam::gateway_tick_with(host, &live, &dirty, &Vec::new)
        };
        let runs = || store::list_runs(&data.dir, Some("nightly"));

        tick(0).expect("the first tick of a minute").join().unwrap();
        assert!(
            runs().is_empty(),
            "a task seen for the first time waits for its slot"
        );
        assert!(
            tick(40).is_none(),
            "a second tick in the same minute is skipped"
        );

        tick(65).expect("a new minute").join().unwrap();
        assert_eq!(runs().len(), 1, "the due slot starts one run");
        assert_eq!(runs()[0].trigger, "schedule");

        tick(130).expect("another minute").join().unwrap();
        assert_eq!(runs().len(), 1, "the handled slot is not started again");
    }
}
