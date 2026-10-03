//! Output styles: STYLE.md parsing, tier-1 sync, tier-2 apply/remove, lint.

pub mod lint;
pub mod sync;
pub mod transpilers;

pub use sync::{apply_style, remove_style, sync_styles, StyleOptions};
pub use transpilers::{all_style_transpilers, StyleTranspiler, Tier};

use super::parser::parse_frontmatter;
use super::schema::{self, Fm};
use serde_yaml::Mapping;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct StyleFrontmatter {
    pub name: String,
    pub description: String,
    pub keep_coding_instructions: bool,
    pub metadata: Mapping,
}

#[derive(Clone, Debug)]
pub struct Style {
    pub frontmatter: StyleFrontmatter,
    pub body: String,
    pub source_path: PathBuf,
}

impl Style {
    pub fn name(&self) -> &str {
        &self.frontmatter.name
    }

    pub fn version(&self) -> Option<String> {
        schema::version_of(&self.frontmatter.metadata)
    }

    /// Stand-in used only to ask a transpiler for an output path.
    pub fn placeholder(name: &str) -> Style {
        Style {
            frontmatter: StyleFrontmatter {
                name: name.to_string(),
                description: "dummy".into(),
                keep_coding_instructions: true,
                metadata: Mapping::new(),
            },
            body: String::new(),
            source_path: PathBuf::from("dummy"),
        }
    }
}

fn build_frontmatter(fm: &Fm) -> Result<StyleFrontmatter, String> {
    let (name, description) = schema::name_and_description(fm)?;
    Ok(StyleFrontmatter {
        name,
        description,
        keep_coding_instructions: schema::lax_bool(fm, "keep_coding_instructions", true)?,
        metadata: schema::metadata(fm)?,
    })
}

pub fn parse_style_file(path: &Path) -> Result<Style, String> {
    if !path.exists() {
        return Err(format!("Style file not found: {}", path.display()));
    }
    let content = super::pyfs::read_text(path)?;
    let (fm_data, body) = parse_frontmatter(&content)?;
    if fm_data.is_empty() {
        return Err(format!("No YAML frontmatter found in {}", path.display()));
    }
    Ok(Style {
        frontmatter: build_frontmatter(&fm_data)?,
        body,
        source_path: path.to_path_buf(),
    })
}

pub fn discover_styles_report(repo: &Path) -> (Vec<Style>, Vec<String>) {
    let mut warnings = Vec::new();
    let mut styles = Vec::new();
    let dir = repo.join("styles");
    if !dir.is_dir() {
        return (styles, warnings);
    }
    let Ok(read) = std::fs::read_dir(&dir) else {
        return (styles, warnings);
    };
    let mut children: Vec<PathBuf> = read.filter_map(|e| e.ok().map(|e| e.path())).collect();
    children.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
    for style_dir in children {
        if !style_dir.is_dir() {
            continue;
        }
        let file = style_dir.join("STYLE.md");
        let dir_name = style_dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if !file.exists() {
            warnings.push(format!(
                "Style directory {dir_name} has no STYLE.md, skipping"
            ));
            continue;
        }
        match parse_style_file(&file) {
            Ok(s) => styles.push(s),
            Err(e) => warnings.push(format!("Failed to parse {}: {e}", file.display())),
        }
    }
    (styles, warnings)
}

pub fn discover_styles(repo: &Path) -> Vec<Style> {
    discover_styles_report(repo).0
}
