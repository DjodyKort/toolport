//! What a layer says about where and how it is delivered (MIG-CTX-11): `scope`, `folders`,
//! `imports`, `delivery` in the frontmatter of `rules/<layer>/SKILL.md`. A layer without these keys
//! is a path-scoped (or always-on) rule delivered by copy, exactly as before. Imports resolve to
//! files or to other layers; a copy inlines them (and the `@path` lines inside them, up to
//! [`MAX_HOPS`] hops), an import writes `@path` lines. A cycle or a chain that is too long is
//! reported and not followed.

use super::bundle::Issue;
use super::layers::{body_of, Layer};
use crate::plus::sources::fsx;
use serde_yaml::{Mapping, Value as Yaml};
use std::path::{Path, PathBuf};

/// Claude Code follows `@imports` five hops deep; Toolport stops at four so the chain it reports
/// is one it can show in full.
pub const MAX_HOPS: usize = 4;
const MAX_FILES: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Global,
    Glob,
    Folder,
}

impl Scope {
    pub fn as_str(self) -> &'static str {
        match self {
            Scope::Global => "global",
            Scope::Glob => "glob",
            Scope::Folder => "folder",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "global" => Some(Scope::Global),
            "glob" => Some(Scope::Glob),
            "folder" => Some(Scope::Folder),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delivery {
    Import,
    Copy,
}

impl Delivery {
    pub fn as_str(self) -> &'static str {
        match self {
            Delivery::Import => "import",
            Delivery::Copy => "copy",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "import" => Some(Delivery::Import),
            "copy" => Some(Delivery::Copy),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Spec {
    pub scope: Scope,
    pub folders: Vec<String>,
    pub imports: Vec<String>,
    pub delivery: Delivery,
    pub issues: Vec<Issue>,
}

impl Default for Spec {
    fn default() -> Self {
        Spec {
            scope: Scope::Glob,
            folders: Vec::new(),
            imports: Vec::new(),
            delivery: Delivery::Copy,
            issues: Vec::new(),
        }
    }
}

pub fn issue(level: &'static str, key: &str, message: impl Into<String>) -> Issue {
    Issue {
        level,
        key: key.to_string(),
        message: message.into(),
    }
}

fn strings(fm: &Mapping, key: &str, issues: &mut Vec<Issue>) -> Vec<String> {
    let text = |v: &Yaml| match v {
        Yaml::String(s) => s.trim().to_string(),
        other => super::layers::yaml_text(other),
    };
    match fm.get(Yaml::String(key.into())) {
        None | Some(Yaml::Null) => Vec::new(),
        Some(Yaml::Sequence(items)) => items.iter().map(text).filter(|s| !s.is_empty()).collect(),
        Some(Yaml::String(s)) if !s.trim().is_empty() => vec![s.trim().to_string()],
        Some(Yaml::String(_)) => Vec::new(),
        Some(_) => {
            issues.push(issue("error", key, "must be a list of strings"));
            Vec::new()
        }
    }
}

pub fn spec_of(fm: &Mapping) -> Spec {
    let mut out = Spec::default();
    let get = |key: &str| fm.get(Yaml::String(key.into()));
    match get("scope") {
        None | Some(Yaml::Null) => {}
        Some(Yaml::String(s)) => match Scope::parse(s) {
            Some(scope) => out.scope = scope,
            None => out
                .issues
                .push(issue("error", "scope", format!("{s:?} is not global, glob or folder; glob is used"))),
        },
        Some(_) => out.issues.push(issue("error", "scope", "must be global, glob or folder")),
    }
    match get("delivery") {
        None | Some(Yaml::Null) => {}
        Some(Yaml::String(s)) => match Delivery::parse(s) {
            Some(delivery) => out.delivery = delivery,
            None => out
                .issues
                .push(issue("error", "delivery", format!("{s:?} is not import or copy; copy is used"))),
        },
        Some(_) => out.issues.push(issue("error", "delivery", "must be import or copy")),
    }
    let mut issues = Vec::new();
    out.folders = strings(fm, "folders", &mut issues);
    out.imports = strings(fm, "imports", &mut issues);
    out.issues.extend(issues);
    out
}

pub fn expand_user(home: &Path, raw: &str) -> PathBuf {
    if raw == "~" {
        return home.to_path_buf();
    }
    match raw.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None => PathBuf::from(raw),
    }
}

/// `~/rel` for a file under the home folder, so the delivered text does not depend on where the
/// home folder is.
fn shown(home: &Path, path: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rel) if !rel.as_os_str().is_empty() => format!("~/{}", rel.display()),
        _ => path.display().to_string(),
    }
}

/// An import that names a layer rather than a file: no separator, no `~`, no extension.
fn is_layer_ref(spec: &str) -> bool {
    !spec.contains('/') && !spec.starts_with('~') && !spec.starts_with('.') && !spec.ends_with(".md")
}

fn layer_named<'a>(layers: &'a [Layer], name: &str) -> Option<&'a Layer> {
    layers.iter().find(|l| l.name == name)
}

/// Checks that need no file: the values themselves.
pub fn lint_static(home: &Path, layer: &Layer) -> Vec<Issue> {
    let spec = &layer.spec;
    let mut out = spec.issues.clone();
    if spec.scope == Scope::Folder && spec.folders.is_empty() {
        out.push(issue("error", "folders", "scope folder needs at least one folder"));
    }
    if spec.scope != Scope::Folder && !spec.folders.is_empty() {
        out.push(issue("warning", "folders", "folders is only used when scope is folder"));
    }
    for folder in &spec.folders {
        let path = expand_user(home, folder);
        if !path.is_absolute() {
            out.push(issue("error", "folders", format!("{folder}: must be an absolute folder (or start with ~/)")));
        }
    }
    let mut seen: Vec<&String> = Vec::new();
    for import in &spec.imports {
        if seen.contains(&import) {
            out.push(issue("warning", "imports", format!("{import}: listed twice")));
        }
        seen.push(import);
    }
    if spec.delivery == Delivery::Import && spec.imports.is_empty() {
        out.push(issue("warning", "delivery", "delivery import has nothing to import"));
    }
    out
}

pub struct Delivered {
    pub text: String,
    pub issues: Vec<Issue>,
    pub files: Vec<String>,
}

struct Walk<'a> {
    home: &'a Path,
    layers: &'a [Layer],
    issues: Vec<Issue>,
    files: Vec<String>,
    budget: usize,
}

fn import_token(line: &str) -> Option<&str> {
    let rest = line.trim().strip_prefix('@')?;
    let pathish = rest.contains('/') || rest.contains('.') || rest.starts_with('~');
    (!rest.is_empty() && pathish && !rest.contains(char::is_whitespace)).then_some(rest)
}

fn chain_text(chain: &[PathBuf], last: &Path) -> String {
    chain
        .iter()
        .map(|p| p.display().to_string())
        .chain(std::iter::once(last.display().to_string()))
        .collect::<Vec<_>>()
        .join(" -> ")
}

impl Walk<'_> {
    fn resolve(&self, base: &Path, token: &str) -> PathBuf {
        let expanded = expand_user(self.home, token);
        if expanded.is_absolute() {
            expanded
        } else {
            base.join(expanded)
        }
    }

    /// The text of `path` with the `@path` lines inside it replaced by what they point at. `hops`
    /// counts the imports between the layer and this file, the layer's own import being the first.
    fn file_text(&mut self, path: &Path, chain: &mut Vec<PathBuf>, hops: usize) -> Option<String> {
        if self.budget == 0 {
            self.issues.push(issue("warning", "imports", format!("stopped after {MAX_FILES} imported files")));
            return None;
        }
        self.budget -= 1;
        let text = fsx::read_text(path, fsx::TEXT_CAP)?;
        self.files.push(fsx::display(path));
        chain.push(fsx::canonical(path));
        let base = path.parent().unwrap_or(path).to_path_buf();
        let mut out = String::new();
        let mut fenced = false;
        for line in text.lines() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
                fenced = !fenced;
            }
            let replaced = if fenced { None } else { import_token(line).map(|t| self.nested(&base, t, chain, hops)) };
            match replaced {
                Some(Some(inlined)) => out.push_str(inlined.trim_end()),
                _ => out.push_str(line),
            }
            out.push('\n');
        }
        chain.pop();
        Some(out)
    }

    fn nested(&mut self, base: &Path, token: &str, chain: &mut Vec<PathBuf>, hops: usize) -> Option<String> {
        let target = self.resolve(base, token);
        if !fsx::is_file(&target) {
            return None;
        }
        let canonical = fsx::canonical(&target);
        if chain.contains(&canonical) {
            self.issues.push(issue(
                "warning",
                "imports",
                format!("import cycle {} (not followed)", chain_text(chain, &canonical)),
            ));
            return None;
        }
        if hops + 1 > MAX_HOPS {
            self.issues.push(issue(
                "warning",
                "imports",
                format!("{}: @{token} is more than {MAX_HOPS} hops deep (not followed)", chain_text(chain, &canonical)),
            ));
            return None;
        }
        self.file_text(&target, chain, hops + 1)
    }

    fn layer_text(&mut self, layer: &Layer, via: &mut Vec<String>) -> String {
        via.push(layer.name.clone());
        let mut text = body_of(&layer.path).trim().to_string();
        for spec in &layer.spec.imports {
            if let Some(part) = self.import(&layer.path, spec, via) {
                text.push_str("\n\n");
                text.push_str(&part);
            }
        }
        via.pop();
        text
    }

    fn import(&mut self, owner: &Path, spec: &str, via: &mut Vec<String>) -> Option<String> {
        if is_layer_ref(spec) {
            let Some(layer) = layer_named(self.layers, spec) else {
                self.issues.push(issue("warning", "imports", format!("{spec}: no layer or file of that name")));
                return None;
            };
            if via.contains(&layer.name) {
                self.issues.push(issue(
                    "warning",
                    "imports",
                    format!("import cycle {} -> {} (not followed)", via.join(" -> "), layer.name),
                ));
                return None;
            }
            if via.len() > MAX_HOPS {
                self.issues.push(issue(
                    "warning",
                    "imports",
                    format!("{spec}: more than {MAX_HOPS} hops deep (not followed)"),
                ));
                return None;
            }
            let text = self.layer_text(layer, via);
            return Some(format!("<!-- imported from layer {} -->\n{text}", layer.name));
        }
        let target = self.resolve(owner.parent().unwrap_or(owner), spec);
        if !fsx::is_file(&target) {
            self.issues.push(issue("warning", "imports", format!("{spec}: {} does not exist", target.display())));
            return None;
        }
        let inlined = self.file_text(&target, &mut Vec::new(), 1)?;
        Some(format!("<!-- imported from {} -->\n{}", shown(self.home, &target), inlined.trim_end()))
    }
}

/// The text of the layer as it is delivered, the layer's own imports resolved: by copy the imported
/// text sits in the result, by import the files stay `@path` lines. Everything that is wrong with
/// the imports comes back as issues and nothing is followed past a cycle or the hop limit.
pub fn deliver(home: &Path, layers: &[Layer], layer: &Layer) -> Delivered {
    let mut walk = Walk { home, layers, issues: Vec::new(), files: Vec::new(), budget: MAX_FILES };
    let copied = walk.layer_text(layer, &mut Vec::new());
    let text = match layer.spec.delivery {
        Delivery::Copy => copied,
        Delivery::Import => {
            let mut text = body_of(&layer.path).trim().to_string();
            for spec in &layer.spec.imports {
                if is_layer_ref(spec) {
                    let mut again = Walk { home, layers, issues: Vec::new(), files: Vec::new(), budget: MAX_FILES };
                    if let Some(part) = again.import(&layer.path, spec, &mut vec![layer.name.clone()]) {
                        text.push_str(&format!("\n\n{part}"));
                    }
                } else {
                    let target = walk.resolve(layer.path.parent().unwrap_or(&layer.path), spec);
                    if fsx::is_file(&target) {
                        text.push_str(&format!("\n\n@{}", shown(home, &target)));
                    }
                }
            }
            text
        }
    };
    Delivered { text, issues: walk.issues, files: walk.files }
}

/// Lint with the files: what `deliver` reports plus the warning that an import outside the folder
/// Claude starts in is skipped by a headless session.
pub fn lint(home: &Path, layers: &[Layer], layer: &Layer) -> Vec<Issue> {
    let mut out = lint_static(home, layer);
    if layer.spec.imports.is_empty() {
        return out;
    }
    out.extend(deliver(home, layers, layer).issues);
    if layer.spec.delivery == Delivery::Import {
        out.push(issue(
            "warning",
            "delivery",
            "an import outside the folder Claude starts in is skipped by a headless session and may ask for approval once in an interactive one; copy is safe everywhere",
        ));
    }
    out
}

/// The body a rule file is deployed with: the layer's own text, imports resolved. `None` when the
/// rule has no `imports`, so every rule that is not a layer with imports deploys as it did.
pub fn rule_body(home: &Path, source: &Path) -> Option<String> {
    if spec_of(&super::layers::frontmatter(source)).imports.is_empty() {
        return None;
    }
    let rules_dir = source.parent()?.parent()?;
    let layers = super::layers::list_layers_in(rules_dir);
    let layer = layers.iter().find(|l| l.path == source)?;
    Some(deliver(home, &layers, layer).text)
}

/// True for a rule whose scope is `folder`: it is delivered into its folders and never as a rule
/// of the user's own Claude folder.
pub fn is_folder_rule(source: &Path) -> bool {
    let fm = super::layers::frontmatter(source);
    matches!(fm.get(Yaml::String("scope".into())), Some(Yaml::String(s)) if s == "folder")
}

/// `text` with the frontmatter keys of `edits` replaced, added or (value `None`) removed, every
/// other line kept as it is.
pub fn rewrite_frontmatter(text: &str, edits: &[(&str, Option<String>)]) -> Option<String> {
    let (yaml, after) = super::layers::split_fenced(text)?;
    let mut seen = vec![false; edits.len()];
    let mut out: Vec<String> = Vec::new();
    let mut skipping = false;
    for line in yaml.lines() {
        let key = (!line.starts_with([' ', '\t', '-', '#'])).then(|| line.split(':').next().unwrap_or("").trim());
        if let Some(i) = key.and_then(|k| edits.iter().position(|(name, _)| *name == k)) {
            seen[i] = true;
            skipping = true;
            if let (name, Some(value)) = &edits[i] {
                out.push(format!("{name}: {value}"));
            }
            continue;
        }
        if skipping && (line.starts_with([' ', '\t']) || line.starts_with("- ")) {
            continue;
        }
        skipping = false;
        out.push(line.to_string());
    }
    for (i, (name, value)) in edits.iter().enumerate() {
        if let (false, Some(value)) = (seen[i], value) {
            out.push(format!("{name}: {value}"));
        }
    }
    Some(format!("---{}\n---{after}", out.join("\n")))
}

/// A YAML flow list of double-quoted strings.
pub fn flow_list(items: &[String]) -> String {
    let quoted: Vec<String> = items
        .iter()
        .map(|i| format!("\"{}\"", i.replace('\\', "\\\\").replace('"', "\\\"")))
        .collect();
    format!("[{}]", quoted.join(", "))
}

/// The delivery settings a command or a scaffold wants to set; `None` leaves the key as it is, an
/// empty list removes it.
#[derive(Clone, Debug, Default)]
pub struct SpecEdit {
    pub glob: Option<String>,
    pub scope: Option<Scope>,
    pub folders: Option<Vec<String>>,
    pub imports: Option<Vec<String>>,
    pub delivery: Option<Delivery>,
}

impl SpecEdit {
    pub fn is_empty(&self) -> bool {
        self.glob.is_none()
            && self.scope.is_none()
            && self.folders.is_none()
            && self.imports.is_none()
            && self.delivery.is_none()
    }

    pub fn edits(&self) -> Vec<(&'static str, Option<String>)> {
        let list = |items: &Vec<String>| (!items.is_empty()).then(|| flow_list(items));
        let mut out = Vec::new();
        if let Some(glob) = &self.glob {
            out.push(("globs", Some(format!("\"{}\"", glob.replace('\\', "\\\\").replace('"', "\\\"")))));
        }
        if let Some(scope) = self.scope {
            out.push(("scope", (scope != Scope::Glob).then(|| scope.as_str().to_string())));
        }
        if let Some(folders) = &self.folders {
            out.push(("folders", list(folders)));
        }
        if let Some(imports) = &self.imports {
            out.push(("imports", list(imports)));
        }
        if let Some(delivery) = self.delivery {
            out.push(("delivery", (delivery != Delivery::Copy).then(|| delivery.as_str().to_string())));
        }
        out
    }
}
