//! Compares what the library would install with what is installed, file by file. A sync that
//! left nothing behind and held nothing back reports no finding.

use super::lock::{get_entry, LockFile};
use super::parser::{Skill, SkillType};
use super::sync::{compare, desired_for, installed_hash, Found};
use super::transpiler::TranspilerRegistry;
use serde_json::{json, Value};
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub skill: String,
    pub client: String,
    pub path: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Verify {
    pub checked: usize,
    pub missing: Vec<Finding>,
    pub differing: Vec<Finding>,
    /// Same content, other executable bits than the library copy.
    pub mode: Vec<Finding>,
    /// Differing files that also no longer match the hash the lock recorded when the sync
    /// installed them: changed by hand since.
    pub edited: Vec<Finding>,
}

impl Verify {
    pub fn clean(&self) -> bool {
        self.missing.is_empty() && self.differing.is_empty() && self.mode.is_empty()
    }

    /// `matched` of `total` files, and every file that is not as the library has it.
    pub fn summary(&self) -> Value {
        let rows = |findings: &[Finding], reason: &'static str| {
            findings
                .iter()
                .map(|f| json!({"skill": f.skill, "client": f.client, "path": f.path, "reason": reason}))
                .collect::<Vec<_>>()
        };
        let mut mismatched = rows(&self.missing, "missing");
        mismatched.extend(rows(&self.differing, "differs"));
        mismatched.extend(rows(&self.mode, "mode"));
        json!({
            "matched": self.checked - mismatched.len(),
            "total": self.checked,
            "clean": self.clean(),
            "mismatched": mismatched,
        })
    }
}

/// Only the clients the lock says each skill was synced to are checked.
pub fn verify_installed(
    skills: &[Skill],
    lock: &LockFile,
    registry: &TranspilerRegistry,
    root: &Path,
) -> Verify {
    verify_clients(skills, lock, registry, root, None)
}

/// [`verify_installed`] for the clients in `only`, when given.
pub fn verify_clients(
    skills: &[Skill],
    lock: &LockFile,
    registry: &TranspilerRegistry,
    root: &Path,
    only: Option<&[String]>,
) -> Verify {
    let mut out = Verify::default();
    for skill in skills {
        let map = match skill.skill_type {
            SkillType::Rule => &lock.rules,
            SkillType::Skill => &lock.skills,
        };
        let Some(entry) = get_entry(map, skill.name()) else {
            continue;
        };
        for client in &entry.clients_synced {
            if only.is_some_and(|clients| !clients.contains(client)) {
                continue;
            }
            let Some(t) = registry.get(client) else {
                continue;
            };
            if t.capabilities().append_mode {
                continue;
            }
            let Ok(layout) = desired_for(t, skill, root) else {
                continue;
            };
            for file in layout.files {
                out.checked += 1;
                let finding = || Finding {
                    skill: skill.name().to_string(),
                    client: client.clone(),
                    path: file.rel.clone(),
                };
                match compare(&file) {
                    Found::Same => {}
                    Found::Mode => out.mode.push(finding()),
                    Found::Missing => out.missing.push(finding()),
                    Found::Different => {
                        out.differing.push(finding());
                        let edited = entry
                            .file_hash(client, &file.rel)
                            .is_some_and(|hash| installed_hash(&file.abs).as_deref() != Some(hash));
                        if edited {
                            out.edited.push(finding());
                        }
                    }
                }
            }
        }
    }
    out
}
