//! Whether a running Claude Code session still holds an older plugin state than the files say:
//! Claude Code reads plugins and their settings when a session starts, so a plugin file that
//! changed after the oldest running session started has not reached that session.

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use crate::context::fsx;
use crate::skills::gitexec::run_command;

const PS_TIMEOUT: Duration = Duration::from_secs(5);

/// `[[dd-]hh:]mm:ss`, the elapsed-time column of `ps`.
fn parse_elapsed(text: &str) -> Option<i64> {
    let (days, clock) = match text.split_once('-') {
        Some((d, rest)) => (d.parse::<i64>().ok()?, rest),
        None => (0, text),
    };
    let mut seconds = 0;
    for part in clock.split(':') {
        seconds = seconds * 60 + part.parse::<i64>().ok()?;
    }
    Some(days * 86_400 + seconds)
}

/// The start times (unix seconds) of the processes named `claude`; empty when `ps` cannot say.
pub fn running_starts(now: i64) -> Vec<i64> {
    let mut cmd = Command::new("ps");
    cmd.args(["-axo", "etime=,comm="]);
    let Ok(out) = run_command(cmd, PS_TIMEOUT) else {
        return Vec::new();
    };
    out.stdout
        .lines()
        .filter_map(|line| {
            let (elapsed, comm) = line.trim().split_once(char::is_whitespace)?;
            let name = comm.trim().rsplit('/').next()?;
            (name == "claude").then(|| parse_elapsed(elapsed).map(|age| now - age))?
        })
        .collect()
}

/// A session older than the newest change to one of `files`.
pub fn restart_required(files: &[&Path], starts: &[i64]) -> bool {
    let changed = files.iter().filter_map(|f| fsx::mtime_secs(f)).max();
    match (changed, starts.iter().min()) {
        (Some(changed), Some(oldest)) => *oldest < changed,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test01_elapsed_time_is_read_in_every_ps_form() {
        assert_eq!(parse_elapsed("05:09"), Some(309));
        assert_eq!(parse_elapsed("02:05:09"), Some(7509));
        assert_eq!(parse_elapsed("3-02:05:09"), Some(3 * 86_400 + 7509));
        assert_eq!(parse_elapsed("soon"), None);
    }

    #[test]
    fn test02_only_a_session_older_than_the_change_needs_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("installed_plugins.json");
        std::fs::write(&file, "{}").unwrap();
        let changed = fsx::mtime_secs(&file).unwrap();
        assert!(restart_required(&[&file], &[changed - 100, changed + 100]));
        assert!(!restart_required(&[&file], &[changed + 100]));
        assert!(!restart_required(&[&file], &[]));
        assert!(!restart_required(
            &[&dir.path().join("none")],
            &[changed - 100]
        ));
    }
}
