//! Which Toolport binaries the vault master key trusts, and when that list
//! has to be written again.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

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

fn sha256_hex(bytes: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Every binary that reads the vault master key, whichever of them is running: the app, the
/// gateway (nested helper or sibling), `toolportctl`, `toolport-selfmcp`, and the copies
/// published into the data dir. Symlinks resolve to their target and duplicates collapse, so
/// `~/.local/bin/toolportctl` and `Contents/MacOS/toolport-gateway` add nothing of their own.
pub fn trusted_binaries(
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

/// The trusted binaries that upstream's own list (the running binary and the gateway) lacks.
pub fn beyond_app_and_gateway(
    exe: &Path,
    gateway: Option<&Path>,
    data_bin: Option<&Path>,
    extra_dirs: &[PathBuf],
) -> Vec<PathBuf> {
    let known: Vec<PathBuf> = std::iter::once(exe)
        .chain(gateway)
        .filter_map(|path| path.canonicalize().ok())
        .collect();
    trusted_binaries(exe, gateway, data_bin, extra_dirs)
        .into_iter()
        .filter(|path| !known.contains(path))
        .collect()
}

/// Changes whenever a trusted binary is added, removed or its content changes. Content, not
/// mtime: republishing an unchanged binary must not look like a rebuild, because every
/// rewrite of the ACL discards an "Always Allow" the user just gave.
pub fn fingerprint(paths: &[PathBuf]) -> String {
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

pub fn needs_refresh(stored: Option<&str>, current: &str) -> bool {
    stored.map(str::trim) != Some(current)
}

/// Runs `rewrite` only when the trusted set differs from the stored fingerprint, and stores
/// the new fingerprint only after the rewrite succeeded. Returns whether it rewrote.
pub fn refresh_with<E>(
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

pub fn stored_fingerprint(data_dir: &Path) -> Option<String> {
    std::fs::read_to_string(data_dir.join(FINGERPRINT_FILE)).ok()
}

pub fn store_fingerprint(data_dir: &Path, value: &str) -> bool {
    let _ = std::fs::create_dir_all(data_dir);
    std::fs::write(data_dir.join(FINGERPRINT_FILE), value).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "toolport-plus-trust-{name}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            // canonical, so assertions hold where the temp dir is itself a symlink
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir.canonicalize().unwrap())
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

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
        let fx = Scratch::new("all");
        let macos = fx.0.join("Toolport.app/Contents/MacOS");
        for name in [
            "conduit",
            "toolport-gateway",
            "toolportctl",
            "toolport-selfmcp",
        ] {
            touch(&macos.join(name));
        }
        let found = trusted_binaries(&macos.join("conduit"), None, None, &[]);
        assert_eq!(
            names(&found),
            [
                "conduit",
                "toolport-gateway",
                "toolport-selfmcp",
                "toolportctl"
            ]
        );
    }

    #[test]
    fn a_helper_as_the_caller_still_finds_the_app() {
        let fx = Scratch::new("caller");
        let macos = fx.0.join("Toolport.app/Contents/MacOS");
        for name in ["conduit", "toolportctl", "toolport-selfmcp"] {
            touch(&macos.join(name));
        }
        let found = trusted_binaries(&macos.join("toolport-selfmcp"), None, None, &[]);
        assert_eq!(
            names(&found),
            ["conduit", "toolport-selfmcp", "toolportctl"]
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_resolve_and_the_nested_gateway_counts_once() {
        let fx = Scratch::new("links");
        let contents = fx.0.join("Toolport.app/Contents");
        let nested = contents.join("Helpers/ToolportGateway.app/Contents/MacOS/toolport-gateway");
        touch(&nested);
        touch(&contents.join("MacOS/conduit"));
        touch(&contents.join("MacOS/toolportctl"));
        std::os::unix::fs::symlink(&nested, contents.join("MacOS/toolport-gateway")).unwrap();
        let bin = fx.0.join("home/.local/bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::os::unix::fs::symlink(contents.join("MacOS/toolportctl"), bin.join("toolportctl"))
            .unwrap();

        let from_link = trusted_binaries(&bin.join("toolportctl"), Some(&nested), None, &[]);
        assert_eq!(
            names(&from_link),
            ["conduit", "toolport-gateway", "toolportctl"]
        );
        assert_eq!(from_link.len(), 3);
    }

    #[test]
    fn published_copies_in_the_data_dir_are_trusted_too() {
        let fx = Scratch::new("published");
        let macos = fx.0.join("Toolport.app/Contents/MacOS");
        touch(&macos.join("conduit"));
        let data_bin = fx.0.join("data/bin");
        touch(&data_bin.join("toolport-selfmcp"));
        touch(&data_bin.join("toolport-gateway"));
        let found = trusted_binaries(&macos.join("conduit"), None, Some(&data_bin), &[]);
        assert_eq!(
            names(&found),
            ["conduit", "toolport-gateway", "toolport-selfmcp"]
        );
    }

    #[test]
    fn missing_binaries_are_skipped() {
        let fx = Scratch::new("missing");
        let macos = fx.0.join("Toolport.app/Contents/MacOS");
        touch(&macos.join("conduit"));
        let ghost = fx.0.join("nowhere/toolport-gateway");
        let found = trusted_binaries(&macos.join("conduit"), Some(&ghost), None, &[]);
        assert_eq!(names(&found), ["conduit"]);
    }

    #[test]
    fn selfmcp_is_found_in_the_data_bin_layout_and_extra_dirs() {
        let fx = Scratch::new("layouts");
        let macos = fx.0.join("Toolport.app/Contents/MacOS");
        touch(&macos.join("conduit"));
        touch(&macos.join("toolportctl"));
        let data_bin = fx.0.join("data/bin");
        touch(&data_bin.join("toolport-selfmcp"));
        let found = trusted_binaries(&macos.join("conduit"), None, Some(&data_bin), &[]);
        assert!(names(&found).contains(&"toolport-selfmcp".to_string()));

        let local = fx.0.join("home/.local/bin");
        touch(&local.join("toolport-selfmcp"));
        let found = trusted_binaries(&macos.join("conduit"), None, None, &[local]);
        assert_eq!(
            names(&found),
            ["conduit", "toolport-selfmcp", "toolportctl"]
        );
    }

    #[test]
    fn a_removed_binary_drops_out_of_the_set() {
        let fx = Scratch::new("stale");
        let macos = fx.0.join("Toolport.app/Contents/MacOS");
        touch(&macos.join("conduit"));
        touch(&macos.join("toolportctl"));
        let exe = macos.join("conduit");
        assert_eq!(trusted_binaries(&exe, None, None, &[]).len(), 2);
        std::fs::remove_file(macos.join("toolportctl")).unwrap();
        assert_eq!(names(&trusted_binaries(&exe, None, None, &[])), ["conduit"]);
    }

    #[test]
    fn the_extra_list_leaves_out_the_app_and_the_gateway() {
        let fx = Scratch::new("beyond");
        let macos = fx.0.join("Toolport.app/Contents/MacOS");
        for name in [
            "conduit",
            "toolport-gateway",
            "toolportctl",
            "toolport-selfmcp",
        ] {
            touch(&macos.join(name));
        }
        let gateway = macos.join("toolport-gateway");
        let extra = beyond_app_and_gateway(&macos.join("conduit"), Some(&gateway), None, &[]);
        assert_eq!(names(&extra), ["toolport-selfmcp", "toolportctl"]);

        let from_ctl =
            beyond_app_and_gateway(&macos.join("toolportctl"), Some(&gateway), None, &[]);
        assert_eq!(names(&from_ctl), ["conduit", "toolport-selfmcp"]);
    }

    #[test]
    fn fingerprint_tracks_the_set_and_each_rebuild() {
        let fx = Scratch::new("fingerprint");
        let a = fx.0.join("a");
        let b = fx.0.join("b");
        touch(&a);
        touch(&b);
        let both = vec![a.clone(), b.clone()];
        let reordered = vec![b.clone(), a.clone()];
        let first = fingerprint(&both);
        assert_eq!(first, fingerprint(&reordered));
        assert_ne!(first, fingerprint(std::slice::from_ref(&a)));
        std::fs::write(&a, b"rebuilt, longer").unwrap();
        assert_ne!(first, fingerprint(&both));
    }

    #[test]
    fn fingerprint_ignores_a_republish_that_only_touches_mtime() {
        let fx = Scratch::new("mtime");
        let a = fx.0.join("a");
        touch(&a);
        let before = fingerprint(std::slice::from_ref(&a));
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&a, b"bin").unwrap();
        assert_eq!(before, fingerprint(&[a]));
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
        let result = refresh_with(
            None,
            "fp",
            || Err("locked"),
            |v| {
                stored = Some(v.to_string());
                Ok(())
            },
        );
        assert_eq!(result, Err("locked"));
        assert_eq!(stored, None);
    }

    #[test]
    fn refresh_runs_until_the_fingerprint_is_stored() {
        let fx = Scratch::new("marker");
        assert_eq!(stored_fingerprint(&fx.0), None);
        assert!(needs_refresh(stored_fingerprint(&fx.0).as_deref(), "abc"));
        assert!(store_fingerprint(&fx.0, "abc"));
        assert!(!needs_refresh(stored_fingerprint(&fx.0).as_deref(), "abc"));
        assert!(!needs_refresh(Some("abc\n"), "abc"));
        assert!(needs_refresh(stored_fingerprint(&fx.0).as_deref(), "abd"));
    }
}
