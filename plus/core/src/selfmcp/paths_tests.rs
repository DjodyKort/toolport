use std::path::Path;

use tempfile::TempDir;

use super::paths::{not_root_or_home, under_home};
use crate::fsx::{enter, Paths, PathsGuard};

fn at_home(home: &Path) -> PathsGuard {
    enter(Paths {
        home: Some(home.to_path_buf()),
        data_dir: None,
    })
}

fn kind<T: std::fmt::Debug>(result: Result<T, super::ToolError>) -> &'static str {
    result.unwrap_err().kind
}

#[test]
fn test01_a_folder_that_does_not_exist_yet_resolves_inside_the_home() {
    let dir = TempDir::new().unwrap();
    let _guard = at_home(dir.path());
    let path = under_home("~/new/skills").unwrap();
    assert!(path.ends_with("new/skills"));
    assert!(path.starts_with(std::fs::canonicalize(dir.path()).unwrap()));
}

#[test]
fn test02_the_home_directory_itself_and_anything_outside_it_is_refused() {
    let dir = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    let _guard = at_home(dir.path());
    assert_eq!(kind(under_home("~")), "refused");
    assert_eq!(kind(under_home(&dir.path().to_string_lossy())), "refused");
    assert_eq!(
        kind(under_home(&outside.path().to_string_lossy())),
        "refused"
    );
    assert_eq!(kind(under_home("/")), "refused");
}

#[test]
fn test03_parent_segments_and_relative_paths_are_invalid() {
    let dir = TempDir::new().unwrap();
    let _guard = at_home(dir.path());
    assert_eq!(kind(under_home("~/a/../../b")), "invalid_arguments");
    assert_eq!(kind(under_home("relative/dir")), "invalid_arguments");
    assert_eq!(kind(under_home("  ")), "invalid_arguments");
}

#[cfg(unix)]
#[test]
fn test04_a_symlink_that_leaves_the_home_does_not_count_as_inside() {
    let dir = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    std::os::unix::fs::symlink(outside.path(), dir.path().join("link")).unwrap();
    let _guard = at_home(dir.path());
    assert_eq!(kind(under_home("~/link/repo")), "refused");
}

#[test]
fn test05_a_source_root_may_be_anywhere_except_the_filesystem_root_and_the_home() {
    let dir = TempDir::new().unwrap();
    let _guard = at_home(dir.path());
    assert_eq!(kind(not_root_or_home("/")), "refused");
    assert_eq!(kind(not_root_or_home("~")), "refused");
    assert_eq!(
        kind(not_root_or_home(&dir.path().to_string_lossy())),
        "refused"
    );
    assert!(not_root_or_home("~/code").is_ok());
    assert!(not_root_or_home("/srv/repos").is_ok());
}
