//! The gateway side of the handshake switches: what `initialize` declares to a server, and which
//! server instructions join the gateway's own.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use toolport_plus::handshake::{self, DECLARATIONS, DECLARE_KEY, FORWARD_KEY};

use crate::registry::{self, Registry, ServerEntry};
use crate::router::Router;

/// How long a client's `initialize` waits for servers that opted in to catch up with its
/// declaration, and for the router to finish connecting servers whose instructions it forwards.
const WAIT: Duration = Duration::from_secs(10);
const READY_POLL: Duration = Duration::from_millis(50);

const CLIENT_CAPABILITIES_META: &str = "io.modelcontextprotocol/clientCapabilities";

fn opted_in(server: &ServerEntry) -> bool {
    server.enabled && handshake::switched_on(&server.unknown_fields, DECLARE_KEY)
}

fn forwards(server: &ServerEntry) -> bool {
    server.enabled
        && handshake::switched_on(&server.unknown_fields, FORWARD_KEY)
        && !crate::clients::is_gateway_server(server)
}

fn in_scope(server: &ServerEntry, allowed: Option<&HashSet<String>>) -> bool {
    allowed.is_none_or(|scope| scope.contains(&server.id))
}

pub fn downstream_capabilities(server_id: &str) -> Value {
    if DECLARATIONS.clients().is_empty() {
        return json!({});
    }
    let opted = registry::load().is_ok_and(|reg| {
        reg.servers
            .iter()
            .any(|server| server.id == server_id && opted_in(server))
    });
    if opted {
        handshake::declaration_for_connect(server_id)
    } else {
        json!({})
    }
}

pub fn forward_instructions(
    own: Option<String>,
    reg: &Registry,
    allowed: Option<&HashSet<String>>,
) -> Option<String> {
    let defend = reg.content_defense_effective();
    let sections: Vec<String> = reg
        .servers
        .iter()
        .filter(|server| forwards(server) && in_scope(server, allowed))
        .filter_map(|server| {
            let text = handshake::stored_instructions(&server.id)?;
            let screened = screen(&text, defend);
            if screened.is_none() {
                crate::gatewaylog::append(&format!(
                    "forwardInstructions: not forwarding the instructions of '{}': they match an injection signature",
                    server.id
                ));
            }
            handshake::section(&server.name, &screened?)
        })
        .collect();
    handshake::with_sections(own, &sections)
}

/// The text as it may reach the model: stored secrets redacted, forged gateway markers defanged,
/// and `None` when it trips the injection scan while content defense is on.
fn screen(text: &str, defend: bool) -> Option<String> {
    let redacted = crate::secret_redact::redact_text(text);
    let cleaned = handshake::clean(&redacted);
    let neutral = crate::integrity::neutralize_gateway_voice(cleaned.trim());
    (!defend || crate::integrity::scan_text(&neutral).is_empty()).then_some(neutral)
}

pub fn client_handshake(
    req: &Value,
    allowed: Option<&HashSet<String>>,
    registry: &Mutex<Registry>,
    router: &Mutex<Arc<Router>>,
    ready: &AtomicBool,
) {
    let method = req.get("method").and_then(Value::as_str).unwrap_or("");
    let is_initialize = method == "initialize";
    let declared = if is_initialize {
        req.pointer("/params/capabilities")
    } else {
        req.pointer("/params/_meta")
            .and_then(|meta| meta.get(CLIENT_CAPABILITIES_META))
    };
    let changed = DECLARATIONS.note_client(declared);

    let (opted, forwarding) = {
        let reg = registry.lock().unwrap_or_else(PoisonError::into_inner);
        let opted: Vec<String> = reg
            .servers
            .iter()
            .filter(|server| opted_in(server))
            .map(|server| server.id.clone())
            .collect();
        let forwarding = reg
            .servers
            .iter()
            .any(|server| forwards(server) && in_scope(server, allowed));
        (opted, forwarding)
    };
    let deadline = Instant::now() + WAIT;
    let announces = matches!(method, "initialize" | "server/discover");
    if (changed || is_initialize) && !opted.is_empty() || announces && forwarding {
        wait_until_ready(ready, deadline);
    }
    if (changed || is_initialize) && !opted.is_empty() {
        redeclare(router, opted, deadline);
    }
}

fn wait_until_ready(ready: &AtomicBool, deadline: Instant) {
    while !ready.load(Ordering::SeqCst) && Instant::now() < deadline {
        std::thread::sleep(READY_POLL);
    }
}

/// Restarts each opted-in server that was connected with an older declaration. A busy or slow
/// server that is not done by the deadline catches up in the background.
fn redeclare(router: &Mutex<Arc<Router>>, opted: Vec<String>, deadline: Instant) {
    let stale: Vec<String> = opted
        .into_iter()
        .filter(|id| DECLARATIONS.is_stale(id))
        .collect();
    if stale.is_empty() {
        return;
    }
    let live = Arc::clone(&router.lock().unwrap_or_else(PoisonError::into_inner));
    let (done, finished) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for id in stale {
            if live
                .plus_restart_server(&id)
                .is_some_and(|(restarted, _)| restarted)
            {
                eprintln!("conduit: re-spawned '{id}' to declare its client's capabilities");
            }
        }
        let _ = done.send(());
    });
    let _ = finished.recv_timeout(deadline.saturating_duration_since(Instant::now()));
}
