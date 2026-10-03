use std::path::{Path, PathBuf};

/// Recursively collects regular files under `dir` that satisfy `keep`; symlinks are not followed.
pub(crate) fn collect_files(dir: &Path, keep: &dyn Fn(&Path) -> bool, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        match entry.file_type() {
            Ok(t) if t.is_dir() => collect_files(&path, keep, out),
            Ok(t) if t.is_file() && keep(&path) => out.push(path),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walks_nested_directories_and_filters_by_predicate() {
        let root = std::env::temp_dir().join(format!("fswalk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("a/b")).unwrap();
        std::fs::write(root.join("top.py"), "").unwrap();
        std::fs::write(root.join("a/b/deep.py"), "").unwrap();
        std::fs::write(root.join("a/skip.txt"), "").unwrap();

        let mut py = Vec::new();
        collect_files(
            &root,
            &|p| p.extension().is_some_and(|e| e == "py"),
            &mut py,
        );
        py.sort();
        assert_eq!(py, vec![root.join("a/b/deep.py"), root.join("top.py")]);

        let mut all = Vec::new();
        collect_files(&root, &|_| true, &mut all);
        assert_eq!(all.len(), 3);

        let mut none = Vec::new();
        collect_files(&root.join("missing"), &|_| true, &mut none);
        assert!(none.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }
}
