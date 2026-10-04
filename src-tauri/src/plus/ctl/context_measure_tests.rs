//! The approval step of `context measure` at a terminal. Everything else is covered by
//! `tests/context_measure.rs`, which runs the real binary.

use super::context::measure_in;
use crate::plus::context::measure::{Env, ProcessLauncher};
use crate::plus::context::{load_config, Roots};
use crate::plus::testutil::DataDirFx;
use std::io::Cursor;
use std::path::PathBuf;

struct Fx {
    base: DataDirFx,
    roots: Roots,
    cwd: PathBuf,
}

impl Fx {
    fn new(tag: &str) -> Self {
        let base = DataDirFx::new("ctl-context-measure", tag);
        let home = base.dir.join("home");
        let cwd = home.join("work/repo");
        std::fs::create_dir_all(cwd.join(".git")).unwrap();
        Self {
            roots: Roots::from_home(&home),
            base,
            cwd,
        }
    }

    fn cache(&self) -> PathBuf {
        self.base.dir.join("data/plus/cache/measure")
    }

    fn run(&self, extra: &[&str], tty: bool, answer: &str) -> Result<String, String> {
        let config = load_config(&self.roots.context_config_path());
        let stub = ProcessLauncher::with_bin(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/loads/claude-stub.sh")
                .display()
                .to_string(),
        );
        let data = self.base.dir.join("data");
        let env = Env {
            roots: &self.roots,
            config: &config,
            data_dir: Some(&data),
            launcher: &stub,
        };
        let cwd = self.cwd.display().to_string();
        let mut argv = vec!["--cwd".to_string(), cwd];
        argv.extend(extra.iter().map(|s| s.to_string()));
        measure_in(&argv, &env, tty, &mut Cursor::new(answer.as_bytes().to_vec()))
            .map(|out| out.human)
            .map_err(|e| format!("{}: {}", e.kind.code(), e.message))
    }
}

#[test]
fn at_a_terminal_a_y_starts_the_measurement_and_anything_else_stops_it() {
    let fx = Fx::new("tty");
    for answer in ["n\n", "\n", "", "maybe\n"] {
        let err = fx.run(&[], true, answer).unwrap_err();
        assert!(err.contains("--yes"), "{answer:?}: {err}");
        assert!(!fx.cache().exists(), "{answer:?}: declined, yet something ran");
    }
    let human = fx.run(&[], true, "y\n").unwrap();
    assert!(human.contains("as is") && human.contains("68445 tokens"), "{human}");
    assert!(fx.cache().exists());
}

#[test]
fn a_cache_hit_does_not_ask_at_a_terminal() {
    let fx = Fx::new("tty-hit");
    fx.run(&[], false, "").unwrap_err();
    fx.run(&["--yes"], false, "").unwrap();
    let human = fx.run(&[], true, "").unwrap();
    assert!(human.contains("(cached)"), "{human}");
}

#[test]
fn without_a_terminal_only_yes_starts_a_measurement() {
    let fx = Fx::new("notty");
    assert!(fx.run(&[], false, "y\n").is_err(), "an answer on stdin is not a yes");
    assert!(!fx.cache().exists());
    let human = fx
        .run(&["--without", "plugin:kit@market", "--yes"], false, "")
        .unwrap();
    assert!(human.contains("without plugin:kit@market"), "{human}");
    assert!(human.contains("-8651 (-12.6%)"), "the delta is signed: {human}");
}
