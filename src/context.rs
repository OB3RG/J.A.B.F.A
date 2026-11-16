use std::collections::HashSet;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};
use serde::Serialize;

use crate::OUTPUT_SCHEMA_VERSION;
use crate::error::{JabfaError, Result};
use crate::index::IndexConfig;
use crate::navigation::safe_relative_path;
use crate::query::{SearchResult, search_sections, search_sections_in_paths};

const MAX_SUBJECT_PATHS: usize = 20;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContextResponse {
    pub output_schema_version: u32,
    pub generation: i64,
    pub context: String,
    pub estimated_tokens: usize,
    pub included: usize,
    pub omitted: usize,
    pub deduplicated: usize,
    pub truncated: usize,
}

pub fn build_context(
    config: &IndexConfig,
    query: &str,
    max_tokens: usize,
) -> Result<ContextResponse> {
    build_context_impl(config, query, max_tokens, None)
}

/// Build evidence only from selected vault-relative subject paths.
pub fn build_context_for_paths(
    config: &IndexConfig,
    query: &str,
    paths: &[&Path],
    max_tokens: usize,
) -> Result<ContextResponse> {
    if paths.len() > MAX_SUBJECT_PATHS {
        return Err(JabfaError::SubjectSelectionLimitExceeded {
            max_paths: MAX_SUBJECT_PATHS,
        });
    }
    let paths = paths
        .iter()
        .map(|path| safe_relative_path(path))
        .collect::<Result<Vec<_>>>()?;
    build_context_impl(config, query, max_tokens, Some(&paths))
}

fn build_context_impl(
    config: &IndexConfig,
    query: &str,
    max_tokens: usize,
    selected_paths: Option<&[PathBuf]>,
) -> Result<ContextResponse> {
    let search = match selected_paths {
        Some(paths) => search_sections_in_paths(config, query, paths, 100)?,
        None => search_sections(config, query, 100)?,
    };
    if !config.database_path.is_file() {
        return Err(JabfaError::DatabaseNotFound(config.database_path.clone()));
    }
    let connection = Connection::open_with_flags(
        &config.database_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;

    let mut response = ContextResponse {
        output_schema_version: OUTPUT_SCHEMA_VERSION,
        generation: search.generation,
        context: String::new(),
        estimated_tokens: 0,
        included: 0,
        omitted: 0,
        deduplicated: 0,
        truncated: 0,
    };
    let mut seen_text = HashSet::new();

    for result in search.results {
        let text: String = connection.query_row(
            "SELECT text FROM sections WHERE id = ?1 AND content_hash = ?2",
            rusqlite::params![result.section_id, result.content_hash],
            |row| row.get(0),
        )?;
        let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
        let text_hash = blake3::hash(normalized.as_bytes()).to_hex().to_string();
        if !seen_text.insert(text_hash) {
            response.deduplicated += 1;
            continue;
        }

        let citation = citation(&result);
        let block = format!("{citation}{text}\n");
        let candidate = append_block(&response.context, &block);
        if estimate_tokens(&candidate) <= max_tokens {
            response.context = candidate;
            response.included += 1;
            continue;
        }

        if response.included == 0 {
            let suffix = "\n";
            let available = max_tokens
                .saturating_mul(4)
                .saturating_sub(citation.len() + suffix.len());
            if available > 0 {
                let shortened = if selected_paths.is_some() {
                    truncate_to_passages(&text, available, query, &result.snippet)
                } else {
                    truncate_to_match(&text, available, query, &result.snippet)
                };
                response.context = format!("{citation}{shortened}{suffix}");
                response.included += 1;
                response.truncated += 1;
                continue;
            }
        }

        response.omitted += 1;
    }

    response.estimated_tokens = estimate_tokens(&response.context);
    Ok(response)
}

fn citation(result: &SearchResult) -> String {
    let heading = if result.heading_path.is_empty() {
        "Preamble".to_owned()
    } else {
        result.heading_path.join(" > ")
    };
    format!(
        "## Evidence\nPath: `{}`\nHeading: {}\nLines: {}-{}\nHash: {}\n\n",
        result.path.display(),
        heading,
        result.start_line,
        result.end_line,
        result.content_hash
    )
}

fn append_block(context: &str, block: &str) -> String {
    if context.is_empty() {
        block.to_owned()
    } else {
        format!("{context}\n{block}")
    }
}

fn estimate_tokens(text: &str) -> usize {
    text.len().div_ceil(4)
}

fn truncate_to_match(text: &str, available_bytes: usize, query: &str, snippet: &str) -> String {
    const MARKER: &str = "…";
    if text.len() <= available_bytes {
        return text.to_owned();
    }
    if available_bytes <= MARKER.len() {
        return truncate_utf8(text, available_bytes).to_owned();
    }

    let Some(match_start) = find_query_match(text, query, snippet) else {
        let shortened = truncate_utf8(text, available_bytes - MARKER.len());
        return format!("{shortened}{MARKER}");
    };
    if available_bytes <= 2 * MARKER.len() {
        let shortened = truncate_utf8(text, available_bytes - MARKER.len());
        return format!("{shortened}{MARKER}");
    }

    // Reserve both markers so the returned excerpt stays within budget even
    // when the matching passage is in the middle of the section.
    let content_budget = available_bytes - 2 * MARKER.len();
    let mut start = match_start
        .saturating_sub(content_budget / 2)
        .min(text.len().saturating_sub(content_budget));
    while start > 0 && !text.is_char_boundary(start) {
        start -= 1;
    }
    let mut end = (start + content_budget).min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let prefix = if start > 0 { MARKER } else { "" };
    let suffix = if end < text.len() { MARKER } else { "" };
    format!("{prefix}{}{suffix}", &text[start..end])
}

fn truncate_to_passages(text: &str, available_bytes: usize, query: &str, snippet: &str) -> String {
    let passages = bold_passages(text);
    if passages.len() < 2 {
        return truncate_to_match(text, available_bytes, query, snippet);
    }
    let query_tokens = normalized_tokens(query);
    if query_tokens.is_empty() {
        return truncate_to_match(text, available_bytes, query, snippet);
    }
    let passage_tokens = passages
        .iter()
        .map(|passage| normalized_tokens(passage))
        .collect::<Vec<_>>();
    let coverage = passage_tokens
        .iter()
        .map(|tokens| tokens.intersection(&query_tokens).count())
        .collect::<Vec<_>>();
    let best_coverage = coverage.iter().copied().max().unwrap_or(0);
    let mut uncovered = query_tokens.clone();
    let mut selected = Vec::new();
    if let Some(index) = (0..passages.len()).rfind(|index| {
        is_milestone(passages[*index])
            && coverage[*index] > 0
            && coverage[*index] * 2 >= best_coverage
    }) {
        selected.push(index);
        uncovered.retain(|token| !passage_tokens[index].contains(token));
    }
    let heading_tokens = passages
        .iter()
        .map(|passage| normalized_tokens(passage_label(passage)))
        .collect::<Vec<_>>();
    let mut covered_headings = selected.iter().fold(HashSet::new(), |mut tokens, index| {
        tokens.extend(heading_tokens[*index].iter().cloned());
        tokens
    });
    while selected.len() < 4 {
        let best = (0..passages.len())
            .filter(|index| !selected.contains(index))
            .map(|index| {
                let added = heading_tokens[index]
                    .intersection(&query_tokens)
                    .filter(|token| !covered_headings.contains(*token))
                    .count();
                (index, added, coverage[index])
            })
            .filter(|(_, added, _)| *added > 0)
            .max_by(|left, right| {
                left.1
                    .cmp(&right.1)
                    .then_with(|| left.2.cmp(&right.2))
                    .then_with(|| left.0.cmp(&right.0))
            });
        let Some((index, _, _)) = best else {
            break;
        };
        selected.push(index);
        covered_headings.extend(heading_tokens[index].iter().cloned());
        uncovered.retain(|token| !passage_tokens[index].contains(token));
    }
    while selected.len() < 4 {
        let best = (0..passages.len())
            .filter(|index| !selected.contains(index))
            .map(|index| {
                let added = passage_tokens[index].intersection(&uncovered).count();
                (index, added, coverage[index])
            })
            .filter(|(_, added, _)| *added > 0)
            .max_by(|left, right| {
                left.1
                    .cmp(&right.1)
                    .then_with(|| left.2.cmp(&right.2))
                    .then_with(|| left.0.cmp(&right.0))
            });
        let Some((index, _, _)) = best else {
            break;
        };
        selected.push(index);
        uncovered.retain(|token| !passage_tokens[index].contains(token));
    }
    if selected.is_empty() {
        return truncate_to_match(text, available_bytes, query, snippet);
    }

    const SEPARATOR: &str = "\n\n…\n\n";
    const SUFFIX: &str = "\n…";
    let mut composed = String::new();
    for index in selected {
        let passage = passages[index].trim();
        let separator = if composed.is_empty() { "" } else { SEPARATOR };
        if composed.len() + separator.len() + passage.len() + SUFFIX.len() > available_bytes {
            if composed.is_empty() {
                if available_bytes <= SUFFIX.len() {
                    return truncate_to_match(text, available_bytes, query, snippet);
                }
                let shortened =
                    truncate_to_match(passage, available_bytes - SUFFIX.len(), query, snippet);
                composed.push_str(&shortened);
            }
            break;
        }
        composed.push_str(separator);
        composed.push_str(passage);
    }
    if composed.is_empty() {
        return truncate_to_match(text, available_bytes, query, snippet);
    }
    composed.push_str(SUFFIX);
    composed
}

fn bold_passages(text: &str) -> Vec<&str> {
    let mut passages = Vec::new();
    let mut start = 0;
    let mut offset = 0;
    for paragraph in text.split_inclusive("\n\n") {
        if is_passage_boundary(paragraph) && offset > start {
            passages.push(&text[start..offset]);
            start = offset;
        }
        offset += paragraph.len();
    }
    if start < text.len() {
        passages.push(&text[start..]);
    }
    passages
}

fn is_passage_boundary(paragraph: &str) -> bool {
    let paragraph = paragraph.trim_start();
    [
        "**Milestone (",
        "**The finding.**",
        "**Consequences.**",
        "**Links.**",
    ]
    .iter()
    .any(|marker| paragraph.starts_with(marker))
}

fn passage_label(passage: &str) -> &str {
    let passage = passage.trim_start();
    if let Some(rest) = passage.strip_prefix("**")
        && let Some(end) = rest.find("**")
    {
        return &passage[..end + 4];
    }
    passage.lines().next().unwrap_or_default()
}

fn is_milestone(passage: &str) -> bool {
    passage.trim_start().starts_with("**Milestone (")
}

fn normalized_tokens(text: &str) -> HashSet<String> {
    text.split(|character: char| !character.is_alphanumeric())
        .filter(|term| !term.is_empty())
        .map(str::to_lowercase)
        .collect()
}

fn find_query_match(text: &str, query: &str, snippet: &str) -> Option<usize> {
    let mut fragments = snippet
        .split('…')
        .map(str::trim)
        .filter(|fragment| !fragment.is_empty())
        .collect::<Vec<_>>();
    fragments.sort_by_key(|fragment| std::cmp::Reverse(fragment.len()));
    for fragment in fragments {
        if let Some(index) = text.find(fragment) {
            return Some(index);
        }
    }

    let mut terms = query
        .split_whitespace()
        .map(|term| term.trim_matches(|character: char| !character.is_alphanumeric()))
        .filter(|term| !term.is_empty())
        .collect::<Vec<_>>();
    terms.sort_by_key(|term| std::cmp::Reverse(term.len()));
    terms.dedup();
    for term in terms {
        if let Some(index) = text.find(term) {
            return Some(index);
        }
        let lowered_text = text.to_lowercase();
        if lowered_text.len() == text.len()
            && let Some(index) = lowered_text.find(&term.to_lowercase())
        {
            return Some(index);
        }
    }
    None
}

fn truncate_utf8(text: &str, available_bytes: usize) -> &str {
    let mut end = available_bytes.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}
