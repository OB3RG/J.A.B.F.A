use std::path::PathBuf;

use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub heading: Option<String>,
    pub heading_path: Vec<String>,
    pub level: u8,
    pub start_line: usize,
    pub end_line: usize,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    Markdown,
    Wiki,
    Embed,
}

impl LinkKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Markdown => "markdown",
            Self::Wiki => "wiki",
            Self::Embed => "embed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub raw_target: String,
    pub kind: LinkKind,
    pub line_number: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub text: String,
    pub completed: bool,
    pub line_number: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedDocument {
    pub path: PathBuf,
    pub content_hash: String,
    pub frontmatter: Option<Value>,
    pub tags: Vec<String>,
    pub tasks: Vec<Task>,
    pub links: Vec<Link>,
    pub sections: Vec<Section>,
}
