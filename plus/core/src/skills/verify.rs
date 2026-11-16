//! Compares what the library would install with what is installed, file by file. A sync that
//! left nothing behind and held nothing back reports no finding.

use super::lock::{get_entry, LockFile};
use super::parser::{Skill, SkillType};
use super::sync::{compare, desired_for, Found};
use super::transpiler::TranspilerRegistry;
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
}

impl Verify {
    pub fn clean(&self) -> bool {
        self.missing.is_empty() && self.differing.is_empty()
    }
}

/// Only the clients the lock says each skill was synced to are checked.
pub fn verify_installed(
    skills: &[Skill],
    lock: &LockFile,
    registry: &TranspilerRegistry,
    root: &Path,
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
                    Found::Missing => out.missing.push(finding()),
                    Found::Different => out.differing.push(finding()),
                }
            }
        }
    }
    out
}
