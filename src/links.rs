use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use crate::model::{Link, LinkKind, ParsedDocument};

pub(crate) struct ResolvedLink {
    pub target: Option<PathBuf>,
    pub target_heading: Option<String>,
    pub resolution: &'static str,
}

pub(crate) struct LinkResolver {
    by_path: HashMap<String, PathBuf>,
    by_stem: HashMap<String, Vec<PathBuf>>,
    by_alias: HashMap<String, Vec<PathBuf>>,
}

impl LinkResolver {
    pub fn new(documents: &[ParsedDocument]) -> Self {
        let mut resolver = Self {
            by_path: HashMap::new(),
            by_stem: HashMap::new(),
            by_alias: HashMap::new(),
        };

        for document in documents {
            let key = path_key(&document.path);
            resolver.by_path.insert(key, document.path.clone());
            if let Some(stem) = document.path.file_stem().and_then(|stem| stem.to_str()) {
                resolver
                    .by_stem
                    .entry(stem.to_owned())
                    .or_default()
                    .push(document.path.clone());
            }
            if let Some(aliases) = document
                .frontmatter
                .as_ref()
                .and_then(|frontmatter| frontmatter.get("aliases"))
                .and_then(|aliases| aliases.as_array())
            {
                for alias in aliases.iter().filter_map(|alias| alias.as_str()) {
                    resolver
                        .by_alias
                        .entry(alias.to_owned())
                        .or_default()
                        .push(document.path.clone());
                }
            }
        }

        resolver
    }

    pub fn resolve(&self, source: &Path, link: &Link) -> ResolvedLink {
        let (raw_target, target_heading) = link
            .raw_target
            .split_once('#')
            .map_or((link.raw_target.as_str(), None), |(target, heading)| {
                (target, Some(heading.to_owned()))
            });
        let raw_target = raw_target.trim();

        if is_external(raw_target) {
            return ResolvedLink {
                target: None,
                target_heading,
                resolution: "external",
            };
        }

        if raw_target.is_empty() {
            return ResolvedLink {
                target: Some(source.to_path_buf()),
                target_heading,
                resolution: "resolved",
            };
        }

        let (candidates, broken_resolution) = match link.kind {
            LinkKind::Markdown => (self.resolve_markdown(source, raw_target), "broken"),
            LinkKind::Wiki | LinkKind::Embed => (self.resolve_wikilink(raw_target), "broken"),
        };

        match candidates.len() {
            0 => ResolvedLink {
                target: None,
                target_heading,
                resolution: broken_resolution,
            },
            1 => ResolvedLink {
                target: Some(candidates.into_iter().next().expect("one candidate")),
                target_heading,
                resolution: "resolved",
            },
            _ => ResolvedLink {
                target: None,
                target_heading,
                resolution: "ambiguous",
            },
        }
    }

    fn resolve_markdown(&self, source: &Path, target: &str) -> Vec<PathBuf> {
        let source_directory = source.parent().unwrap_or_else(|| Path::new(""));
        let Some(path) = normalize_relative(&source_directory.join(target)) else {
            return Vec::new();
        };
        self.by_path
            .get(&path_key(&path))
            .cloned()
            .into_iter()
            .collect()
    }

    fn resolve_wikilink(&self, target: &str) -> Vec<PathBuf> {
        let target = target.trim_start_matches('/');
        if target.contains('/') {
            let key = format!("{target}.md");
            return self.by_path.get(&key).cloned().into_iter().collect();
        }

        let mut candidates = Vec::new();
        if let Some(paths) = self.by_alias.get(target) {
            candidates.extend(paths.iter().cloned());
        }
        if let Some(paths) = self.by_stem.get(target) {
            candidates.extend(paths.iter().cloned());
        }
        candidates.sort();
        candidates.dedup();
        candidates
    }
}

fn path_key(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn normalize_relative(path: &Path) -> Option<PathBuf> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return None;
                }
            }
            Component::Normal(part) => normalized.push(part),
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(normalized)
}

fn is_external(target: &str) -> bool {
    target.contains("://")
        || target.starts_with("mailto:")
        || target.starts_with("tel:")
        || target.starts_with("data:")
}
