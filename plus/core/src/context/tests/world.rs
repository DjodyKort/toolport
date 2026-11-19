use crate::context::fsx;
use crate::context::testkit::ScratchDir;
use std::fs;
use std::path::{Path, PathBuf};
use toolport_plus_api::{MemoryHost, MemoryRegistry, MemorySecrets};

/// A temp home with a data directory beside it; nothing here reads the real one.
pub struct World {
    pub _scratch: ScratchDir,
    pub host: MemoryHost,
    pub home: PathBuf,
    pub data: PathBuf,
    _paths: crate::fsx::PathsGuard,
}

pub fn world(tag: &str) -> World {
    let scratch = ScratchDir::new(tag);
    let dir = fsx::canonical(scratch.path());
    let (home, data) = (dir.join("home"), dir.join("data"));
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&data).unwrap();
    let host = MemoryHost {
        registry: MemoryRegistry::with_servers(vec![]),
        secrets: MemorySecrets {
            data_dir: Some(data.clone()),
            ..Default::default()
        },
        home: Some(home.clone()),
        ..Default::default()
    };
    host.now.set(1_800_000_000);
    let paths = crate::fsx::enter(crate::fsx::Paths {
        home: Some(home.clone()),
        data_dir: Some(data.clone()),
    });
    World {
        _scratch: scratch,
        host,
        home,
        data,
        _paths: paths,
    }
}

impl World {
    pub fn write(&self, rel: &str, text: &str) -> PathBuf {
        let path = self.home.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        path
    }

    pub fn read(&self, rel: &str) -> String {
        fs::read_to_string(self.home.join(rel)).unwrap()
    }

    pub fn exists(&self, rel: &str) -> bool {
        self.home.join(rel).exists()
    }

    /// A rule layer of the skills repository.
    pub fn layer(&self, name: &str, front: &str, body: &str) -> PathBuf {
        self.write(
            &format!(".config/mcpm/skills_repo/rules/{name}/SKILL.md"),
            &format!("---\nname: {name}\ndescription: \"A layer for the tests\"\nactivation: always\n{front}\n---\n{body}\n"),
        )
    }

    pub fn clients_root(&self, rel: &str) {
        self.write(
            ".config/mcpm/context.json",
            &format!("{{\"clients_root\": \"~/{rel}\"}}\n"),
        );
    }

    pub fn tree(&self) -> std::collections::BTreeMap<String, Option<Vec<u8>>> {
        crate::context::testkit::tree_snapshot(self.home.parent().unwrap())
    }
}

#[cfg(unix)]
pub fn script(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}
