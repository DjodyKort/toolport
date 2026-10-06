#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::path::{Path, PathBuf};

use super::hashing::sha256_hex;

const APP_EXECUTABLES: [&str; 2] = ["conduit", "toolport"];
const SIBLINGS: [&str; 4] = [
    "toolport-gateway",
    "conduit-gateway",
    "toolportctl",
    "toolport-selfmcp",
];
const HELPER_BUNDLES: [(&str, &str); 2] = [
    ("ToolportGateway.app", "toolport-gateway"),
    ("ConduitGateway.app", "conduit-gateway"),
];
const FINGERPRINT_FILE: &str = ".keychain-acl-fingerprint";

fn exe_name(stem: &str) -> String {
    format!("{stem}{}", std::env::consts::EXE_SUFFIX)
}

/// Every binary that reads the vault master key, whichever of them is running: the app, the
/// gateway (nested helper or sibling), `toolportctl`, `toolport-selfmcp`, and the copies
/// published into the data dir. Symlinks resolve to their target and duplicates collapse, so
/// `~/.local/bin/toolportctl` and `Contents/MacOS/toolport-gateway` add nothing of their own.
pub(crate) fn trusted_binaries(
    exe: &Path,
    gateway: Option<&Path>,
    data_bin: Option<&Path>,
    extra_dirs: &[PathBuf],
) -> Vec<PathBuf> {
    let exe = exe.canonicalize().unwrap_or_else(|_| exe.to_path_buf());
    let mut candidates = vec![exe.clone()];
    if let Some(dir) = exe.parent() {
        for stem in APP_EXECUTABLES.iter().chain(SIBLINGS.iter()) {
            candidates.push(dir.join(exe_name(stem)));
        }
        for (bundle, binary) in HELPER_BUNDLES {
            candidates.push(
                dir.join("..")
                    .join("Helpers")
                    .join(bundle)
                    .join("Contents")
                    .join("MacOS")
                    .join(exe_name(binary)),
            );
        }
    }
    candidates.extend(gateway.map(Path::to_path_buf));
    for dir in data_bin
        .into_iter()
        .chain(extra_dirs.iter().map(PathBuf::as_path))
    {
        for stem in SIBLINGS {
            candidates.push(dir.join(exe_name(stem)));
        }
    }
    let mut found: Vec<PathBuf> = Vec::new();
    for candidate in candidates {
        let Ok(real) = candidate.canonicalize() else {
            continue;
        };
        if real.is_file() && !found.contains(&real) {
            found.push(real);
        }
    }
    found
}

/// Changes whenever a trusted binary is added, removed or its content changes. Content, not
/// mtime: republishing an unchanged binary must not look like a rebuild, because every
/// rewrite of the ACL discards an "Always Allow" the user just gave.
pub(crate) fn fingerprint(paths: &[PathBuf]) -> String {
    let mut lines: Vec<String> = paths
        .iter()
        .map(|path| {
            let digest = std::fs::read(path).map(sha256_hex).unwrap_or_default();
            format!("{}\t{digest}", path.display())
        })
        .collect();
    lines.sort();
    sha256_hex(lines.join("\n"))
}

pub(crate) fn needs_refresh(stored: Option<&str>, current: &str) -> bool {
    stored.map(str::trim) != Some(current)
}

/// Runs `rewrite` only when the trusted set differs from the stored fingerprint, and stores
/// the new fingerprint only after the rewrite succeeded. Returns whether it rewrote.
pub(crate) fn refresh_with<E>(
    stored: Option<&str>,
    current: &str,
    rewrite: impl FnOnce() -> Result<bool, E>,
    store: impl FnOnce(&str) -> Result<(), E>,
) -> Result<bool, E> {
    if !needs_refresh(stored, current) {
        return Ok(false);
    }
    if !rewrite()? {
        return Ok(false);
    }
    store(current)?;
    Ok(true)
}

pub(crate) fn stored_fingerprint() -> Option<String> {
    let dir = crate::registry::conduit_dir()?;
    std::fs::read_to_string(dir.join(FINGERPRINT_FILE)).ok()
}

pub(crate) fn store_fingerprint(value: &str) -> bool {
    let Some(dir) = crate::registry::conduit_dir() else {
        return false;
    };
    let _ = std::fs::create_dir_all(&dir);
    std::fs::write(dir.join(FINGERPRINT_FILE), value).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plus::testutil::DataDirFx;

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"bin").unwrap();
    }

    fn names(paths: &[PathBuf]) -> Vec<String> {
        let mut out: Vec<String> = paths
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        out.sort();
        out
    }

    #[test]
    fn lists_app_gateway_ctl_and_selfmcp() {
        let fx = DataDirFx::new("keychain-trust", "all");
        let macos = fx.dir.join("Toolport.app/Contents/MacOS");
        for name in ["conduit", "toolport-gateway", "toolportctl", "toolport-selfmcp"] {
            touch(&macos.join(name));
        }
        let found = trusted_binaries(&macos.join("conduit"), None, None, &[]);
        assert_eq!(
            names(&found),
            ["conduit", "toolport-gateway", "toolport-selfmcp", "toolportctl"]
        );
    }

    #[test]
    fn a_helper_as_the_caller_still_finds_the_app() {
        let fx = DataDirFx::new("keychain-trust", "caller");
        let macos = fx.dir.join("Toolport.app/Contents/MacOS");
        for name in ["conduit", "toolportctl", "toolport-selfmcp"] {
            touch(&macos.join(name));
        }
        let found = trusted_binaries(&macos.join("toolport-selfmcp"), None, None, &[]);
        assert_eq!(names(&found), ["conduit", "toolport-selfmcp", "toolportctl"]);
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_resolve_and_the_nested_gateway_counts_once() {
        let fx = DataDirFx::new("keychain-trust", "links");
        let contents = fx.dir.join("Toolport.app/Contents");
        let nested = contents.join("Helpers/ToolportGateway.app/Contents/MacOS/toolport-gateway");
        touch(&nested);
        touch(&contents.join("MacOS/conduit"));
        touch(&contents.join("MacOS/toolportctl"));
        std::os::unix::fs::symlink(&nested, contents.join("MacOS/toolport-gateway")).unwrap();
        let bin = fx.dir.join("home/.local/bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::os::unix::fs::symlink(contents.join("MacOS/toolportctl"), bin.join("toolportctl"))
            .unwrap();

        let from_link = trusted_binaries(&bin.join("toolportctl"), Some(&nested), None, &[]);
        assert_eq!(names(&from_link), ["conduit", "toolport-gateway", "toolportctl"]);
        assert_eq!(from_link.len(), 3);
    }

    #[test]
    fn published_copies_in_the_data_dir_are_trusted_too() {
        let fx = DataDirFx::new("keychain-trust", "published");
        let macos = fx.dir.join("Toolport.app/Contents/MacOS");
        touch(&macos.join("conduit"));
        let data_bin = fx.dir.join("data/bin");
        touch(&data_bin.join("toolport-selfmcp"));
        touch(&data_bin.join("toolport-gateway"));
        let found = trusted_binaries(&macos.join("conduit"), None, Some(&data_bin), &[]);
        assert_eq!(names(&found), ["conduit", "toolport-gateway", "toolport-selfmcp"]);
    }

    #[test]
    fn missing_binaries_are_skipped() {
        let fx = DataDirFx::new("keychain-trust", "missing");
        let macos = fx.dir.join("Toolport.app/Contents/MacOS");
        touch(&macos.join("conduit"));
        let ghost = fx.dir.join("nowhere/toolport-gateway");
        let found = trusted_binaries(&macos.join("conduit"), Some(&ghost), None, &[]);
        assert_eq!(names(&found), ["conduit"]);
    }

    #[test]
    fn fingerprint_tracks_the_set_and_each_rebuild() {
        let fx = DataDirFx::new("keychain-trust", "fingerprint");
        let a = fx.dir.join("a");
        let b = fx.dir.join("b");
        touch(&a);
        touch(&b);
        let both = vec![a.clone(), b.clone()];
        let reordered = vec![b.clone(), a.clone()];
        let first = fingerprint(&both);
        assert_eq!(first, fingerprint(&reordered));
        assert_ne!(first, fingerprint(&[a.clone()]));
        std::fs::write(&a, b"rebuilt, longer").unwrap();
        assert_ne!(first, fingerprint(&both));
    }

    #[test]
    fn fingerprint_ignores_a_republish_that_only_touches_mtime() {
        let fx = DataDirFx::new("keychain-trust", "mtime");
        let a = fx.dir.join("a");
        touch(&a);
        let before = fingerprint(&[a.clone()]);
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&a, b"bin").unwrap();
        assert_eq!(before, fingerprint(&[a]));
    }

    #[test]
    fn selfmcp_is_found_in_the_data_bin_layout_and_extra_dirs() {
        let fx = DataDirFx::new("keychain-trust", "layouts");
        let macos = fx.dir.join("Toolport.app/Contents/MacOS");
        touch(&macos.join("conduit"));
        touch(&macos.join("toolportctl"));
        let data_bin = fx.dir.join("data/bin");
        touch(&data_bin.join("toolport-selfmcp"));
        let found = trusted_binaries(&macos.join("conduit"), None, Some(&data_bin), &[]);
        assert!(names(&found).contains(&"toolport-selfmcp".to_string()));

        let local = fx.dir.join("home/.local/bin");
        touch(&local.join("toolport-selfmcp"));
        let found = trusted_binaries(&macos.join("conduit"), None, None, &[local]);
        assert_eq!(names(&found), ["conduit", "toolport-selfmcp", "toolportctl"]);
    }

    #[test]
    fn a_removed_binary_drops_out_of_the_set() {
        let fx = DataDirFx::new("keychain-trust", "stale");
        let macos = fx.dir.join("Toolport.app/Contents/MacOS");
        touch(&macos.join("conduit"));
        touch(&macos.join("toolportctl"));
        let exe = macos.join("conduit");
        assert_eq!(trusted_binaries(&exe, None, None, &[]).len(), 2);
        std::fs::remove_file(macos.join("toolportctl")).unwrap();
        assert_eq!(names(&trusted_binaries(&exe, None, None, &[])), ["conduit"]);
    }

    #[test]
    fn an_unchanged_fingerprint_performs_no_rewrite() {
        let mut rewrites = 0;
        let mut stored = None::<String>;
        for _ in 0..2 {
            let seen = stored.clone();
            refresh_with::<()>(
                seen.as_deref(),
                "fp1",
                || {
                    rewrites += 1;
                    Ok(true)
                },
                |v| {
                    stored = Some(v.to_string());
                    Ok(())
                },
            )
            .unwrap();
        }
        assert_eq!(rewrites, 1);
    }

    #[test]
    fn a_failed_rewrite_stores_nothing() {
        let mut stored = None::<String>;
        let result = refresh_with(None, "fp", || Err("locked"), |v| {
            stored = Some(v.to_string());
            Ok(())
        });
        assert_eq!(result, Err("locked"));
        assert_eq!(stored, None);
    }

    #[test]
    fn refresh_runs_until_the_fingerprint_is_stored() {
        let _fx = DataDirFx::new("keychain-trust", "marker");
        assert_eq!(stored_fingerprint(), None);
        assert!(needs_refresh(stored_fingerprint().as_deref(), "abc"));
        assert!(store_fingerprint("abc"));
        assert!(!needs_refresh(stored_fingerprint().as_deref(), "abc"));
        assert!(!needs_refresh(Some("abc\n"), "abc"));
        assert!(needs_refresh(stored_fingerprint().as_deref(), "abd"));
    }
}
