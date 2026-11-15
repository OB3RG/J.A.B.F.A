use std::path::Path;

use pulldown_cmark::{Event, HeadingLevel, LinkType, Options, Parser, Tag, TagEnd};
use serde_json::Value;

use crate::error::{JabfaError, Result};
use crate::model::{Link, LinkKind, ParsedDocument, Section, Task};

#[derive(Debug)]
struct Heading {
    text: String,
    level: u8,
    start_line: usize,
}

#[derive(Default)]
struct Analysis {
    headings: Vec<Heading>,
    inline_tags: Vec<String>,
    tasks: Vec<Task>,
    links: Vec<Link>,
}

pub fn parse_markdown(path: &Path, source: &str) -> Result<ParsedDocument> {
    let lines: Vec<&str> = source.lines().collect();
    let frontmatter_end = frontmatter_end_line(&lines);
    let raw_frontmatter = frontmatter_end.map(|end| lines[1..end - 1].join("\n"));
    let frontmatter = match raw_frontmatter {
        Some(raw) if raw.trim().is_empty() => None,
        Some(raw) => {
            Some(
                serde_yaml::from_str(&raw).map_err(|source| JabfaError::Frontmatter {
                    path: path.to_path_buf(),
                    source,
                })?,
            )
        }
        None => None,
    };

    let analysis = analyze_markdown(source, frontmatter_end.unwrap_or(0));
    let mut tags = frontmatter_tags(frontmatter.as_ref());
    for tag in analysis.inline_tags {
        push_unique(&mut tags, tag);
    }

    let mut sections = Vec::new();
    let body_start = frontmatter_end.unwrap_or(0) + 1;
    let first_heading = analysis
        .headings
        .first()
        .map_or(lines.len() + 1, |heading| heading.start_line);

    if first_heading > body_start && body_start <= lines.len() {
        let preamble_end = first_heading - 1;
        let text = joined_text(&lines, body_start, preamble_end);
        if !text.is_empty() {
            sections.push(Section {
                heading: None,
                heading_path: Vec::new(),
                level: 0,
                start_line: body_start,
                end_line: preamble_end,
                text,
            });
        }
    }

    let mut hierarchy: Vec<(u8, String)> = Vec::new();
    for (index, heading) in analysis.headings.iter().enumerate() {
        while hierarchy
            .last()
            .is_some_and(|(level, _)| *level >= heading.level)
        {
            hierarchy.pop();
        }
        hierarchy.push((heading.level, heading.text.clone()));

        let end_line = analysis
            .headings
            .get(index + 1)
            .map_or(lines.len(), |next| next.start_line - 1);
        sections.push(Section {
            heading: Some(heading.text.clone()),
            heading_path: hierarchy.iter().map(|(_, text)| text.clone()).collect(),
            level: heading.level,
            start_line: heading.start_line,
            end_line,
            text: joined_text(&lines, heading.start_line + 1, end_line),
        });
    }

    Ok(ParsedDocument {
        path: path.to_path_buf(),
        content_hash: blake3::hash(source.as_bytes()).to_hex().to_string(),
        frontmatter,
        tags,
        tasks: analysis.tasks,
        links: analysis.links,
        sections,
    })
}

fn frontmatter_end_line(lines: &[&str]) -> Option<usize> {
    if lines.first().copied() != Some("---") {
        return None;
    }

    lines
        .iter()
        .enumerate()
        .skip(1)
        .find(|(_, line)| **line == "---")
        .map(|(index, _)| index + 1)
}

fn frontmatter_tags(frontmatter: Option<&Value>) -> Vec<String> {
    let Some(values) = frontmatter
        .and_then(|value| value.get("tags"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };

    let mut tags = Vec::new();
    for value in values {
        if let Some(tag) = value.as_str() {
            push_unique(&mut tags, tag.trim_start_matches('#').to_owned());
        }
    }
    tags
}

fn analyze_markdown(source: &str, frontmatter_end: usize) -> Analysis {
    let line_starts = line_start_offsets(source);
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_WIKILINKS);

    let mut analysis = Analysis::default();
    let mut current_heading: Option<Heading> = None;
    let mut current_task: Option<Task> = None;
    let mut code_block_depth = 0_usize;

    for (event, range) in Parser::new_ext(source, options).into_offset_iter() {
        let event_line = line_number(&line_starts, range.start);
        let in_frontmatter = event_line <= frontmatter_end;
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                current_heading = Some(Heading {
                    text: String::new(),
                    level: heading_level(level),
                    start_line: event_line,
                });
            }
            Event::Start(Tag::CodeBlock(_)) => {
                code_block_depth += 1;
            }
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                ..
            }) if !in_frontmatter && code_block_depth == 0 => {
                let kind = if matches!(link_type, LinkType::WikiLink { .. }) {
                    LinkKind::Wiki
                } else {
                    LinkKind::Markdown
                };
                analysis.links.push(Link {
                    raw_target: dest_url.into_string(),
                    kind,
                    line_number: event_line,
                });
            }
            Event::Start(Tag::Image {
                link_type,
                dest_url,
                ..
            }) if !in_frontmatter && code_block_depth == 0 => {
                let kind = if matches!(link_type, LinkType::WikiLink { .. }) {
                    LinkKind::Embed
                } else {
                    LinkKind::Markdown
                };
                analysis.links.push(Link {
                    raw_target: dest_url.into_string(),
                    kind,
                    line_number: event_line,
                });
            }
            Event::End(TagEnd::CodeBlock) => {
                code_block_depth = code_block_depth.saturating_sub(1);
            }
            Event::TaskListMarker(completed) if !in_frontmatter && code_block_depth == 0 => {
                current_task = Some(Task {
                    text: String::new(),
                    completed,
                    line_number: event_line,
                });
            }
            Event::Text(text) => {
                if let Some(heading) = &mut current_heading {
                    heading.text.push_str(&text);
                }
                if let Some(task) = &mut current_task {
                    task.text.push_str(&text);
                }
                if !in_frontmatter && code_block_depth == 0 {
                    for tag in inline_tags(&text) {
                        push_unique(&mut analysis.inline_tags, tag);
                    }
                }
            }
            Event::Code(text) => {
                if let Some(heading) = &mut current_heading {
                    heading.text.push_str(&text);
                }
                if let Some(task) = &mut current_task {
                    task.text.push_str(&text);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some(heading) = &mut current_heading {
                    heading.text.push(' ');
                }
                if let Some(task) = &mut current_task {
                    task.text.push(' ');
                }
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some(mut heading) = current_heading.take() {
                    heading.text = heading.text.trim().to_owned();
                    if heading.start_line > frontmatter_end {
                        analysis.headings.push(heading);
                    }
                }
            }
            Event::End(TagEnd::Item) => {
                if let Some(mut task) = current_task.take() {
                    task.text = task.text.trim().to_owned();
                    if !task.text.is_empty() {
                        analysis.tasks.push(task);
                    }
                }
            }
            _ => {}
        }
    }

    analysis
}

fn inline_tags(text: &str) -> Vec<String> {
    let mut tags = Vec::new();
    let mut characters = text.char_indices().peekable();

    while let Some((index, character)) = characters.next() {
        if character != '#' {
            continue;
        }
        if text[..index]
            .chars()
            .next_back()
            .is_some_and(is_tag_character)
        {
            continue;
        }

        let start = index + 1;
        let mut end = start;
        let mut has_letter = false;
        while let Some(&(next_index, next)) = characters.peek() {
            if !is_tag_character(next) {
                break;
            }
            has_letter |= next.is_alphabetic();
            end = next_index + next.len_utf8();
            characters.next();
        }

        if end > start && has_letter {
            push_unique(&mut tags, text[start..end].to_owned());
        }
    }

    tags
}

fn is_tag_character(character: char) -> bool {
    character.is_alphanumeric() || matches!(character, '_' | '-' | '/')
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !values.contains(&value) {
        values.push(value);
    }
}

fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn line_start_offsets(source: &str) -> Vec<usize> {
    let mut starts = vec![0];
    for (index, byte) in source.bytes().enumerate() {
        if byte == b'\n' {
            starts.push(index + 1);
        }
    }
    starts
}

fn line_number(line_starts: &[usize], offset: usize) -> usize {
    match line_starts.binary_search(&offset) {
        Ok(index) => index + 1,
        Err(index) => index,
    }
}

fn joined_text(lines: &[&str], start_line: usize, end_line: usize) -> String {
    if start_line == 0 || start_line > end_line || start_line > lines.len() {
        return String::new();
    }

    lines[start_line - 1..end_line.min(lines.len())]
        .join("\n")
        .trim()
        .to_owned()
}
