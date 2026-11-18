//! Servers that start from a package a runner downloads (`npx`, `uvx`, `pipx`, `docker run`):
//! which package and version the arguments pin, and whether a newer one exists. The version a
//! runner picks for an unpinned package lives in its own cache, which Toolport does not read, so
//! only an exact pin is judged.

pub mod lookup;
pub mod version;

#[cfg(test)]
mod tests;

use std::path::Path;

use toolport_plus_api::ServerRecord;

const REMOTE_TRANSPORTS: [&str; 3] = ["http", "sse", "streamable-http"];

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Npm,
    Pypi,
    Docker,
}

impl Kind {
    pub fn word(self) -> &'static str {
        match self {
            Kind::Npm => "npm",
            Kind::Pypi => "pypi",
            Kind::Docker => "docker",
        }
    }
}

/// What one lookup asks the registry. `flavor` narrows a container image's tags to the ones
/// shaped like the pinned tag (`1.2.3-alpine` is compared with other `x.y.z-alpine` tags only).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Target {
    pub kind: Kind,
    pub name: String,
    pub flavor: String,
}

impl Target {
    /// The name the lookup is recorded under in the update check.
    pub fn source(&self) -> String {
        if self.flavor.is_empty() {
            format!("package:{}:{}", self.kind.word(), self.name)
        } else {
            format!("package:{}:{}:{}", self.kind.word(), self.name, self.flavor)
        }
    }
}

/// A server and the package version its arguments pin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pinned {
    pub server_id: String,
    pub server_name: String,
    pub target: Target,
    pub version: String,
}

const NPM_RUNNERS: [&str; 3] = ["npx", "bunx", "pnpx"];
const NPM_VALUE_FLAGS: [&str; 6] = [
    "--registry",
    "--cache",
    "-c",
    "--call",
    "--userconfig",
    "--prefix",
];
const PYPI_VALUE_FLAGS: [&str; 14] = [
    "--with",
    "-w",
    "--with-requirements",
    "--python",
    "-p",
    "--index",
    "--index-url",
    "-i",
    "--extra-index-url",
    "--directory",
    "--project",
    "--env-file",
    "--python-preference",
    "--constraint",
];
const DOCKER_VALUE_FLAGS: [&str; 26] = [
    "-e",
    "--env",
    "--env-file",
    "-v",
    "--volume",
    "--mount",
    "-p",
    "--publish",
    "--name",
    "-w",
    "--workdir",
    "-u",
    "--user",
    "--network",
    "--net",
    "--entrypoint",
    "--platform",
    "--add-host",
    "-l",
    "--label",
    "--memory",
    "-m",
    "--cpus",
    "--pull",
    "--restart",
    "--hostname",
];

fn program(command: &str) -> String {
    let name = Path::new(command)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(command)
        .to_ascii_lowercase();
    for suffix in [".cmd", ".exe", ".bat"] {
        if let Some(stem) = name.strip_suffix(suffix) {
            return stem.to_string();
        }
    }
    name
}

/// The arguments after the runner's own words, and which registry the package comes from.
fn runner<'a>(command: &str, args: &'a [String]) -> Option<(Kind, &'a [String])> {
    let program = program(command);
    let first = args.first().map(String::as_str);
    match (program.as_str(), first) {
        (name, _) if NPM_RUNNERS.contains(&name) => Some((Kind::Npm, args)),
        ("pnpm" | "yarn", Some("dlx")) => Some((Kind::Npm, &args[1..])),
        ("uvx", _) => Some((Kind::Pypi, args)),
        ("uv", Some("tool")) if args.get(1).map(String::as_str) == Some("run") => {
            Some((Kind::Pypi, &args[2..]))
        }
        ("pipx", Some("run")) => Some((Kind::Pypi, &args[1..])),
        ("docker" | "podman", Some("run")) => Some((Kind::Docker, &args[1..])),
        _ => None,
    }
}

/// The package argument of an `npx` or `uvx` style call: `--from`, `--package` and `-p` name
/// it explicitly, otherwise it is the first argument that is not an option.
fn package_argument(kind: Kind, args: &[String]) -> Option<String> {
    let value_flags: &[&str] = if kind == Kind::Npm {
        &NPM_VALUE_FLAGS
    } else {
        &PYPI_VALUE_FLAGS
    };
    let named: &[&str] = if kind == Kind::Npm {
        &["--package", "-p"]
    } else {
        &["--from"]
    };
    let mut positional = None;
    let mut index = 0;
    while let Some(arg) = args.get(index) {
        index += 1;
        if arg == "--" {
            positional = positional.or_else(|| args.get(index).cloned());
            break;
        }
        if let Some((flag, value)) = arg.split_once('=').filter(|_| arg.starts_with('-')) {
            if named.contains(&flag) {
                return Some(value.to_string());
            }
            continue;
        }
        if named.contains(&arg.as_str()) {
            return args.get(index).cloned();
        }
        if value_flags.contains(&arg.as_str()) {
            index += 1;
            continue;
        }
        if arg.starts_with('-') {
            continue;
        }
        positional = Some(arg.clone());
        break;
    }
    positional
}

fn valid_name(name: &str, scoped: bool) -> bool {
    let body = if scoped {
        name.strip_prefix('@').unwrap_or(name)
    } else {
        name
    };
    !body.is_empty()
        && body
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/'))
        && body
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric())
}

fn npm_pin(spec: &str) -> Option<(String, String)> {
    let at = spec.rfind('@').filter(|at| *at > 0)?;
    let (name, version) = (&spec[..at], &spec[at + 1..]);
    let full = version::numbers(version).is_some_and(|numbers| numbers.len() >= 3);
    (valid_name(name, true) && full && version::exact(version))
        .then(|| (name.into(), version.into()))
}

fn pypi_pin(spec: &str) -> Option<(String, String)> {
    let (name, version) = spec.split_once("==").or_else(|| spec.split_once('@'))?;
    let name = name.split('[').next().unwrap_or(name);
    (valid_name(name, false) && version::exact(version)).then(|| (name.into(), version.into()))
}

/// The Docker Hub repository of an image reference and its tag; `None` for another registry,
/// an image pinned by digest, or one with no tag or `latest`.
fn docker_pin(image: &str) -> Option<(String, String, String)> {
    if image.contains('@') {
        return None;
    }
    let image = ["docker.io/", "index.docker.io/", "registry-1.docker.io/"]
        .iter()
        .find_map(|prefix| image.strip_prefix(prefix))
        .unwrap_or(image);
    let (path, tag) = image
        .rsplit_once(':')
        .filter(|(_, tag)| !tag.contains('/'))?;
    let first = path.split('/').next().unwrap_or("");
    if path.contains('/') && (first.contains('.') || first.contains(':') || first == "localhost") {
        return None;
    }
    if !valid_name(path, false) || path.matches('/').count() > 1 {
        return None;
    }
    let repository = if path.contains('/') {
        path.to_string()
    } else {
        format!("library/{path}")
    };
    let flavor = version::tag_flavor(tag)?;
    Some((repository, tag.to_string(), flavor))
}

fn docker_image(args: &[String]) -> Option<&str> {
    let mut index = 0;
    while let Some(arg) = args.get(index) {
        index += 1;
        if DOCKER_VALUE_FLAGS.contains(&arg.as_str()) {
            index += 1;
        } else if !arg.starts_with('-') {
            return Some(arg);
        }
    }
    None
}

fn recorded_docker(server: &ServerRecord) -> Option<String> {
    let source = server.fork.mcpm_source.as_ref()?;
    if source.kind.as_deref() != Some("docker") {
        return None;
    }
    let image = source.extra.get("image")?.as_str()?;
    let tag = source.extra.get("tag")?.as_str()?;
    Some(format!("{image}:{tag}"))
}

/// The package version a stdio server's arguments pin, nothing when it is not pinned exactly.
pub fn pinned(server: &ServerRecord) -> Option<Pinned> {
    let transport = server.text("transport").unwrap_or("stdio");
    if REMOTE_TRANSPORTS.contains(&transport) {
        return None;
    }
    let command = server.text("command")?;
    let args = server.strings("args");
    let (kind, rest) = runner(command, &args)
        .map(|(kind, rest)| (Some(kind), rest))
        .unwrap_or((None, &[]));
    let (target, version) = match kind {
        Some(Kind::Npm) => {
            let (name, version) = npm_pin(&package_argument(Kind::Npm, rest)?)?;
            (named(Kind::Npm, name, String::new()), version)
        }
        Some(Kind::Pypi) => {
            let (name, version) = pypi_pin(&package_argument(Kind::Pypi, rest)?)?;
            (named(Kind::Pypi, name, String::new()), version)
        }
        Some(Kind::Docker) => {
            docker_target(&recorded_docker(server).or(docker_image(rest).map(String::from))?)?
        }
        None => docker_target(&recorded_docker(server)?)?,
    };
    Some(Pinned {
        server_id: server.id.clone(),
        server_name: server.name.clone(),
        target,
        version,
    })
}

fn named(kind: Kind, name: String, flavor: String) -> Target {
    Target { kind, name, flavor }
}

fn docker_target(image: &str) -> Option<(Target, String)> {
    let (repository, tag, flavor) = docker_pin(image)?;
    Some((named(Kind::Docker, repository, flavor), tag))
}

/// Every enabled server whose arguments pin a package version, by server name.
pub fn pins(servers: &[ServerRecord]) -> Vec<Pinned> {
    let mut found: Vec<Pinned> = servers
        .iter()
        .filter(|server| server.enabled)
        .filter_map(pinned)
        .collect();
    found.sort_by(|a, b| a.server_name.cmp(&b.server_name));
    found
}

/// What one pass has to ask, once per package however many servers pin it.
pub fn targets(pins: &[Pinned]) -> Vec<Target> {
    let mut targets: Vec<Target> = pins.iter().map(|pin| pin.target.clone()).collect();
    targets.sort();
    targets.dedup();
    targets
}
