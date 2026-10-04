//! Output styles: STYLE.md parsing, tier-1 sync, tier-2 apply/remove, lint.

pub mod handlers;
pub mod lint;
pub mod manage;
pub mod sync;
pub mod transpilers;

pub use sync::{apply_style, remove_style, sync_styles, StyleOptions};
pub use transpilers::{all_style_transpilers, StyleTranspiler, Tier};

use super::kind::{discover, discover_report, read_document, ContentKind};
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

pub struct StyleKind;

impl ContentKind for StyleKind {
    type Item = Style;
    const NOUN: &'static str = "Style";
    const DIRS: &'static [&'static str] = &["styles"];
    const FILE: &'static str = "STYLE.md";

    fn parse(path: &Path) -> Result<Style, String> {
        parse_style_file(path)
    }
}

pub fn parse_style_file(path: &Path) -> Result<Style, String> {
    let (fm_data, body) = read_document(StyleKind::NOUN, path)?;
    Ok(Style {
        frontmatter: build_frontmatter(&fm_data)?,
        body,
        source_path: path.to_path_buf(),
    })
}

pub fn discover_styles_report(repo: &Path) -> (Vec<Style>, Vec<String>) {
    discover_report::<StyleKind>(repo)
}

pub fn discover_styles(repo: &Path) -> Vec<Style> {
    discover::<StyleKind>(repo)
}
