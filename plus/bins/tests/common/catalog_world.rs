#![allow(dead_code)]

//! The registry index fixtures of the catalog index slice: files a sync would have left in the data
//! dir, with fixed timestamps so the index reads as stale on any day. Nothing here reaches a registry.

use serde_json::json;

use super::world::{write_json, World};

const SYNCED_AT: &str = "2026-01-01T00:00:00Z";

fn entry(id: &str, title: &str) -> serde_json::Value {
    json!({
        "id": id,
        "entry": {
            "name": title, "description": "Synthetic registry server", "transport": "http",
            "command": null, "args": [], "url": "https://widget.invalid/mcp", "envKeys": [],
            "source": "registry", "homepage": null, "publisher": "io.github.acme",
        },
    })
}

pub fn indexed(world: &World) {
    write_json(
        &world.data.join("plus/registry-index.json"),
        &json!({
            "version": 1,
            "lastSyncedAt": SYNCED_AT,
            "entries": [
                entry("io.github.acme/widget", "Widget"),
                entry("io.github.acme/gadget", "Gadget"),
            ],
        }),
    );
}

/// A sync that stopped after ten pages: its flush is on disk and the attempt is recorded as failed.
pub fn stopped(world: &World) {
    indexed(world);
    write_json(
        &world.data.join("plus/registry-index.part-00001.json"),
        &json!({
            "version": 1, "full": false, "startedAt": "2026-01-02T00:00:00Z", "since": SYNCED_AT,
            "nextCursor": "c10", "pages": 10, "entries": [], "retired": [],
        }),
    );
    write_json(
        &world.data.join("plus/registry-index-state.json"),
        &json!({
            "lastAttemptAt": "2026-01-02T00:00:00Z",
            "error": {"kind": "registry", "message": "the MCP Registry did not answer: connection failed: down"},
        }),
    );
}
