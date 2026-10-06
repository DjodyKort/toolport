use super::*;
use crate::plus::attention::{dismissals, ls};
use crate::plus::testutil::DataDirFx;
use crate::plus::update::net::{HttpClient, HttpError};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

const T0: i64 = 1_790_000_000;
const HOUR: i64 = 3600;

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid"])
        .args(["-c", "commit.gpgsign=false", "-c", "init.defaultBranch=main"])
        .args(["-c", "protocol.file.allow=always"])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

fn commit(dir: &Path, file: &str) {
    std::fs::write(dir.join(file), file).unwrap();
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", &format!("add {file}")]);
}

struct World {
    base: PathBuf,
    work: PathBuf,
    fork_pusher: PathBuf,
    upstream_pusher: PathBuf,
}

impl World {
    fn new(tag: &str) -> World {
        let base = std::env::temp_dir().join(format!("plus-update-watch-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let seed = base.join("seed");
        std::fs::create_dir_all(&seed).unwrap();
        git(&seed, &["init", "-q"]);
        commit(&seed, "README.md");
        let bare = |name: &str| {
            let dest = base.join(name);
            git(&seed, &["clone", "-q", "--bare", ".", dest.to_str().unwrap()]);
            dest
        };
        let (upstream, fork) = (bare("upstream.git"), bare("fork.git"));
        let clone = |src: &Path, name: &str| {
            let dest = base.join(name);
            git(&base, &["clone", "-q", src.to_str().unwrap(), dest.to_str().unwrap()]);
            dest
        };
        let work = clone(&fork, "work");
        git(&work, &["remote", "add", "upstream", upstream.to_str().unwrap()]);
        git(&work, &["fetch", "-q", "upstream"]);
        World { fork_pusher: clone(&fork, "fork-pusher"), upstream_pusher: clone(&upstream, "upstream-pusher"), base, work }
    }

    fn push_fork(&self, file: &str) {
        commit(&self.fork_pusher, file);
        git(&self.fork_pusher, &["push", "-q", "origin", "HEAD:main"]);
    }

    fn push_upstream(&self, file: &str) {
        commit(&self.upstream_pusher, file);
        git(&self.upstream_pusher, &["push", "-q", "origin", "HEAD:main"]);
    }

    fn server(&self) -> Value {
        json!({"id": "g", "name": "g", "transport": "stdio", "command": "node", "args": ["x.js"],
            "cwd": self.work.to_str().unwrap(),
            "mcpmSource": {"type": "git", "path": self.work.to_str().unwrap(), "remote": "origin", "branch": "main",
                "upstream": {"remote": "upstream", "branch": "main"}}})
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

#[derive(Clone, Default)]
struct Registry(Arc<Mutex<Option<String>>>);

impl Registry {
    fn publish(&self, version: Option<&str>) {
        *self.0.lock().unwrap() = version.map(String::from);
    }
}

impl HttpClient for Registry {
    fn get_text(&self, _url: &str, _headers: &[(String, String)]) -> Result<String, HttpError> {
        match &*self.0.lock().unwrap() {
            Some(version) => Ok(json!({"version": version}).to_string()),
            None => Err(HttpError::new(None, "offline")),
        }
    }

    fn download(&self, _url: &str, _dest: &Path) -> Result<String, HttpError> {
        Err(HttpError::new(None, "offline"))
    }
}

fn env(world: &World, registry: &Registry) -> Env {
    let mut env = Env::system();
    env.home = Some(world.base.clone());
    env.http = Box::new(registry.clone());
    env.npm_registry = "https://npm.example.invalid".into();
    env
}

fn fixture(tag: &str, world: &World, with_package: bool) -> DataDirFx {
    let fx = DataDirFx::new("plus-update-watch-reg", tag);
    let mut servers = vec![world.server()];
    if with_package {
        servers.push(json!({"id": "pkg", "name": "pkg", "transport": "stdio", "command": "npx", "args": ["-y", "pkg@1.0.0"]}));
    }
    fx.write_registry(&json!({"version": 1, "servers": servers,
        "profiles": [{"id": "default", "name": "Default", "enabledServerIds": []}], "activeProfileId": "default"}));
    fx
}

fn kinds(outcome: &Outcome) -> Vec<String> {
    outcome.raised.iter().map(|f| format!("{}:{}", f.server, f.kind)).collect()
}

#[test]
fn nothing_fires_without_a_change_and_each_new_state_fires_once() {
    let world = World::new("cycle");
    let registry = Registry::default();
    registry.publish(Some("1.0.0"));
    let _fx = fixture("cycle", &world, true);
    let env = env(&world, &registry);

    let first = tick_with(&env, T0, false).unwrap();
    assert!(first.ran);
    assert!(first.raised.is_empty(), "{:?}", first.raised);
    assert_eq!(first.active, 0);

    world.push_fork("FORK-1.md");
    let early = tick_with(&env, T0 + 23 * HOUR, false).unwrap();
    assert_eq!((early.ran, early.reason), (false, Some("not-due")));

    let second = tick_with(&env, T0 + 25 * HOUR, false).unwrap();
    assert_eq!(kinds(&second), ["g:fork"]);
    assert_eq!(second.raised[0].since, T0 + 25 * HOUR);

    let repeat = tick_with(&env, T0 + 50 * HOUR, false).unwrap();
    assert!(repeat.ran);
    assert!(repeat.raised.is_empty(), "{:?}", repeat.raised);
    assert_eq!(repeat.active, 1);

    world.push_fork("FORK-2.md");
    world.push_upstream("UP-1.md");
    registry.publish(Some("2.0.0"));
    let third = tick_with(&env, T0 + 75 * HOUR, false).unwrap();
    assert_eq!(kinds(&third), ["g:fork", "g:upstream", "pkg:package"]);
    assert_eq!(third.active, 3);

    let quiet = tick_with(&env, T0 + 100 * HOUR, false).unwrap();
    assert!(quiet.raised.is_empty(), "{:?}", quiet.raised);
}

#[test]
fn a_state_that_cleared_and_came_back_fires_again() {
    let world = World::new("recur");
    let registry = Registry::default();
    registry.publish(Some("2.0.0"));
    let _fx = fixture("recur", &world, true);
    let env = env(&world, &registry);

    assert_eq!(kinds(&tick_with(&env, T0, true).unwrap()), ["pkg:package"]);
    assert!(tick_with(&env, T0 + HOUR, true).unwrap().raised.is_empty());
    registry.publish(Some("1.0.0"));
    assert_eq!(tick_with(&env, T0 + 2 * HOUR, true).unwrap().active, 0);
    registry.publish(Some("2.0.0"));
    assert_eq!(kinds(&tick_with(&env, T0 + 3 * HOUR, true).unwrap()), ["pkg:package"]);
}

#[test]
fn a_failed_check_keeps_the_finding_without_raising_it_again() {
    let world = World::new("flaky");
    let registry = Registry::default();
    registry.publish(Some("2.0.0"));
    let _fx = fixture("flaky", &world, true);
    let env = env(&world, &registry);

    assert_eq!(kinds(&tick_with(&env, T0, true).unwrap()), ["pkg:package"]);
    registry.publish(None);
    let offline = tick_with(&env, T0 + HOUR, true).unwrap();
    assert_eq!((offline.errors, offline.active), (1, 1));
    assert!(offline.raised.is_empty());
    registry.publish(Some("2.0.0"));
    let back = tick_with(&env, T0 + 2 * HOUR, true).unwrap();
    assert!(back.raised.is_empty(), "{:?}", back.raised);
    assert_eq!(back.active, 1);
}

#[test]
fn a_diverged_fork_raises_a_conflict_that_needs_you() {
    let world = World::new("conflict");
    let registry = Registry::default();
    let _fx = fixture("conflict", &world, false);
    let env = env(&world, &registry);

    commit(&world.work, "LOCAL.md");
    world.push_fork("REMOTE.md");
    let outcome = tick_with(&env, T0, true).unwrap();
    assert_eq!(kinds(&outcome), ["g:conflict"]);
    let items = collect(&Ctx { now: T0 });
    assert_eq!(items.len(), 1);
    assert_eq!((items[0].level, items[0].from), (Level::NeedsYou, "update"));
    assert_eq!(items[0].id, "updates:g:conflict:1+1");
    assert_eq!(items[0].since, cron::rfc3339(T0));
}

#[test]
fn the_feed_lists_active_findings_and_a_dismissal_hides_only_that_state() {
    let world = World::new("feed");
    let registry = Registry::default();
    let _fx = fixture("feed", &world, false);
    let env = env(&world, &registry);

    world.push_upstream("UP-1.md");
    tick_with(&env, T0, true).unwrap();
    let ctx = Ctx { now: T0 + HOUR };
    let shown = ls(&ctx, &[("updates", collect)], None).unwrap();
    assert_eq!(shown["counts"]["look"], 1);
    let id = shown["items"][0]["id"].as_str().unwrap().to_string();
    assert_eq!(id, "updates:g:upstream:1");
    dismissals::dismiss(&id, None, T0, false).unwrap();
    assert_eq!(ls(&ctx, &[("updates", collect)], None).unwrap()["items"], json!([]));

    world.push_upstream("UP-2.md");
    tick_with(&env, T0 + 2 * HOUR, true).unwrap();
    let again = ls(&ctx, &[("updates", collect)], None).unwrap();
    assert_eq!(again["items"][0]["id"], "updates:g:upstream:2");
}

#[test]
fn the_off_switch_and_interval_persist_and_stop_the_check() {
    let world = World::new("settings");
    let registry = Registry::default();
    let _fx = fixture("settings", &world, false);
    let env = env(&world, &registry);

    assert_eq!(settings(), Settings { enabled: true, interval_hours: 24 });
    world.push_fork("FORK-1.md");
    set_settings(Some(false), Some(6)).unwrap();
    let off = tick_with(&env, T0, true).unwrap();
    assert_eq!((off.ran, off.reason), (false, Some("disabled")));
    assert!(collect(&Ctx { now: T0 }).is_empty());

    set_settings(Some(true), None).unwrap();
    assert_eq!(settings(), Settings { enabled: true, interval_hours: 6 });
    assert_eq!(kinds(&tick_with(&env, T0, false).unwrap()), ["g:fork"]);
    assert_eq!(tick_with(&env, T0 + 5 * HOUR, false).unwrap().reason, Some("not-due"));
    assert!(tick_with(&env, T0 + 6 * HOUR, false).unwrap().ran);

    for bad in [0, MAX_INTERVAL_HOURS + 1] {
        assert!(set_settings(None, Some(bad)).is_err(), "{bad}");
    }
}

#[test]
fn the_handlers_read_and_write_the_settings() {
    let _fx = DataDirFx::new("plus-update-watch-handler", "settings");
    assert_eq!(settings_handler(json!({})).unwrap(), json!({"enabled": true, "intervalHours": 24}));
    assert_eq!(
        settings_handler(json!({"enabled": false, "intervalHours": 12})).unwrap(),
        json!({"enabled": false, "intervalHours": 12})
    );
    assert!(settings_handler(json!({"intervalHours": "daily"})).is_err());
    let off = crate::plus::dispatch("plus.update.watchTick", json!({})).unwrap();
    assert_eq!((off["ran"].clone(), off["reason"].clone()), (json!(false), json!("disabled")));
    assert!(crate::plus::dispatch("plus.update.watchSettings", json!({})).is_ok());
}
