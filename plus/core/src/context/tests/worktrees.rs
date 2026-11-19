use crate::context::bundle_apply::Desired;
use crate::context::bundle_controls as controls;
use crate::context::fsx;
use crate::context::layers::ensure_exclude_line;
use crate::context::leave_out::{self, Args, Op, Target, World, AUTO_MEMORY_OFF, CONTROL};
use crate::context::loads::{InstructionLoads, LoadSource, WhatLoads};
use crate::context::roots::Roots;
use crate::memory::resolve::project_slug;
use std::path::PathBuf;

struct Fx {
    _dir: tempfile::TempDir,
    base: PathBuf,
    roots: Roots,
    data: PathBuf,
}

impl Fx {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let base = fsx::canonical(dir.path());
        let data = base.join("data");
        std::fs::create_dir_all(&data).unwrap();
        let fx = Self {
            roots: Roots::from_home(&base),
            _dir: dir,
            base,
            data,
        };
        fx.put("work/app/CLAUDE.md", "project rules\n");
        std::fs::create_dir_all(fx.at("work/app/.git/info")).unwrap();
        std::fs::create_dir_all(fx.at("work/app/src/deep")).unwrap();
        fx
    }

    fn at(&self, rel: &str) -> PathBuf {
        self.base.join(rel)
    }

    fn put(&self, rel: &str, text: &str) -> PathBuf {
        let path = self.at(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
        path
    }

    fn worktree(&self) -> PathBuf {
        let registered = self.at("work/app/.git/worktrees/wt");
        std::fs::create_dir_all(&registered).unwrap();
        let wt = self.at("work/wt");
        std::fs::create_dir_all(&wt).unwrap();
        self.put(
            "work/wt/.git",
            &format!("gitdir: {}\n", registered.display()),
        );
        std::fs::write(registered.join("commondir"), "../..\n").unwrap();
        std::fs::write(
            registered.join("gitdir"),
            format!("{}\n", wt.join(".git").display()),
        )
        .unwrap();
        wt
    }

    fn memory_index(&self) {
        let slug = project_slug(&self.at("work/app"));
        self.put(
            &format!(".claude/projects/{slug}/memory/MEMORY.md"),
            "- a note\n",
        );
    }

    fn loads(&self, cwd: &std::path::Path) -> WhatLoads {
        let count = |text: &str| text.len() as u64;
        let loads = InstructionLoads {
            home: &self.base,
            claude_dir: &self.base.join(".claude"),
            policy_file: None,
            count_tokens: &count,
        };
        let mut report = loads.what_loads(cwd).unwrap();
        leave_out::mark(&mut report.items, &self.data, cwd);
        report
    }

    fn run(
        &self,
        cwd: &std::path::Path,
        op: Op,
        target: Target,
        dry_run: bool,
    ) -> serde_json::Value {
        let count = |text: &str| text.len() as u64;
        let loads = InstructionLoads {
            home: &self.base,
            claude_dir: &self.base.join(".claude"),
            policy_file: None,
            count_tokens: &count,
        };
        leave_out::run(
            &World {
                roots: &self.roots,
                loads: &loads,
                data_dir: &self.data,
            },
            &Args {
                op,
                target,
                cwd,
                dry_run,
            },
        )
        .unwrap()
    }
}

fn memory_row(report: &WhatLoads) -> &crate::context::loads::LoadItem {
    report
        .items
        .iter()
        .find(|i| i.kind == "memory-index")
        .expect("a memory-index row")
}

#[test]
fn a_linked_worktree_and_a_subfolder_find_the_memory_of_the_repository() {
    let fx = Fx::new();
    fx.memory_index();
    let wt = fx.worktree();
    for cwd in [fx.at("work/app"), fx.at("work/app/src/deep"), wt] {
        let report = fx.loads(&cwd);
        assert!(memory_row(&report).loaded, "{}", cwd.display());
    }
}

#[test]
fn leave_out_from_a_subfolder_writes_the_settings_at_the_repository_root() {
    let fx = Fx::new();
    fx.memory_index();
    let sub = fx.at("work/app/src/deep");
    fx.run(&sub, Op::Add, Target::Memory, false);
    let root_file = fx.at("work/app/.claude/settings.local.json");
    assert!(std::fs::read_to_string(&root_file)
        .unwrap()
        .contains("\"autoMemoryEnabled\": false"));
    assert!(!sub.join(".claude").exists());
    let report = fx.loads(&sub);
    let row = memory_row(&report);
    assert!(!row.loaded && row.reason == AUTO_MEMORY_OFF);
    assert_eq!(row.left_out_by, Some("toolport"));
    let at_root = fx.loads(&fx.at("work/app"));
    assert_eq!(memory_row(&at_root).left_out_by, Some("toolport"));
}

#[test]
fn leave_out_in_a_linked_worktree_writes_the_main_checkout_and_its_shared_exclude() {
    let fx = Fx::new();
    let wt = fx.worktree();
    let plan = fx.run(&wt, Op::Add, Target::Memory, true);
    let steps = plan["plan"]["steps"].to_string();
    assert!(steps.contains("work/app/.git/info/exclude"), "{steps}");
    assert!(!fx.at("work/app/.claude/settings.local.json").exists());

    fx.run(&wt, Op::Add, Target::Memory, false);
    assert!(fx.at("work/app/.claude/settings.local.json").exists());
    assert!(!wt.join(".claude").exists());
    let exclude = std::fs::read_to_string(fx.at("work/app/.git/info/exclude")).unwrap();
    assert!(exclude.lines().any(|l| l == ".claude/settings.local.json"));
    assert!(fx.at("work/wt/.git").is_file());

    fx.run(&wt, Op::Rm, Target::Memory, false);
    assert!(!fx.at("work/app/.claude/settings.local.json").exists());
}

#[test]
fn a_switch_written_in_a_subfolder_by_an_older_version_still_shows_and_moves_to_the_root() {
    let fx = Fx::new();
    fx.memory_index();
    let sub = fx.at("work/app/src");
    controls::apply(
        &fx.data,
        &sub,
        CONTROL,
        &Desired {
            memory_off: true,
            ..Desired::default()
        },
        false,
    )
    .unwrap();
    assert!(sub.join(".claude/settings.local.json").exists());
    assert!(leave_out::owned(&fx.data, &sub).1);

    let global = fx.put(".claude/CLAUDE.md", "global\n");
    fx.run(
        &sub,
        Op::Add,
        Target::File(global.display().to_string()),
        false,
    );
    assert!(!sub.join(".claude/settings.local.json").exists());
    let root_file = std::fs::read_to_string(fx.at("work/app/.claude/settings.local.json")).unwrap();
    assert!(root_file.contains("autoMemoryEnabled") && root_file.contains("claudeMdExcludes"));
    assert!(leave_out::owned(&fx.data, &sub).1);
}

#[test]
fn the_exclude_line_of_a_linked_worktree_goes_into_the_shared_file() {
    let fx = Fx::new();
    let wt = fx.worktree();
    assert!(ensure_exclude_line(&wt, "x").unwrap());
    assert!(!ensure_exclude_line(&wt, "x").unwrap());
    let shared = std::fs::read_to_string(fx.at("work/app/.git/info/exclude")).unwrap();
    assert_eq!(shared.trim(), "x");
}
