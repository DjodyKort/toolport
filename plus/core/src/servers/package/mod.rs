//! Servers that start from a package a runner downloads (`npx`, `uvx`, `pipx`, `docker run`) or
//! from a GitHub release: which package and version the arguments or the source pin, whether a
//! newer one exists, and how a pin is rewritten. The version a runner picks for an unpinned
//! package lives in its own cache, which Toolport does not read, so only an exact pin is judged.

pub mod journal;
pub mod lookup;
pub mod release;
pub mod update;
pub mod version;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_release;
#[cfg(test)]
mod tests_update;

use std::path::Path;

use toolport_plus_api::ServerRecord;

const REMOTE_TRANSPORTS: [&str; 3] = ["http", "sse", "streamable-http"];

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Npm,
    Pypi,
    Docker,
    /// A binary or archive installed from a GitHub release; the name is `owner/repo`.
    Github,
}

impl Kind {
    pub fn word(self) -> &'static str {
        match self {
            Kind::Npm => "npm",
            Kind::Pypi => "pypi",
            Kind::Docker => "docker",
            Kind::Github => "github",
        }
    }

    pub fn from_word(word: &str) -> Option<Kind> {
        [Kind::Npm, Kind::Pypi, Kind::Docker, Kind::Github]
            .into_iter()
            .find(|kind| kind.word() == word)
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
    /// Why Toolport cannot rewrite this pin itself; the update has to be made by hand.
    pub manual: Option<String>,
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

/// The arguments after the runner's own words (as an offset into the argument list), and which
/// registry the package comes from.
fn runner(command: &str, args: &[String]) -> Option<(Kind, usize)> {
    let program = program(command);
    let first = args.first().map(String::as_str);
    match (program.as_str(), first) {
        (name, _) if NPM_RUNNERS.contains(&name) => Some((Kind::Npm, 0)),
        ("pnpm" | "yarn", Some("dlx")) => Some((Kind::Npm, 1)),
        ("uvx", _) => Some((Kind::Pypi, 0)),
        ("uv", Some("tool")) if args.get(1).map(String::as_str) == Some("run") => {
            Some((Kind::Pypi, 2))
        }
        ("pipx", Some("run")) => Some((Kind::Pypi, 1)),
        ("docker" | "podman", Some("run")) => Some((Kind::Docker, 1)),
        _ => None,
    }
}

/// Where the package argument of an `npx` or `uvx` style call sits: `--from`, `--package` and
/// `-p` name it explicitly, otherwise it is the first argument that is not an option. `prefix` is
/// what precedes the package text inside that argument (`--package=`).
fn package_spot(kind: Kind, args: &[String]) -> Option<Spot> {
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
    let at = |index: usize| Spot {
        index,
        prefix: String::new(),
    };
    let mut positional = None;
    let mut index = 0;
    while let Some(arg) = args.get(index) {
        index += 1;
        if arg == "--" {
            positional = positional.or_else(|| args.get(index).map(|_| at(index)));
            break;
        }
        if let Some((flag, _)) = arg.split_once('=').filter(|_| arg.starts_with('-')) {
            if named.contains(&flag) {
                return Some(Spot {
                    index: index - 1,
                    prefix: format!("{flag}="),
                });
            }
            continue;
        }
        if named.contains(&arg.as_str()) {
            return args.get(index).map(|_| at(index));
        }
        if value_flags.contains(&arg.as_str()) {
            index += 1;
            continue;
        }
        if arg.starts_with('-') {
            continue;
        }
        positional = Some(at(index - 1));
        break;
    }
    positional
}

fn docker_image(args: &[String]) -> Option<usize> {
    let mut index = 0;
    while let Some(arg) = args.get(index) {
        index += 1;
        if DOCKER_VALUE_FLAGS.contains(&arg.as_str()) {
            index += 1;
        } else if !arg.starts_with('-') {
            return Some(index - 1);
        }
    }
    None
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

pub(crate) fn recorded_docker(server: &ServerRecord) -> Option<String> {
    let source = server.fork.mcpm_source.as_ref()?;
    if source.kind.as_deref() != Some("docker") {
        return None;
    }
    let image = source.extra.get("image")?.as_str()?;
    let tag = source.extra.get("tag")?.as_str()?;
    Some(format!("{image}:{tag}"))
}

fn named(kind: Kind, name: String, flavor: String) -> Target {
    Target { kind, name, flavor }
}

fn docker_target(image: &str) -> Option<(Target, String)> {
    let (repository, tag, flavor) = docker_pin(image)?;
    Some((named(Kind::Docker, repository, flavor), tag))
}

/// Where a pin sits in the server's arguments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spot {
    /// Index into the server's argument list.
    pub index: usize,
    /// What precedes the package text inside that argument (`--package=`); empty when the
    /// argument is the package text.
    pub prefix: String,
}

/// A pin together with where it was read from, so it can be rewritten.
struct Found {
    pin: Pinned,
    /// `None` when the pin is not in the arguments: a GitHub release, or an image only the
    /// server's source records.
    spot: Option<Spot>,
}

fn locate(server: &ServerRecord) -> Option<Found> {
    let transport = server.text("transport").unwrap_or("stdio");
    if REMOTE_TRANSPORTS.contains(&transport) {
        return None;
    }
    let command = server.text("command")?;
    let args = server.strings("args");
    let (kind, offset) = runner(command, &args)
        .map(|(kind, offset)| (Some(kind), offset))
        .unwrap_or((None, 0));
    let rest = &args[offset.min(args.len())..];
    let shift = |spot: Spot| Spot {
        index: spot.index + offset,
        prefix: spot.prefix,
    };
    let (target, version, spot) = match kind {
        Some(Kind::Npm) => {
            let spot = package_spot(Kind::Npm, rest)?;
            let (name, version) = npm_pin(&rest[spot.index][spot.prefix.len()..])?;
            (
                named(Kind::Npm, name, String::new()),
                version,
                Some(shift(spot)),
            )
        }
        Some(Kind::Pypi) => {
            let spot = package_spot(Kind::Pypi, rest)?;
            let (name, version) = pypi_pin(&rest[spot.index][spot.prefix.len()..])?;
            (
                named(Kind::Pypi, name, String::new()),
                version,
                Some(shift(spot)),
            )
        }
        Some(Kind::Docker) => {
            let in_args = docker_image(rest).map(|index| index + offset);
            let image = recorded_docker(server).or(in_args.map(|index| args[index].clone()))?;
            let (target, version) = docker_target(&image)?;
            let spot = in_args.map(|index| Spot {
                index,
                prefix: String::new(),
            });
            (target, version, spot)
        }
        Some(Kind::Github) => return None,
        None => {
            let (target, version) = docker_target(&recorded_docker(server)?)?;
            (target, version, None)
        }
    };
    Some(Found {
        pin: Pinned {
            server_id: server.id.clone(),
            server_name: server.name.clone(),
            target,
            version,
            manual: None,
        },
        spot,
    })
}

/// The package version a server's arguments or its GitHub release source pin, nothing when it is
/// not pinned exactly.
pub fn pinned(server: &ServerRecord) -> Option<Pinned> {
    if let Some(pin) = release::pinned(server) {
        return Some(pin);
    }
    let mut found = locate(server)?;
    found.pin.manual = rewrite_in(&found, server, &found.pin.version).err();
    Some(found.pin)
}

/// One argument of a server changing from `before` to `after`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rewrite {
    pub index: usize,
    pub before: String,
    pub after: String,
}

fn swap_version(spec: &str, kind: Kind, to: &str) -> Option<String> {
    match kind {
        Kind::Npm => spec.rfind('@').map(|at| format!("{}@{to}", &spec[..at])),
        Kind::Pypi => match (spec.find("=="), spec.find('@')) {
            (Some(at), _) => Some(format!("{}=={to}", &spec[..at])),
            (None, Some(at)) => Some(format!("{}@{to}", &spec[..at])),
            _ => None,
        },
        Kind::Docker => spec.rfind(':').map(|at| format!("{}:{to}", &spec[..at])),
        Kind::Github => None,
    }
}

fn rewrite_in(found: &Found, server: &ServerRecord, to: &str) -> Result<Rewrite, String> {
    let pin = &found.pin;
    if server
        .extra
        .get("launch")
        .is_some_and(|launch| !launch.is_null())
    {
        return Err(
            "its arguments come from a catalog launch recipe, and editing them detaches it".into(),
        );
    }
    let spot = found.spot.as_ref().ok_or_else(|| {
        format!(
            "the {} pin is recorded in the server's source, not in its command, so Toolport cannot tell what the command reads",
            pin.target.kind.word()
        )
    })?;
    let mut args = server.strings("args");
    let before = args[spot.index].clone();
    let spec = &before[spot.prefix.len()..];
    if pin.target.kind == Kind::Docker
        && docker_target(spec).as_ref() != Some(&(pin.target.clone(), pin.version.clone()))
    {
        return Err("the image in the command is not the one the server's source records".into());
    }
    let after = swap_version(spec, pin.target.kind, to)
        .map(|spec| format!("{}{spec}", spot.prefix))
        .ok_or_else(|| "the pin could not be located in the argument".to_string())?;
    args[spot.index] = after.clone();
    let mut changed = server.clone();
    changed.extra.insert("args".into(), serde_json::json!(args));
    if let Some(source) = changed.fork.mcpm_source.as_mut() {
        if recorded_docker(server).is_some() {
            source.extra.insert("tag".into(), to.into());
        }
    }
    match locate(&changed) {
        Some(read) if read.pin.target == pin.target && read.pin.version == to => Ok(Rewrite {
            index: spot.index,
            before,
            after,
        }),
        _ => Err(format!(
            "{to} would not read back as a pin of {}",
            pin.target.name
        )),
    }
}

/// The argument of `server` that pins its package, with `to` in place of the pinned version.
/// `Err` says why Toolport cannot rewrite it: `to` is not a version the pin can hold, or the pin
/// is not in the arguments.
pub fn rewrite(server: &ServerRecord, to: &str) -> Result<Rewrite, String> {
    let found = locate(server).ok_or_else(|| "the server pins no package".to_string())?;
    rewrite_in(&found, server, to)
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
