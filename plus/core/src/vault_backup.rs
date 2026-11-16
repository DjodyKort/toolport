//! Rolling backup of the file vault.
//!
//! Before `secrets.enc` is replaced, its current ciphertext is kept as
//! `secrets.enc.bak`, one generation deep. The copy is never decrypted and
//! nothing restores it automatically: recovering is copying it back by hand.

use std::path::{Path, PathBuf};

const BACKUP_SUFFIX: &str = ".bak";

pub fn backup_path(vault: &Path) -> PathBuf {
    let mut name = vault.as_os_str().to_owned();
    name.push(BACKUP_SUFFIX);
    PathBuf::from(name)
}

/// Stores the vault's current ciphertext at [`backup_path`] through `write`, which
/// must replace the file atomically and keep it owner-only.
///
/// A missing or blank vault has nothing to keep. Any other failure is an error
/// and the caller must not replace the vault: a replace without its backup would
/// leave no way back from a bad write. Call it only after the current vault was
/// opened successfully, so the copy is always a generation that decrypts.
pub fn keep_previous(
    vault: &Path,
    write: impl FnOnce(&Path, &str) -> Result<(), String>,
) -> Result<(), String> {
    let previous = match std::fs::read_to_string(vault) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(unchanged(format!("cannot read it ({error})"))),
    };
    if previous.trim().is_empty() {
        return Ok(());
    }
    write(&backup_path(vault), &previous)
        .map_err(|error| unchanged(format!("cannot write secrets.enc.bak ({error})")))
}

fn unchanged(reason: String) -> String {
    format!("secrets.enc was not backed up, so it was left unchanged: {reason}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stands in for upstream's atomic write; mode and atomicity are asserted
    /// against the real one in conduit's seam tests.
    fn plain_write(path: &Path, contents: &str) -> Result<(), String> {
        std::fs::write(path, contents).map_err(|error| error.to_string())
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "toolport-plus-vault-backup-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn test01_backup_path_appends_bak_to_the_vault_name() {
        assert_eq!(
            backup_path(Path::new("/data/secrets.enc")),
            PathBuf::from("/data/secrets.enc.bak")
        );
    }

    #[test]
    fn test02_a_missing_vault_has_nothing_to_keep() {
        let dir = temp_dir("missing");
        let vault = dir.join("secrets.enc");
        let mut called = false;
        keep_previous(&vault, |_, _| {
            called = true;
            Ok(())
        })
        .unwrap();
        assert!(!called);
        assert!(!backup_path(&vault).exists());
    }

    #[test]
    fn test03_a_blank_vault_is_not_rolled_over_a_good_backup() {
        let dir = temp_dir("blank");
        let vault = dir.join("secrets.enc");
        std::fs::write(backup_path(&vault), "good-generation").unwrap();
        std::fs::write(&vault, "  \n").unwrap();
        keep_previous(&vault, plain_write).unwrap();
        assert_eq!(
            std::fs::read_to_string(backup_path(&vault)).unwrap(),
            "good-generation"
        );
    }

    #[test]
    fn test04_the_backup_holds_the_previous_ciphertext_and_rolls_forward() {
        let dir = temp_dir("rolling");
        let vault = dir.join("secrets.enc");
        std::fs::write(&vault, "ciphertext-one").unwrap();
        keep_previous(&vault, plain_write).unwrap();
        assert_eq!(
            std::fs::read_to_string(backup_path(&vault)).unwrap(),
            "ciphertext-one"
        );
        std::fs::write(&vault, "ciphertext-two").unwrap();
        keep_previous(&vault, plain_write).unwrap();
        assert_eq!(
            std::fs::read_to_string(backup_path(&vault)).unwrap(),
            "ciphertext-two"
        );
    }

    #[test]
    fn test05_a_failed_backup_write_is_an_error_naming_the_backup() {
        let dir = temp_dir("failing");
        let vault = dir.join("secrets.enc");
        std::fs::write(&vault, "ciphertext-one").unwrap();
        let error = keep_previous(&vault, |_, _| Err("disk full".into())).unwrap_err();
        assert!(error.contains("secrets.enc.bak"), "{error}");
        assert!(error.contains("disk full"), "{error}");
        assert!(error.contains("left unchanged"), "{error}");
        assert_eq!(std::fs::read_to_string(&vault).unwrap(), "ciphertext-one");
    }

    #[test]
    fn test06_an_unreadable_vault_is_an_error() {
        let dir = temp_dir("unreadable");
        let vault = dir.join("secrets.enc");
        std::fs::create_dir(&vault).unwrap();
        let error = keep_previous(&vault, plain_write).unwrap_err();
        assert!(error.contains("cannot read it"), "{error}");
        assert!(!backup_path(&vault).exists());
    }
}
