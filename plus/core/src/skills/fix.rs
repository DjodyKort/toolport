//! The one lint finding that has a safe one-click fix: a hook script of a library skill without
//! an executable bit. Nothing else a lint reports is rewritten for the user.

use super::lint::{folder_skills, non_executable_hooks};
use super::parser::Skill;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fixed {
    pub skill: String,
    /// Relative to the skill folder.
    pub file: String,
    pub event: String,
    pub from: String,
    pub to: String,
}

/// Mode with an execute bit wherever the file is already readable.
#[cfg(unix)]
fn executable_mode(mode: u32) -> u32 {
    mode | ((mode & 0o444) >> 2)
}

#[cfg(unix)]
pub fn chmod_hooks(skills: &[Skill], dry_run: bool) -> Result<Vec<Fixed>, String> {
    use std::os::unix::fs::PermissionsExt;
    let mut fixed = Vec::new();
    for skill in folder_skills(skills) {
        for (file, event) in non_executable_hooks(skill) {
            let path = skill.source_dir().join(&file);
            let mode = std::fs::metadata(&path)
                .map_err(|e| format!("{}: {e}", path.display()))?
                .permissions()
                .mode();
            let new = executable_mode(mode);
            if !dry_run {
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(new))
                    .map_err(|e| format!("{}: {e}", path.display()))?;
            }
            fixed.push(Fixed {
                skill: skill.name().to_string(),
                file,
                event,
                from: format!("{:04o}", mode & 0o7777),
                to: format!("{:04o}", new & 0o7777),
            });
        }
    }
    Ok(fixed)
}

#[cfg(not(unix))]
pub fn chmod_hooks(_skills: &[Skill], _dry_run: bool) -> Result<Vec<Fixed>, String> {
    Ok(Vec::new())
}
