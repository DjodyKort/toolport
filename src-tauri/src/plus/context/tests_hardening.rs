use super::*;
use serde_json::json;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

struct Home(PathBuf);

impl Home {
    fn new(tag: &str) -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "ctx-hrd-{tag}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn roots(&self) -> Roots {
        Roots::from_home(&self.0)
    }

    fn arg(&self) -> String {
        self.0.to_string_lossy().into_owned()
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn config(extra: Value) -> ContextConfig {
    let mut base = json!({"wrap_default_claude": false, "dedupe": {"enabled": false}});
    for (k, v) in extra.as_object().unwrap() {
        base[k] = v.clone();
    }
    ContextConfig::from_value(base).unwrap()
}

fn names_under(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(names_under(&path));
        } else {
            out.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    out
}

fn real(persist: bool) -> ApplyOptions {
    ApplyOptions {
        persist,
        dry_run: false,
    }
}

fn run_args(home: &Home, profile: &str) -> Value {
    json!({
        "home": home.arg(),
        "config": {
            "wrap_default_claude": false,
            "dedupe": {"enabled": false},
            "profiles": {profile: {}},
        },
    })
}

#[test]
fn a_real_run_waits_for_the_context_lock_while_a_dry_run_does_not() {
    let home = Home::new("lock");
    let path = home.roots().context_config_path();
    let guard = crate::registry::lock_at(&path).unwrap();

    let planned = crate::plus::dispatch("plus.context.plan", run_args(&home, "locked")).unwrap();
    assert_eq!(planned["dryRun"], true);

    let writer = {
        let args = run_args(&home, "locked");
        std::thread::spawn(move || crate::plus::dispatch("plus.context.apply", args))
    };
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        !writer.is_finished() && !path.exists(),
        "a real run must not start while another holder has the context lock"
    );
    drop(guard);
    let applied = writer.join().unwrap().unwrap();
    assert_eq!(applied["dryRun"], false);
    assert!(path.is_file());
}

#[test]
fn a_save_leaves_no_temp_file_and_a_valid_config() {
    let home = Home::new("atomic");
    let path = home.roots().context_config_path();
    let cfg = config(json!({"profiles": {"research": {}}}));
    save_config(&path, &cfg).unwrap();
    save_config(&path, &cfg).unwrap();
    assert_eq!(load_config(&path), cfg);
    let names = names_under(path.parent().unwrap());
    assert!(
        names.iter().all(|n| !n.ends_with(".conduit-tmp")),
        "{names:?}"
    );
}

#[test]
fn concurrent_real_runs_never_tear_or_set_aside_a_valid_context_config() {
    let _budget = crate::registry::LockTimeoutOverride::generous();
    let home = Home::new("race");
    let path = home.roots().context_config_path();
    let (home, path) = (&home, &path);
    let done = AtomicBool::new(false);

    let reads = std::thread::scope(|scope| {
        let reader = scope.spawn(|| {
            let mut reads = 0;
            while !done.load(Ordering::SeqCst) {
                if let Ok(text) = fs::read_to_string(path) {
                    let value: Value = serde_json::from_str(&text).unwrap_or_else(|e| {
                        panic!("a reader saw a torn context.json ({e}): {text:?}")
                    });
                    ContextConfig::from_value(value).expect("a reader saw an invalid config");
                    reads += 1;
                }
            }
            reads
        });
        let writers: Vec<_> = (0..6)
            .map(|i| {
                scope.spawn(move || {
                    for _ in 0..4 {
                        let out = crate::plus::dispatch(
                            "plus.context.apply",
                            run_args(home, &format!("p{i}")),
                        )
                        .unwrap();
                        let actions = out["actions"].to_string();
                        assert!(
                            !actions.contains("unreadable"),
                            "a writer mistook a concurrent write for a damaged file: {actions}"
                        );
                    }
                })
            })
            .collect();
        for writer in writers {
            writer.join().unwrap();
        }
        done.store(true, Ordering::SeqCst);
        reader.join().unwrap()
    });
    assert!(reads > 0, "the reader never observed the file");

    let names = names_under(path.parent().unwrap());
    assert!(
        names
            .iter()
            .all(|n| !n.contains("context.json.unreadable-")),
        "a valid config was copied aside: {names:?}"
    );
    assert!(
        names.iter().all(|n| !n.ends_with(".conduit-tmp")),
        "{names:?}"
    );
    let last = load_config(path);
    assert_eq!(last.profiles.len(), 1, "{:?}", last.profiles.keys());
    assert!(last.profiles.keys().next().unwrap().starts_with('p'));
}

#[test]
fn a_planned_shim_write_is_not_reported_as_done() {
    let home = Home::new("shim-write");
    let roots = home.roots();
    let shims = roots.shims_path();
    let mut cfg = config(json!({"wrap_default_claude": true}));

    let planned = plan(&roots, &mut cfg).unwrap();
    assert!(
        planned
            .actions
            .contains(&format!("would write shims: {}", shims.display())),
        "{:?}",
        planned.actions
    );
    assert!(
        !planned.actions.iter().any(|a| a.starts_with("wrote shims")),
        "{:?}",
        planned.actions
    );
    assert!(!shims.exists());

    let applied = apply(&roots, &mut cfg, real(false)).unwrap();
    assert!(
        applied
            .actions
            .contains(&format!("wrote shims: {}", shims.display())),
        "{:?}",
        applied.actions
    );
    assert!(shims.is_file());
}

#[test]
fn a_planned_shim_removal_is_not_reported_as_done() {
    let home = Home::new("shim-remove");
    let roots = home.roots();
    let shims = roots.shims_path();
    apply(
        &roots,
        &mut config(json!({"wrap_default_claude": true})),
        real(false),
    )
    .unwrap();
    assert!(shims.is_file());

    let mut off = config(json!({"wrap_default_claude": false}));
    let planned = plan(&roots, &mut off).unwrap();
    assert!(
        planned
            .actions
            .contains(&"would remove shims (no profiles, wrapper disabled)".to_string()),
        "{:?}",
        planned.actions
    );
    assert!(shims.is_file());

    let applied = apply(&roots, &mut off, real(false)).unwrap();
    assert!(
        applied
            .actions
            .contains(&"removed shims (no profiles, wrapper disabled)".to_string()),
        "{:?}",
        applied.actions
    );
    assert!(!shims.exists());
}

#[test]
fn context_commands_word_a_dry_run_as_planned_and_a_real_run_as_done() {
    use crate::plus::ctl::context as ctl;
    let home = Home::new("ctl-wording");
    let flags = |extra: &[&str]| -> Vec<String> {
        let mut args: Vec<String> = vec!["--home".into(), home.arg()];
        args.extend(extra.iter().map(|s| s.to_string()));
        args
    };
    let planned_only = |text: &str| {
        assert!(text.contains("would write shims:"), "{text}");
        assert!(!text.contains("wrote shims:"), "{text}");
    };

    planned_only(&ctl::plan(&flags(&[])).unwrap().human);
    planned_only(&ctl::apply(&flags(&["--dry-run"])).unwrap().human);
    planned_only(&ctl::sync(&flags(&["--dry-run"])).unwrap().human);
    assert!(!home.roots().shims_path().exists());

    let synced = ctl::sync(&flags(&[])).unwrap().human;
    let (plan_part, apply_part) = synced.split_once("apply:\n").expect("sync prints both");
    planned_only(plan_part);
    assert!(apply_part.contains("wrote shims:"), "{apply_part}");
    assert!(!apply_part.contains("would write"), "{apply_part}");
    assert!(home.roots().shims_path().is_file());
}
