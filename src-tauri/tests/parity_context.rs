//! Replays the mcpm-context golden scenarios (inputs vendored under `fixtures/context-cases/`)
//! through the Rust context engine and compares the produced tree with the goldens.
//!
//! Launch-profile generation belongs to CTX-3, so the profile-bearing cases are compared on
//! everything except the generated `claude-profiles/` tree and the apply reports that list
//! profile actions; `profiles-reconcile` is not replayed at all.

mod common;

use common::parity::*;
use conduit_lib::plus::context::settings::{deep_merge_union, looks_secret_bearing};
use conduit_lib::plus::context::{
    self, doctor, layers, load_config, rules, shims, ApplyOptions, ContextConfig, Roots,
};
use conduit_lib::plus::skills::json::parse as parse_j;
use conduit_lib::plus::skills::{FixedClock, Instant};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn inputs_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/context-cases")
}

fn zsh_available() -> bool {
    Command::new("zsh").arg("-c").arg(":").output().is_ok()
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        // `.git` cannot be committed, so the worktree pointer fixture is stored as DOT_GIT.
        let name = if name == "DOT_GIT" { ".git".into() } else { name };
        let dest = to.join(name);
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &dest);
        } else {
            fs::copy(entry.path(), dest).unwrap();
        }
    }
}

fn snapshot(home: &Path) -> BTreeMap<String, Vec<u8>> {
    list_files(home)
        .unwrap()
        .into_iter()
        .map(|(rel, path)| (rel, fs::read(path).unwrap()))
        .collect()
}

fn glob_match(pattern: &str, text: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == text,
        Some((head, tail)) => {
            text.starts_with(head)
                && (0..=text.len() - head.len()).any(|i| {
                    text.is_char_boundary(head.len() + i)
                        && glob_match(tail, &text[head.len() + i..])
                })
        }
    }
}

fn subst(value: &Value, home: &str) -> Value {
    match value {
        Value::String(s) => Value::String(s.replace("{HOME}", home)),
        Value::Array(a) => Value::Array(a.iter().map(|v| subst(v, home)).collect()),
        Value::Object(m) => Value::Object(
            m.iter()
                .map(|(k, v)| (k.clone(), subst(v, home)))
                .collect::<Map<_, _>>(),
        ),
        other => other.clone(),
    }
}

fn pydump(value: &Value) -> String {
    let text = serde_json::to_string(value).unwrap();
    format!("{}\n", parse_j(&text).unwrap().dumps())
}

fn git(home: &Path, cwd: &Path, args: &[&str]) -> std::process::Output {
    Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", home)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.test")
        .output()
        .expect("git runs")
}

fn jq_merge(a: &Value, b: &Value) -> Value {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            let mut out = x.clone();
            for (k, v) in y {
                let merged = match x.get(k) {
                    Some(old) => jq_merge(old, v),
                    None => v.clone(),
                };
                out.insert(k.clone(), merged);
            }
            Value::Object(out)
        }
        _ => b.clone(),
    }
}

fn zsh_n(path: &Path) -> String {
    if !zsh_available() {
        return "zsh unavailable\n".into();
    }
    let out = Command::new("zsh").arg("-n").arg(path).output().unwrap();
    format!(
        "rc={}\n{}{}",
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

struct Scenario {
    home: PathBuf,
    roots: Roots,
    notes: Map<String, Value>,
}

impl Scenario {
    fn p(&self, rel: &str) -> PathBuf {
        self.home.join(rel)
    }

    fn emit(&self, name: &str, text: &str) {
        let out = self.p("_golden").join(name);
        fs::create_dir_all(out.parent().unwrap()).unwrap();
        fs::write(out, text).unwrap();
    }

    fn emit_json(&self, label: &str, value: &Value) {
        let name = if label.ends_with(".json") {
            label.to_string()
        } else {
            format!("{label}.json")
        };
        self.emit(&name, &pydump(value));
    }

    fn config_for(&self, step: &Value) -> ContextConfig {
        if step["saved"].as_bool().unwrap_or(false) {
            load_config(&self.roots.context_config_path())
        } else {
            ContextConfig::from_value(step["config"].clone()).unwrap()
        }
    }

    fn step(&mut self, step: &Value) {
        let s = |k: &str| step[k].as_str().unwrap_or_default().to_string();
        match step["op"].as_str().unwrap() {
            "write_file" => {
                let p = self.p(&s("path"));
                fs::create_dir_all(p.parent().unwrap()).unwrap();
                fs::write(&p, s("content")).unwrap();
            }
            "snapshot_file" => {
                let src = self.p(&s("src"));
                let text = fs::read_to_string(src).unwrap_or_else(|_| "<absent>\n".into());
                self.emit(&s("label"), &text);
            }
            "exists" => {
                let rows: Map<String, Value> = step["paths"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|p| {
                        let p = p.as_str().unwrap();
                        (p.to_string(), json!(self.p(p).exists()))
                    })
                    .collect();
                self.emit_json(&s("label"), &Value::Object(rows));
            }
            "list_backups" => {
                let base = self.roots.backups_dir();
                let mut names: Vec<String> = list_files(&base)
                    .map(|m| {
                        m.keys()
                            .map(|k| k.rsplit('/').next().unwrap().to_string())
                            .collect()
                    })
                    .unwrap_or_default();
                names.sort();
                self.emit_json(&s("label"), &json!(names));
            }
            "scaffold_personal" => {
                let r = layers::scaffold_personal_rule(&self.roots).unwrap();
                self.notes.insert("scaffold_personal".into(), self.rel_or_null(r));
            }
            "scaffold_client" => {
                let name = s("name");
                let r =
                    layers::scaffold_client_rule(&self.roots, &name, step["glob"].as_str()).unwrap();
                self.notes
                    .insert(format!("scaffold_client:{name}"), self.rel_or_null(r));
            }
            "skills_global" => {
                let clock = FixedClock(Instant { unix_secs: 1_767_225_600, micros: 0 });
                rules::deploy_rules(&self.roots, &clock, false).unwrap();
            }
            "cf_clobber" => self.cf_clobber(),
            "git_init" => {
                let p = self.p(&s("path"));
                fs::create_dir_all(&p).unwrap();
                let out = git(
                    &self.home,
                    &self.home,
                    &["init", "-q", "-b", "main", "--template=", p.to_str().unwrap()],
                );
                assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
            }
            "git_check" => {
                let repo = self.p(&s("path"));
                let mut rows = Map::new();
                for f in step["files"].as_array().unwrap() {
                    let f = f.as_str().unwrap();
                    let out = git(&self.home, &repo, &["check-ignore", "-q", f]);
                    rows.insert(f.to_string(), json!(out.status.success()));
                }
                let st = git(
                    &self.home,
                    &repo,
                    &["status", "--porcelain", "--untracked-files=all"],
                );
                let status: Vec<String> = String::from_utf8_lossy(&st.stdout)
                    .lines()
                    .map(String::from)
                    .collect();
                self.emit_json(&s("label"), &json!({ "ignored": rows, "status": status }));
            }
            "apply" => {
                let mut config = self.config_for(step);
                let persist = step["persist"].as_bool().unwrap_or(true);
                let report =
                    context::apply(&self.roots, &mut config, ApplyOptions { persist, dry_run: false })
                        .unwrap();
                self.emit_json(&s("label"), &report.to_value());
            }
            "doctor" => {
                let config = load_config(&self.roots.context_config_path());
                self.emit_json(&s("label"), &json!(doctor::run_checks(&self.roots, &config)));
            }
            "shim_snippet" => {
                let profiles: BTreeMap<String, context::config::ProfileSpec> = step["profiles"]
                    .as_object()
                    .map(|m| {
                        m.iter()
                            .map(|(n, v)| (n.clone(), serde_json::from_value(v.clone()).unwrap()))
                            .collect()
                    })
                    .unwrap_or_default();
                let text = shims::shim_snippet(
                    &self.roots,
                    &profiles,
                    step["wrap_default"].as_bool().unwrap(),
                );
                let label = s("label");
                self.emit(&format!("{label}.zsh"), &text);
                let tmp = self.p("_golden").join(format!("{label}.zsh"));
                self.emit(&format!("{label}.zsh-n.txt"), &zsh_n(&tmp));
            }
            "zsh_syntax" => {
                let out = zsh_n(&self.p(&s("path")));
                self.emit(&format!("{}.zsh-n.txt", s("label")), &out);
            }
            "zsh_script" => {
                if !zsh_available() {
                    self.emit(&format!("{}.txt", s("label")), "zsh unavailable\n");
                    return;
                }
                let out = Command::new("zsh")
                    .arg("-f")
                    .arg(self.p(&s("script")))
                    .current_dir(&self.home)
                    .env_clear()
                    .env("PATH", "/usr/bin:/bin")
                    .env("HOME", &self.home)
                    .env("TZ", "UTC")
                    .env("LC_ALL", "C.UTF-8")
                    .output()
                    .unwrap();
                self.emit(
                    &format!("{}.txt", s("label")),
                    &format!(
                        "rc={}\n--- stdout\n{}--- stderr\n{}",
                        out.status.code().unwrap_or(-1),
                        String::from_utf8_lossy(&out.stdout),
                        String::from_utf8_lossy(&out.stderr)
                    ),
                );
            }
            "pure_calls" => {
                let secrets: Map<String, Value> = step["secret_entries"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|e| {
                        let e = e.as_str().unwrap();
                        (e.to_string(), json!(looks_secret_bearing(e)))
                    })
                    .collect();
                let merges: Vec<Value> = step["merges"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|pair| {
                        let j = |v: &Value| parse_j(&serde_json::to_string(v).unwrap()).unwrap();
                        let merged = deep_merge_union(&j(&pair[0]), &j(&pair[1]));
                        serde_json::from_str(&merged.dumps()).unwrap()
                    })
                    .collect();
                self.emit_json(
                    &s("label"),
                    &json!({ "looks_secret_bearing": secrets, "deep_merge_union": merges }),
                );
            }
            other => panic!("unsupported scenario op {other}"),
        }
    }

    fn rel_or_null(&self, p: Option<PathBuf>) -> Value {
        match p {
            Some(p) => json!(p.strip_prefix(&self.home).unwrap().to_string_lossy()),
            None => Value::Null,
        }
    }

    /// Models cf-dev-tools' `sync_claude_files` from its fake install dir (A6).
    fn cf_clobber(&self) {
        let cf = self.p(".local/share/cf-dev-tools/claude");
        let ch = self.p(".claude");
        fs::create_dir_all(&ch).unwrap();
        if cf.join("CLAUDE.md").exists() {
            fs::copy(cf.join("CLAUDE.md"), ch.join("CLAUDE.md")).unwrap();
        }
        let team = cf.join("settings.json");
        if team.exists() {
            let cur = ch.join("settings.json");
            let base: Value = if cur.exists() {
                serde_json::from_str(&fs::read_to_string(&cur).unwrap()).unwrap()
            } else {
                json!({})
            };
            let team_v: Value = serde_json::from_str(&fs::read_to_string(&team).unwrap()).unwrap();
            let merged = jq_merge(&base, &team_v);
            if cur.exists() {
                if let Ok(out) = Command::new("jq")
                    .args(["-s", ".[0] * .[1]"])
                    .arg(&cur)
                    .arg(&team)
                    .output()
                {
                    if out.status.success() {
                        let theirs: Value = serde_json::from_slice(&out.stdout).unwrap();
                        assert_eq!(theirs, merged, "jq-merge model disagrees with jq");
                    }
                }
            }
            fs::write(&cur, format!("{}\n", serde_json::to_string_pretty(&merged).unwrap())).unwrap();
        }
        let cmds = cf.join("commands");
        if cmds.is_dir() {
            fs::create_dir_all(ch.join("commands")).unwrap();
            for e in fs::read_dir(cmds).unwrap() {
                let e = e.unwrap();
                if e.path().extension().is_some_and(|x| x == "md") {
                    fs::copy(e.path(), ch.join("commands").join(e.file_name())).unwrap();
                }
            }
        }
    }
}

fn is_zsh_derived(rel: &str) -> bool {
    rel.ends_with(".zsh-n.txt") || rel.ends_with("/throttle.txt") || rel.ends_with("/nowrap.txt")
}

fn replay(case: &str, profile_case: bool) {
    let dir = inputs_root().join(case);
    let spec: Value = serde_json::from_slice(&fs::read(dir.join("case.json")).unwrap()).unwrap();
    let root = std::env::temp_dir().join(format!("parity-context-{}-{case}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let home = root.join("home");
    fs::create_dir_all(&home).unwrap();
    if dir.join("input").is_dir() {
        copy_dir(&dir.join("input"), &home);
    }
    let before = snapshot(&home);
    let home_str = home.to_string_lossy().into_owned();
    let mut sc = Scenario {
        home: home.clone(),
        roots: Roots::from_home(&home),
        notes: Map::new(),
    };

    let args = subst(&spec["args"], &home_str);
    match spec["entry"].as_str().unwrap() {
        "context_apply" => {
            let mut config = ContextConfig::from_value(args["config"].clone()).unwrap();
            let persist = args["persist"].as_bool().unwrap_or(true);
            context::apply(&sc.roots, &mut config, ApplyOptions { persist, dry_run: false }).unwrap();
        }
        "context_scenario" => {
            for step in args["steps"].as_array().unwrap() {
                sc.step(step);
            }
            if !sc.notes.is_empty() {
                let label = args["notes_label"].as_str().unwrap_or("scaffold.json").to_string();
                sc.emit_json(&label, &Value::Object(sc.notes.clone()));
            }
        }
        other => panic!("unsupported entry {other}"),
    }

    let excludes: Vec<String> = spec["exclude"]
        .as_array()
        .map(|a| a.iter().map(|s| s.as_str().unwrap().to_string()).collect())
        .unwrap_or_default();
    let zsh = zsh_available();
    let skip = |rel: &str| {
        excludes.iter().any(|p| glob_match(p, rel))
            || (!zsh && is_zsh_derived(rel))
            || (profile_case
                && (rel.starts_with(".config/mcpm/claude-profiles/")
                    || (rel.starts_with("_golden/apply") && rel.ends_with(".report.json"))
                    || (rel.starts_with("_golden/") && rel.contains("profiles") && rel.ends_with(".zsh"))
                    || (rel.starts_with("_golden/") && rel.ends_with(".txt") && !rel.ends_with(".zsh-n.txt"))
                    || rel.ends_with("context-shims.zsh")))
    };

    let after = snapshot(&home);
    let actual = root.join("actual");
    for (rel, data) in &after {
        if skip(rel) || before.get(rel) == Some(data) {
            continue;
        }
        let out = actual.join(rel);
        fs::create_dir_all(out.parent().unwrap()).unwrap();
        fs::write(out, data).unwrap();
    }
    let sync_re = regex::Regex::new(r#"("synced_at"\s*:\s*)"[^"]*""#).unwrap();
    for (rel, path) in list_files(&actual).unwrap() {
        if rel.ends_with("mcpm-skills.lock") {
            let text = fs::read_to_string(&path).unwrap();
            fs::write(&path, sync_re.replace(&text, r#"$1"<SYNCED_AT>""#).into_owned()).unwrap();
        }
    }

    let golden = GoldenCase::load(&fixtures_root().join("context").join(case)).unwrap();
    let filtered = root.join("golden");
    let mut classes = Map::new();
    for (rel, path) in list_files(&golden.tree()).unwrap() {
        if skip(&rel) {
            continue;
        }
        let out = filtered.join("tree").join(&rel);
        fs::create_dir_all(out.parent().unwrap()).unwrap();
        fs::copy(path, out).unwrap();
        classes.insert(rel.clone(), json!({ "class": golden.classes[&rel] }));
    }
    let manifest = json!({ "comparison": golden.comparison, "files": classes });
    fs::write(filtered.join("manifest.json"), manifest.to_string()).unwrap();
    let filtered_case = GoldenCase::load(&filtered).unwrap();
    fs::create_dir_all(&actual).unwrap();
    let n = Normalizers {
        root: Some(root.to_string_lossy().into_owned()),
        home: Some(home_str),
        ignore_json_keys: Vec::new(),
    };
    let result = compare_case(&filtered_case, &actual, &n);
    let _ = fs::remove_dir_all(&root);
    if let Err(problems) = result {
        panic!("{case}: context engine diverges from mcpm golden\n{problems}");
    }
}

#[test]
fn layered_rules_match_golden() {
    replay("layered-rules", false);
}

#[test]
fn client_local_deploy_matches_golden() {
    replay("client-local-deploy", false);
    replay("client-local-missing-root", false);
}

#[test]
fn settings_union_and_cf_clobber_match_golden() {
    for case in ["settings-union", "settings-cf-clobber", "settings-edge"] {
        replay(case, false);
    }
}

#[test]
fn dedupe_matches_golden() {
    replay("dedupe-legacy-names", false);
}

#[test]
fn doctor_and_tripwire_match_golden() {
    for case in ["cf-tripwire", "cf-no-wrapper", "doctor-no-cf"] {
        replay(case, false);
    }
}

#[test]
fn shim_generation_matches_golden() {
    for case in ["shims-wrap-on", "shims-wrap-off", "shims-throttle", "shims-throttle-nowrap"] {
        replay(case, true);
    }
}

#[test]
fn every_declared_case_has_vendored_inputs_and_a_golden() {
    let all = [
        "layered-rules", "client-local-deploy", "client-local-missing-root", "settings-union",
        "settings-cf-clobber", "settings-edge", "dedupe-legacy-names", "cf-tripwire", "cf-no-wrapper",
        "doctor-no-cf", "shims-wrap-on", "shims-wrap-off", "shims-throttle", "shims-throttle-nowrap",
        "profiles-reconcile",
    ];
    for case in all {
        assert!(inputs_root().join(case).join("case.json").is_file(), "{case}");
        assert!(
            fixtures_root().join("context").join(case).join("manifest.json").is_file(),
            "{case}"
        );
    }
}
