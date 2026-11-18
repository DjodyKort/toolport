#![allow(dead_code)]

//! A world where something needs the person in every area the attention list reads: a server
//! whose secret the vault does not hold, a waiting task run, a task that failed an hour ago and a
//! sync the library held changes back from. The synthetic vault key makes the real vault readable
//! and empty.

use serde_json::json;
use toolport_plus::tasks::cron::rfc3339;

use super::library_world::git;
use super::tasks_world::tasks_home;
use super::world::{read_json, write_json, World};

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

pub fn attention_home(world: &World) {
    tasks_home(world);
    let registry = world.data.join("registry.json");
    let mut value = read_json(&registry);
    value["servers"][0]["env"] = json!([{"key": "API_KEY", "secret": true}]);
    write_json(&registry, &value);

    let at = now() - 3600;
    let step = json!({"id": "say", "title": "Say hello", "type": "exec", "status": "failed",
                      "startedAt": rfc3339(at), "endedAt": rfc3339(at + 1), "output": "boom"});
    write_json(
        &world.data.join("plus/task-runs/run-fixture-failed.json"),
        &json!({
            "id": "run-fixture-failed", "task": "nightly-report", "trigger": "schedule",
            "status": "failed", "startedAt": rfc3339(at), "endedAt": rfc3339(at + 1),
            "durationMs": 1000, "steps": [step],
        }),
    );

    write_json(
        &world.data.join("plus/skills-auto.json"),
        &json!({
            "paused": false, "since": null, "reason": null,
            "lastRun": {
                "at": rfc3339(now() - 7200), "applied": 2, "skipped": 0,
                "held": [{"skill": "demo", "client": "claude-code", "path": "/synthetic/demo",
                          "op": "overwrite", "reason": "the file differs from the library"}],
            },
        }),
    );
}

/// Everything in [`attention_home`] is gone: the secret is vaulted away from the registry's
/// declaration, the run finished and the held change was settled.
pub fn attention_quiet(world: &World) {
    attention_home(world);
    std::fs::remove_dir_all(world.data.join("plus/task-runs")).unwrap();
    std::fs::remove_file(world.data.join("plus/skills-auto.json")).unwrap();
    let registry = world.data.join("registry.json");
    let mut value = read_json(&registry);
    value["servers"][0]["env"] = json!([{"key": "API_KEY", "value": "plain", "secret": false}]);
    write_json(&registry, &value);
}

/// The busy world plus a skill tap whose upstream has one commit the clone has not fetched: the
/// row appears once `attention check` has fetched. Commits carry fixed dates, so their ids are
/// the same on every run.
pub fn attention_updates(world: &World) {
    attention_home(world);
    let upstream = world.base.join("tap-upstream");
    std::fs::create_dir_all(&upstream).unwrap();
    git(&upstream, &world.home, &["init", "-q", "-b", "main"]);
    std::fs::write(upstream.join("README.md"), "one").unwrap();
    git(&upstream, &world.home, &["add", "."]);
    git(&upstream, &world.home, &["commit", "-q", "-m", "one"]);
    let url = format!("file://{}", upstream.display());
    let clone = world.data.join("taps/team-skills");
    git(
        &world.base,
        &world.home,
        &[
            "clone",
            "-q",
            "--depth",
            "1",
            &url,
            &clone.to_string_lossy(),
        ],
    );
    write_json(
        &world.data.join("taps.json"),
        &json!({"team-skills": {"repo": url, "url": url}}),
    );
    std::fs::write(upstream.join("README.md"), "two").unwrap();
    git(&upstream, &world.home, &["commit", "-q", "-am", "two"]);
}
