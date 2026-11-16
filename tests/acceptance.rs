mod common;

use std::collections::BTreeSet;
use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;

use common::{
    assert_section, basic_vault, malformed_vault, obsidian_vault, retrieval_vault, subject_vault,
    vault_manifest,
};
use jabfa::{
    BrainLearningRequest, CreateLearningRequest, IndexConfig, JabfaError, PatchSectionRequest,
    RelatedLimits, SqlQueryLimits, build_context, build_context_for_paths, create_brain_learning,
    create_learning, doctor, get_document, get_section, index_status, index_vault,
    index_vault_incremental, list_links, parse_markdown, patch_section, query_sql,
    related_documents, search_sections, search_subjects, watch_vault,
};
use rusqlite::Connection;
use tempfile::TempDir;

type LinkRow = (i64, String, String, Option<String>, String, Option<String>);

#[test]
fn at_01_index_discovers_documents_without_modifying_sources() {
    let temp = TempDir::new().expect("create temporary directory");
    let vault = basic_vault(&temp);
    let database = temp.path().join("index.sqlite3");
    let before = vault_manifest(&vault);

    let report = index_vault(&IndexConfig::new(&vault, &database)).expect("index basic vault");

    let files: Vec<String> = report
        .files
        .iter()
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .collect();
    assert_eq!(files, ["frontmatter.md", "nested/note.md", "plain.md"]);
    assert_eq!(report.documents_indexed, 3);
    assert!(database.is_file(), "index database should be created");
    assert_eq!(vault_manifest(&vault), before, "source vault changed");
}

#[test]
fn at_02_parser_builds_heading_sections_with_source_ranges() {
    let source = include_str!("fixtures/basic/frontmatter.md");
    let document = parse_markdown(Path::new("frontmatter.md"), source).expect("parse fixture");

    assert_eq!(document.sections.len(), 5);
    assert_section(
        &document.sections[0],
        "Alpha",
        1,
        &["Alpha"],
        10,
        12,
        "Intro paragraph.",
    );
    assert_section(
        &document.sections[1],
        "Search",
        2,
        &["Alpha", "Search"],
        13,
        23,
        "Find the **relevant** section.\n\n- [ ] write the test\n- [x] implement after failure\n\n```md\n# Not a heading\n- [ ] not a task\n```",
    );
    assert_section(
        &document.sections[2],
        "Token budgets",
        3,
        &["Alpha", "Search", "Token budgets"],
        24,
        26,
        "Keep evidence small.",
    );
    assert_section(
        &document.sections[3],
        "Links",
        2,
        &["Alpha", "Links"],
        27,
        29,
        "See [[Beta#Details]].",
    );
    assert_section(
        &document.sections[4],
        "Beta",
        1,
        &["Beta"],
        30,
        31,
        "Last paragraph.",
    );
}

#[test]
fn at_03_frontmatter_tags_and_tasks_are_queryable() {
    let source = include_str!("fixtures/basic/frontmatter.md");
    let document = parse_markdown(Path::new("frontmatter.md"), source).expect("parse fixture");
    let frontmatter = document.frontmatter.as_ref().expect("frontmatter");

    assert_eq!(frontmatter["title"], "JABFA Fixture");
    assert_eq!(frontmatter["created"], "2026-11-13");
    assert_eq!(frontmatter["draft"], false);
    assert_eq!(frontmatter["priority"], 3);
    assert_eq!(document.tags, ["rust", "testing"]);
    assert_eq!(document.tasks.len(), 2);
    assert_eq!(document.tasks[0].text, "write the test");
    assert!(!document.tasks[0].completed);
    assert_eq!(document.tasks[0].line_number, 16);
    assert_eq!(document.tasks[1].text, "implement after failure");
    assert!(document.tasks[1].completed);
    assert_eq!(document.tasks[1].line_number, 17);

    let temp = TempDir::new().expect("create temporary directory");
    let vault = basic_vault(&temp);
    let database = temp.path().join("index.sqlite3");
    index_vault(&IndexConfig::new(&vault, &database)).expect("index basic vault");
    let connection = Connection::open(database).expect("open index database");

    let stored_title: String = connection
        .query_row(
            "SELECT json_extract(frontmatter, '$.title') FROM documents WHERE path = ?1",
            ["frontmatter.md"],
            |row| row.get(0),
        )
        .expect("query title frontmatter");
    let stored_created: String = connection
        .query_row(
            "SELECT json_extract(frontmatter, '$.created') FROM documents WHERE path = ?1",
            ["frontmatter.md"],
            |row| row.get(0),
        )
        .expect("query created frontmatter");
    let stored_draft: i64 = connection
        .query_row(
            "SELECT json_extract(frontmatter, '$.draft') FROM documents WHERE path = ?1",
            ["frontmatter.md"],
            |row| row.get(0),
        )
        .expect("query draft frontmatter");
    let stored_priority: i64 = connection
        .query_row(
            "SELECT json_extract(frontmatter, '$.priority') FROM documents WHERE path = ?1",
            ["frontmatter.md"],
            |row| row.get(0),
        )
        .expect("query priority frontmatter");
    assert_eq!(stored_title, "JABFA Fixture");
    assert_eq!(stored_created, "2026-11-13");
    assert_eq!(stored_draft, 0);
    assert_eq!(stored_priority, 3);

    let mut tag_statement = connection
        .prepare(
            "SELECT tag FROM tags JOIN documents ON documents.id = tags.document_id
             WHERE path = 'frontmatter.md' ORDER BY tag",
        )
        .expect("prepare tag query");
    let stored_tags: Vec<String> = tag_statement
        .query_map([], |row| row.get(0))
        .expect("query tags")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("collect tags");
    assert_eq!(stored_tags, ["rust", "testing"]);

    let mut task_statement = connection
        .prepare(
            "SELECT text, completed, line_number FROM tasks
             JOIN documents ON documents.id = tasks.document_id
             WHERE path = 'frontmatter.md' ORDER BY line_number",
        )
        .expect("prepare task query");
    let stored_tasks: Vec<(String, bool, i64)> = task_statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .expect("query tasks")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("collect tasks");
    assert_eq!(stored_tasks.len(), 2);
    assert_eq!(stored_tasks[0], ("write the test".to_owned(), false, 16));
    assert_eq!(
        stored_tasks[1],
        ("implement after failure".to_owned(), true, 17)
    );

    let fake_task_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM tasks WHERE text LIKE '%not a task%'",
            [],
            |row| row.get(0),
        )
        .expect("count code-block tasks");
    assert_eq!(fake_task_count, 0);

    let inline_tag_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM tags JOIN documents ON documents.id = tags.document_id
             WHERE path = 'plain.md' AND tag = 'inline-tag'",
            [],
            |row| row.get(0),
        )
        .expect("query inline tag");
    assert_eq!(inline_tag_count, 1);
}

#[test]
fn at_04_obsidian_links_resolve_to_documents_and_sections() {
    let temp = TempDir::new().expect("create temporary directory");
    let vault = obsidian_vault(&temp);
    let database = temp.path().join("index.sqlite3");
    index_vault(&IndexConfig::new(&vault, &database)).expect("index Obsidian vault");
    let connection = Connection::open(database).expect("open index database");

    let mut statement = connection
        .prepare(
            "SELECT line_number, kind, raw_target, target_heading, resolution, targets.path
             FROM links
             JOIN documents sources ON sources.id = links.source_document_id
             LEFT JOIN documents targets ON targets.id = links.target_document_id
             WHERE sources.path = 'Home.md'
             ORDER BY line_number",
        )
        .expect("prepare link query");
    let links: Vec<LinkRow> = statement
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
            ))
        })
        .expect("query links")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("collect links");

    assert_eq!(links.len(), 6);
    assert_eq!(
        links[0],
        (
            6,
            "wiki".to_owned(),
            "projects/a/Shared#Details".to_owned(),
            Some("Details".to_owned()),
            "resolved".to_owned(),
            Some("projects/a/Shared.md".to_owned())
        )
    );
    assert_eq!(
        links[1],
        (
            7,
            "wiki".to_owned(),
            "Project Home".to_owned(),
            None,
            "resolved".to_owned(),
            Some("projects/a/Shared.md".to_owned())
        )
    );
    assert_eq!(
        links[2],
        (
            8,
            "embed".to_owned(),
            "Local#Section".to_owned(),
            Some("Section".to_owned()),
            "resolved".to_owned(),
            Some("sub/Local.md".to_owned())
        )
    );
    assert_eq!(
        links[3],
        (
            9,
            "markdown".to_owned(),
            "sub/Local.md".to_owned(),
            None,
            "resolved".to_owned(),
            Some("sub/Local.md".to_owned())
        )
    );
    assert_eq!(
        links[4],
        (
            10,
            "wiki".to_owned(),
            "Shared".to_owned(),
            None,
            "ambiguous".to_owned(),
            None
        )
    );
    assert_eq!(
        links[5],
        (
            11,
            "wiki".to_owned(),
            "Missing".to_owned(),
            None,
            "broken".to_owned(),
            None
        )
    );

    let backlink_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM links
             JOIN documents targets ON targets.id = links.target_document_id
             WHERE targets.path = 'projects/a/Shared.md'",
            [],
            |row| row.get(0),
        )
        .expect("query backlinks");
    assert_eq!(backlink_count, 2);
}

#[test]
fn at_05_full_index_is_transactional_and_repeatable() {
    let temp = TempDir::new().expect("create temporary directory");
    let vault = basic_vault(&temp);
    let database = temp.path().join("index.sqlite3");

    index_vault(&IndexConfig::new(&vault, &database)).expect("first full index");
    let connection = Connection::open(&database).expect("open first index");
    let first_documents = document_snapshot(&connection);
    let first_sections = section_snapshot(&connection);
    assert_eq!(metadata_value(&connection, "generation"), "1");
    assert_eq!(metadata_value(&connection, "state"), "complete");
    let search_matches: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM section_search WHERE section_search MATCH 'relevant'",
            [],
            |row| row.get(0),
        )
        .expect("query FTS index");
    assert_eq!(search_matches, 1);
    drop(connection);

    index_vault(&IndexConfig::new(&vault, &database)).expect("second full index");
    let connection = Connection::open(&database).expect("open second index");
    assert_eq!(document_snapshot(&connection), first_documents);
    assert_eq!(section_snapshot(&connection), first_sections);
    assert_eq!(metadata_value(&connection, "generation"), "2");
    assert_eq!(metadata_value(&connection, "state"), "complete");
    let document_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM documents", [], |row| row.get(0))
        .expect("count documents");
    let section_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM sections", [], |row| row.get(0))
        .expect("count sections");
    assert_eq!(document_count, 3);
    assert_eq!(section_count, 8);
    drop(connection);

    fs::remove_file(&database).expect("remove database for clean rebuild");
    index_vault(&IndexConfig::new(&vault, &database)).expect("clean full rebuild");
    let connection = Connection::open(&database).expect("open clean rebuild");
    assert_eq!(document_snapshot(&connection), first_documents);
    assert_eq!(section_snapshot(&connection), first_sections);
    assert_eq!(metadata_value(&connection, "generation"), "1");
    assert_eq!(metadata_value(&connection, "state"), "complete");
}

#[test]
fn at_06_malformed_note_isolated_from_other_documents() {
    let temp = TempDir::new().expect("create temporary directory");
    let vault = malformed_vault(&temp);
    fs::write(vault.join("invalid-utf8.md"), [0x66, 0x6f, 0x80, 0x6f])
        .expect("write invalid UTF-8 fixture");
    let database = temp.path().join("index.sqlite3");
    let before = vault_manifest(&vault);

    let report = index_vault(&IndexConfig::new(&vault, &database)).expect("index valid documents");

    assert_eq!(report.documents_indexed, 2);
    assert_eq!(report.files.len(), 4);
    assert_eq!(report.diagnostics.len(), 2);
    assert_eq!(report.diagnostics[0].path, Path::new("Broken.md"));
    assert_eq!(report.diagnostics[0].kind, "frontmatter");
    assert!(
        report.diagnostics[0]
            .message
            .contains("invalid YAML frontmatter")
    );
    assert_eq!(report.diagnostics[1].path, Path::new("invalid-utf8.md"));
    assert_eq!(report.diagnostics[1].kind, "io");
    assert!(report.diagnostics[1].message.contains("UTF-8"));
    assert_eq!(vault_manifest(&vault), before, "source vault changed");

    let connection = Connection::open(database).expect("open index database");
    let mut statement = connection
        .prepare("SELECT path FROM documents ORDER BY path")
        .expect("prepare valid document query");
    let documents: Vec<String> = statement
        .query_map([], |row| row.get(0))
        .expect("query valid documents")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("collect valid documents");
    assert_eq!(documents, ["Another.md", "Valid.md"]);
    let broken_sections: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sections
             JOIN documents ON documents.id = sections.document_id
             WHERE documents.path = 'Broken.md'",
            [],
            |row| row.get(0),
        )
        .expect("count partial broken sections");
    assert_eq!(broken_sections, 0);
}

#[test]
fn regression_agent_frontmatter_scalar_colons_are_recovered() {
    let source = "---\ntype: learning\norigin: Dogfood session: MCP retrieval benchmark\ndescription: Recall agent: use indexed evidence first\ntags: [project, mcp]\n---\n# Recoverable metadata\n\nThe indexed body remains searchable.\n";
    let document =
        parse_markdown(Path::new("Recovered.md"), source).expect("parse agent-style scalar colons");
    let frontmatter = document
        .frontmatter
        .as_ref()
        .expect("recovered frontmatter");
    assert_eq!(
        frontmatter["origin"],
        "Dogfood session: MCP retrieval benchmark"
    );
    assert_eq!(
        frontmatter["description"],
        "Recall agent: use indexed evidence first"
    );
    assert!(
        parse_markdown(
            Path::new("Broken.md"),
            include_str!("fixtures/malformed/Broken.md")
        )
        .is_err()
    );

    let temp = TempDir::new().expect("create temporary directory");
    let vault = temp.path().join("vault");
    fs::create_dir(&vault).expect("create recovered-frontmatter vault");
    fs::write(vault.join("Recovered.md"), source).expect("write recovered-frontmatter fixture");
    let before = vault_manifest(&vault);
    let database = temp.path().join("index.sqlite3");
    let config = IndexConfig::new(&vault, &database);
    let report = index_vault(&config).expect("index recovered-frontmatter fixture");
    assert_eq!(report.documents_indexed, 1);
    assert!(report.diagnostics.is_empty());
    let results = search_sections(&config, "searchable", 1).expect("search recovered document");
    assert_eq!(results.results.len(), 1);
    assert_eq!(results.results[0].path, Path::new("Recovered.md"));
    assert_eq!(vault_manifest(&vault), before);
}

#[test]
fn regression_agent_frontmatter_updated_annotation_is_recovered() {
    let source = "---\ntype: learning\ncreated: 2026-10-19\nupdated: 2026-10-20 (follow-up: the named entity changed)\norigin: Dogfood session: metadata recovery\ntags: [project]\n---\n# Dated metadata\n\nBody.\n";
    let document =
        parse_markdown(Path::new("Dated.md"), source).expect("recover annotated updated field");
    let frontmatter = document
        .frontmatter
        .as_ref()
        .expect("recovered frontmatter");
    assert_eq!(frontmatter["updated"], "2026-10-20");
    assert_eq!(
        frontmatter["update_note"],
        "(follow-up: the named entity changed)"
    );
    assert_eq!(frontmatter["origin"], "Dogfood session: metadata recovery");
    assert!(
        parse_markdown(
            Path::new("Broken.md"),
            include_str!("fixtures/malformed/Broken.md")
        )
        .is_err()
    );
}

#[test]
fn at_07_search_returns_ranked_section_citations() {
    let temp = TempDir::new().expect("create temporary directory");
    let vault = basic_vault(&temp);
    let database = temp.path().join("index.sqlite3");
    let config = IndexConfig::new(&vault, &database);
    index_vault(&config).expect("index basic vault");

    let response = search_sections(&config, "relevant", 3).expect("search sections");
    assert_eq!(response.generation, 1);
    assert_eq!(response.results.len(), 1);
    let result = &response.results[0];
    assert_eq!(result.path, Path::new("frontmatter.md"));
    assert_eq!(result.heading_path, ["Alpha", "Search"]);
    assert_eq!(result.start_line, 13);
    assert_eq!(result.end_line, 23);
    assert_eq!(result.content_hash.len(), 64);
    assert!(
        result
            .content_hash
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    );
    assert!(result.score.is_finite());
    assert!(result.snippet.contains("relevant"));

    let all = search_sections(&config, "a", 10).expect("search multiple sections");
    assert!(all.results.len() > 1);
    assert!(
        all.results
            .windows(2)
            .all(|pair| pair[0].score <= pair[1].score)
    );
    let limited = search_sections(&config, "a", 1).expect("limit search results");
    assert_eq!(limited.results.len(), 1);
    assert_eq!(limited.results[0], all.results[0]);
}

#[test]
fn regression_search_handles_hyphenated_plain_text_queries() {
    let temp = TempDir::new().expect("create temporary directory");
    let vault = temp.path().join("vault");
    fs::create_dir(&vault).expect("create hyphenated-query vault");
    fs::write(
        vault.join("Agent.md"),
        "# Agent safeguards\n\nThe JABFA wrapper performs a startup refresh of sqlite before the read-only canary.\n",
    )
    .expect("write hyphenated-query fixture");
    let database = temp.path().join("index.sqlite3");
    let config = IndexConfig::new(&vault, &database);
    index_vault(&config).expect("index hyphenated-query fixture");
    let query = "JABFA wrapper startup refresh sqlite read-only canary";

    let search = search_sections(&config, query, 3).expect("search literal hyphenated query");
    assert_eq!(search.results.len(), 1);
    assert_eq!(search.results[0].path, Path::new("Agent.md"));
    assert!(search.results[0].snippet.contains("read-only"));

    let context = build_context(&config, query, 250).expect("context for hyphenated query");
    assert_eq!(context.included, 1);
    assert!(context.context.contains("read-only canary"));
}

#[test]
fn at_21_subject_search_groups_matches_into_investigable_entries() {
    let temp = TempDir::new().expect("create temporary directory");
    let vault = subject_vault(&temp);
    let database = temp.path().join("index.sqlite3");
    let config = IndexConfig::new(&vault, &database);
    index_vault(&config).expect("index subject vault");
    let before = vault_manifest(&vault);

    let sections = search_sections(&config, "JABFA", 10).expect("raw section matches");
    assert_eq!(sections.results.len(), 3);
    let expected = search_subjects(&config, "JABFA", 10).expect("subject search");
    assert_eq!(expected.generation, 1);
    assert_eq!(expected.results.len(), 2);

    let learning = &expected.results[0];
    assert_eq!(learning.path, Path::new("wiki/learnings/jabfa.md"));
    assert_eq!(learning.title, "JABFA knowledge engine");
    assert_eq!(learning.entry_type, "learning");
    assert_eq!(learning.match_source, "title");
    assert_eq!(learning.matched_sections, 1);
    assert_eq!(learning.heading_path, ["JABFA knowledge engine"]);
    assert!(learning.start_line <= learning.end_line);
    assert_eq!(learning.content_hash.len(), 64);
    assert!(learning.score.is_finite());
    assert!(learning.snippet.contains("JABFA"));

    let log = &expected.results[1];
    assert_eq!(log.path, Path::new("wiki/log.md"));
    assert_eq!(log.title, "Log");
    assert_eq!(log.entry_type, "log");
    assert_eq!(log.match_source, "content");
    assert_eq!(log.matched_sections, 2);
    assert_eq!(
        search_subjects(&config, "JABFA", 1)
            .expect("limited subject search")
            .results
            .len(),
        1
    );

    let output = Command::new(env!("CARGO_BIN_EXE_jabfa"))
        .arg("subjects")
        .arg(&vault)
        .arg("JABFA")
        .arg("--database")
        .arg(&database)
        .arg("--limit")
        .arg("10")
        .arg("--json")
        .output()
        .expect("run CLI subject search");
    assert!(
        output.status.success(),
        "subjects stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let actual: serde_json::Value = serde_json::from_slice(&output.stdout).expect("subjects JSON");
    assert_versioned_fields(&actual, &["generation", "output_schema_version", "results"]);
    assert_object_fields(
        &actual["results"][0],
        &[
            "content_hash",
            "end_line",
            "entry_type",
            "heading_path",
            "match_source",
            "matched_sections",
            "path",
            "score",
            "section_id",
            "snippet",
            "start_line",
            "title",
        ],
    );
    assert_eq!(
        actual,
        serde_json::to_value(&expected).expect("subject JSON value")
    );

    let output = Command::new(env!("CARGO_BIN_EXE_jabfa"))
        .arg("subjects")
        .arg(&vault)
        .arg("JABFA")
        .arg("--database")
        .arg(&database)
        .output()
        .expect("run human CLI subject search");
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).expect("human subjects stdout");
    assert!(stdout.contains("wiki/learnings/jabfa.md"));
    assert!(stdout.contains("JABFA knowledge engine"));
    assert!(stdout.contains("learning"));
    assert!(stdout.contains("wiki/log.md"));
    assert_eq!(vault_manifest(&vault), before, "source vault changed");
}

#[test]
fn regression_subject_title_matching_uses_fts_tokenization() {
    let temp = TempDir::new().expect("create temporary directory");
    let vault = subject_vault(&temp);
    let database = temp.path().join("index.sqlite3");
    let config = IndexConfig::new(&vault, &database);
    index_vault(&config).expect("index punctuation-normalized title vault");

    let sections = search_sections(&config, "read-only", 10).expect("FTS title match");
    assert_eq!(sections.results.len(), 1);
    let subjects = search_subjects(&config, "read-only", 10).expect("subject title match");
    assert_eq!(subjects.results.len(), 1);
    assert_eq!(
        subjects.results[0].path,
        Path::new("wiki/learnings/read-only.md")
    );
    assert_eq!(subjects.results[0].match_source, "title");
}

#[test]
fn at_22_context_can_be_restricted_to_selected_subjects() {
    let temp = TempDir::new().expect("create temporary directory");
    let vault = subject_vault(&temp);
    let database = temp.path().join("index.sqlite3");
    let config = IndexConfig::new(&vault, &database);
    index_vault(&config).expect("index subject vault");
    let before = vault_manifest(&vault);
    let selected = Path::new("wiki/learnings/jabfa.md");

    let unfiltered = build_context(&config, "shared evidence", 1_000)
        .expect("unfiltered subject fixture context");
    assert!(unfiltered.included > 1);
    let expected = build_context_for_paths(&config, "shared evidence", &[selected], 1_000)
        .expect("selected-subject context");
    assert_eq!(expected.generation, 1);
    assert_eq!(expected.included, 1);
    assert!(expected.context.contains("Path: `wiki/learnings/jabfa.md`"));
    assert!(expected.context.contains("durable entry"));
    assert!(!expected.context.contains("wiki/log.md"));
    assert_eq!(
        expected.estimated_tokens,
        expected.context.len().div_ceil(4)
    );

    let empty = build_context_for_paths(&config, "shared evidence", &[], 1_000)
        .expect("empty subject selection");
    assert_eq!(empty.generation, 1);
    assert_eq!(empty.included, 0);
    assert!(empty.context.is_empty());
    let traversal = build_context_for_paths(
        &config,
        "shared evidence",
        &[Path::new("../outside.md")],
        1_000,
    );
    assert!(matches!(traversal, Err(JabfaError::PathOutsideVault(_))));
    let excessive = (0..21)
        .map(|index| PathBuf::from(format!("{index}.md")))
        .collect::<Vec<_>>();
    let excessive = excessive.iter().map(PathBuf::as_path).collect::<Vec<_>>();
    assert!(matches!(
        build_context_for_paths(&config, "shared evidence", &excessive, 1_000),
        Err(JabfaError::SubjectSelectionLimitExceeded { max_paths: 20 })
    ));

    let output = Command::new(env!("CARGO_BIN_EXE_jabfa"))
        .arg("context")
        .arg(&vault)
        .arg("shared evidence")
        .arg("--database")
        .arg(&database)
        .arg("--path")
        .arg(selected)
        .arg("--max-tokens")
        .arg("1000")
        .arg("--json")
        .output()
        .expect("run selected-subject CLI context");
    assert!(
        output.status.success(),
        "context stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let actual: serde_json::Value = serde_json::from_slice(&output.stdout).expect("context JSON");
    assert_eq!(
        actual,
        serde_json::to_value(&expected).expect("context JSON value")
    );

    let output = Command::new(env!("CARGO_BIN_EXE_jabfa"))
        .arg("context")
        .arg(&vault)
        .arg("shared evidence")
        .arg("--database")
        .arg(&database)
        .arg("--path")
        .arg(selected)
        .arg("--max-tokens")
        .arg("1000")
        .output()
        .expect("run human selected-subject CLI context");
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert_eq!(
        String::from_utf8(output.stdout).expect("human context stdout"),
        expected.context
    );
    assert_eq!(vault_manifest(&vault), before, "source vault changed");
}

#[test]
fn at_23_selected_context_composes_relevant_passages_from_long_sections() {
    let temp = TempDir::new().expect("create temporary directory");
    let vault = temp.path().join("vault");
    fs::create_dir(&vault).expect("create milestone-passage vault");
    let source = format!(
        "# Long project\n\n**Milestone (older).** MIT license and the startup wrapper are documented for JABFA.\n\n**Milestone (noise).** {}\n\n**Milestone (current).** The `subjects` workflow uses selected paths and eight read-only tools; the canary completed.\n",
        "Distractor implementation details consume the continuous excerpt budget. ".repeat(40)
    );
    fs::write(vault.join("Project.md"), source).expect("write milestone-passage fixture");
    let database = temp.path().join("index.sqlite3");
    let config = IndexConfig::new(&vault, &database);
    index_vault(&config).expect("index milestone-passage fixture");

    let response = build_context_for_paths(
        &config,
        "MIT wrapper subjects canary",
        &[Path::new("Project.md")],
        180,
    )
    .expect("compose selected milestone passages");
    assert_eq!(response.included, 1);
    assert_eq!(response.truncated, 1);
    assert!(response.estimated_tokens <= 180);
    assert!(response.context.contains("MIT license"));
    assert!(response.context.contains("startup wrapper"));
    assert!(response.context.contains("eight read-only tools"));
    assert!(response.context.contains("canary completed"));
    assert!(!response.context.contains("Distractor implementation"));
    assert!(response.context.contains('…'));
}

#[test]
fn regression_selected_context_includes_latest_relevant_milestone() {
    let temp = TempDir::new().expect("create temporary directory");
    let vault = temp.path().join("vault");
    fs::create_dir(&vault).expect("create latest-milestone vault");
    let source = format!(
        "# Long project\n\n**Milestone (older).** MIT license, startup wrapper, canary setup, and seven tools were documented with extensive supporting detail.\n\n**Milestone (noise).** {}\n\n**Milestone (current).** MIT wrapper setup now uses eight read-only tools and subject-first investigation.\n",
        "Distractor implementation details consume the continuous excerpt budget. ".repeat(40)
    );
    fs::write(vault.join("Project.md"), source).expect("write latest-milestone fixture");
    let database = temp.path().join("index.sqlite3");
    let config = IndexConfig::new(&vault, &database);
    index_vault(&config).expect("index latest-milestone fixture");

    let response = build_context_for_paths(
        &config,
        "MIT wrapper canary setup",
        &[Path::new("Project.md")],
        180,
    )
    .expect("retain latest relevant milestone");
    assert_eq!(response.included, 1);
    assert_eq!(response.truncated, 1);
    assert!(response.estimated_tokens <= 180);
    assert!(response.context.contains("seven tools"));
    assert!(response.context.contains("eight read-only tools"));
    assert!(response.context.contains("subject-first investigation"));
    assert!(!response.context.contains("Distractor implementation"));
}

#[test]
fn regression_selected_context_retains_distinct_query_headings() {
    let temp = TempDir::new().expect("create temporary directory");
    let vault = temp.path().join("vault");
    fs::create_dir(&vault).expect("create query-heading vault");
    let source = format!(
        "# Long project\n\n**Milestone (older, MIT license).** MIT license, the startup wrapper, and a pending canary were documented for JABFA.\n\n**Milestone (noise).** {}\n\n**Milestone (real-vault structured-write canary complete).** The user-approved canary completed; new learning pages use brain-create-learning.\n\n**Milestone (current, subject-first retrieval).** The `subjects` workflow uses required investigation paths and current read-only tools.\n",
        "Distractor implementation details consume the continuous excerpt budget. ".repeat(40)
    );
    fs::write(vault.join("Project.md"), source).expect("write query-heading fixture");
    let database = temp.path().join("index.sqlite3");
    let config = IndexConfig::new(&vault, &database);
    index_vault(&config).expect("index query-heading fixture");

    let response = build_context_for_paths(
        &config,
        "MIT wrapper canary subjects",
        &[Path::new("Project.md")],
        240,
    )
    .expect("retain distinct query headings");
    assert_eq!(response.included, 1);
    assert_eq!(response.truncated, 1);
    assert!(response.estimated_tokens <= 240);
    assert!(response.context.contains("MIT license"));
    assert!(response.context.contains("startup wrapper"));
    assert!(response.context.contains("user-approved canary completed"));
    assert!(response.context.contains("brain-create-learning"));
    assert!(response.context.contains("required investigation paths"));
    assert!(!response.context.contains("Distractor implementation"));
}

#[test]
fn regression_selected_context_retains_four_query_facets() {
    let temp = TempDir::new().expect("create temporary directory");
    let vault = temp.path().join("vault");
    fs::create_dir(&vault).expect("create four-facet vault");
    let source = format!(
        "# Long project\n\n**Milestone (noise).** {}\n\n**Milestone (older, MIT license).** MIT license, the startup wrapper, and a pending canary were documented for JABFA.\n\n**Milestone (live JABFA MCP retrieval benchmark).** JABFA MCP retrieval was measured.\n\n**Milestone (real-vault structured-write canary complete).** The user-approved canary completed; new learning pages use brain-create-learning.\n\n**Milestone (current, subject-first retrieval).** The `subjects` workflow uses required investigation paths.\n",
        "Distractor implementation details consume the continuous excerpt budget. ".repeat(40)
    );
    fs::write(vault.join("Project.md"), source).expect("write four-facet fixture");
    let database = temp.path().join("index.sqlite3");
    let config = IndexConfig::new(&vault, &database);
    index_vault(&config).expect("index four-facet fixture");

    let response = build_context_for_paths(
        &config,
        "JABFA MCP MIT wrapper canary subjects",
        &[Path::new("Project.md")],
        260,
    )
    .expect("retain four query facets");
    assert_eq!(response.included, 1);
    assert_eq!(response.truncated, 1);
    assert!(response.estimated_tokens <= 260);
    assert!(response.context.contains("MIT license"));
    assert!(response.context.contains("startup wrapper"));
    assert!(
        response
            .context
            .contains("JABFA MCP retrieval was measured")
    );
    assert!(response.context.contains("user-approved canary completed"));
    assert!(response.context.contains("required investigation paths"));
    assert!(!response.context.contains("Distractor implementation"));
}

#[test]
fn regression_selected_context_truncates_latest_before_older_passages() {
    let temp = TempDir::new().expect("create temporary directory");
    let vault = temp.path().join("vault");
    fs::create_dir(&vault).expect("create latest-budget vault");
    let source = format!(
        "# Long project\n\n**Milestone (older, MIT license).** MIT license is supported by a short older passage.\n\n**Milestone (current).** {} The unique currentsetup fact must survive.\n",
        "Current implementation details require substantial space before the decisive fact. "
            .repeat(20)
    );
    fs::write(vault.join("Project.md"), source).expect("write latest-budget fixture");
    let database = temp.path().join("index.sqlite3");
    let config = IndexConfig::new(&vault, &database);
    index_vault(&config).expect("index latest-budget fixture");

    let response =
        build_context_for_paths(&config, "MIT currentsetup", &[Path::new("Project.md")], 100)
            .expect("prioritize latest passage within budget");
    assert_eq!(response.included, 1);
    assert_eq!(response.truncated, 1);
    assert!(response.estimated_tokens <= 100);
    assert!(response.context.contains("currentsetup"));
    assert!(!response.context.contains("short older passage"));
}

#[cfg(unix)]
#[test]
fn regression_selected_context_preserves_unix_backslash_paths() {
    let temp = TempDir::new().expect("create temporary directory");
    let vault = temp.path().join("vault");
    fs::create_dir_all(vault.join("wiki")).expect("create backslash-path vault");
    let selected = Path::new("wiki/a\\b.md");
    fs::write(
        vault.join(selected),
        "# Backslash path\n\nSelected evidence from a literal Unix filename.\n",
    )
    .expect("write backslash-path fixture");
    let database = temp.path().join("index.sqlite3");
    let config = IndexConfig::new(&vault, &database);
    index_vault(&config).expect("index backslash-path fixture");

    let sections =
        search_sections(&config, "selected evidence", 10).expect("search backslash-path fixture");
    assert_eq!(sections.results.len(), 1);
    assert_eq!(sections.results[0].path, selected);
    let context = build_context_for_paths(&config, "selected evidence", &[selected], 500)
        .expect("context for literal Unix backslash path");
    assert_eq!(context.included, 1);
    assert!(context.context.contains("literal Unix filename"));
}

#[test]
fn at_08_context_respects_budget_and_deduplicates_evidence() {
    let temp = TempDir::new().expect("create temporary directory");
    let vault = retrieval_vault(&temp);
    let database = temp.path().join("index.sqlite3");
    let config = IndexConfig::new(&vault, &database);
    index_vault(&config).expect("index retrieval vault");

    let full = build_context(&config, "brain agents", 1_000).expect("build full context");
    assert_eq!(full.generation, 1);
    assert_eq!(full.included, 2);
    assert_eq!(full.deduplicated, 1);
    assert_eq!(full.omitted, 0);
    assert_eq!(full.truncated, 0);
    assert!(full.context.contains("Path:"));
    assert!(full.context.contains("Lines:"));
    assert!(full.context.contains("Hash:"));
    assert!(full.context.contains("D.md"));
    assert_ne!(full.context.contains("A.md"), full.context.contains("B.md"));
    assert_eq!(full.estimated_tokens, full.context.len().div_ceil(4));

    let bounded = build_context(&config, "brain agents", 90).expect("build bounded context");
    assert_eq!(bounded.included, 1);
    assert_eq!(bounded.deduplicated, 1);
    assert_eq!(bounded.omitted, 1);
    assert_eq!(bounded.truncated, 0);
    assert!(bounded.estimated_tokens <= 90);
    assert_eq!(bounded.estimated_tokens, bounded.context.len().div_ceil(4));

    let oversized = build_context(&config, "oversized", 80).expect("truncate oversized context");
    assert_eq!(oversized.included, 1);
    assert_eq!(oversized.omitted, 0);
    assert_eq!(oversized.truncated, 1);
    assert!(oversized.estimated_tokens <= 80);
    assert!(oversized.context.contains("Path: `C.md`"));
    assert!(oversized.context.contains('…'));
    assert_eq!(
        oversized.estimated_tokens,
        oversized.context.len().div_ceil(4)
    );
}

#[test]
fn regression_context_truncation_preserves_matching_passage() {
    let temp = TempDir::new().expect("create temporary directory");
    let vault = temp.path().join("vault");
    fs::create_dir(&vault).expect("create long-section vault");
    let source = format!(
        "# Long learning\n\n**The finding.** General project context.\n\n{}**Milestone (current setup).** The unique currentmilestone marker and its actionable details must survive truncation.\n",
        "**Milestone (older).** Earlier implementation details and background consume the section budget.\n\n"
            .repeat(30)
    );
    fs::write(vault.join("Long.md"), source).expect("write long-section fixture");
    let database = temp.path().join("index.sqlite3");
    let config = IndexConfig::new(&vault, &database);
    index_vault(&config).expect("index long-section fixture");

    let search = search_sections(&config, "currentmilestone", 1).expect("search matching passage");
    assert_eq!(search.results.len(), 1);
    assert!(search.results[0].snippet.contains("currentmilestone"));
    let response = build_context(&config, "currentmilestone", 180).expect("bounded long context");
    assert_eq!(response.included, 1);
    assert_eq!(response.truncated, 1);
    assert_eq!(response.omitted, 0);
    assert!(response.estimated_tokens <= 180);
    assert!(response.context.contains("currentmilestone"));
    assert!(response.context.contains("actionable details"));
    assert!(response.context.contains('…'));
    assert!(
        response
            .context
            .contains(&format!("Hash: {}", search.results[0].content_hash))
    );
}

#[test]
fn regression_context_truncation_uses_search_snippet_location() {
    let temp = TempDir::new().expect("create temporary directory");
    let vault = temp.path().join("vault");
    fs::create_dir(&vault).expect("create repeated-match vault");
    let source = format!(
        "# Repeated matches\n\n**Milestone (older).** The license was previously undecided.\n\n{}**Milestone (current).** MIT license setup is complete and the currentsetup details must be retained.\n",
        "Older implementation background consumes space without the complete query.\n\n".repeat(25)
    );
    fs::write(vault.join("Repeated.md"), source).expect("write repeated-match fixture");
    let database = temp.path().join("index.sqlite3");
    let config = IndexConfig::new(&vault, &database);
    index_vault(&config).expect("index repeated-match fixture");

    let search = search_sections(&config, "license setup", 1).expect("search repeated terms");
    assert_eq!(search.results.len(), 1);
    assert!(search.results[0].snippet.contains("currentsetup"));
    let response =
        build_context(&config, "license setup", 180).expect("bounded repeated-match context");
    assert_eq!(response.included, 1);
    assert_eq!(response.truncated, 1);
    assert!(response.estimated_tokens <= 180);
    assert!(response.context.contains("MIT license setup"));
    assert!(response.context.contains("currentsetup"));
}

#[test]
fn at_09_read_only_sql_cannot_mutate_or_escape() {
    let temp = TempDir::new().expect("create temporary directory");
    let vault = basic_vault(&temp);
    let database = temp.path().join("index.sqlite3");
    let config = IndexConfig::new(&vault, &database);
    index_vault(&config).expect("index basic vault");
    let before_vault = vault_manifest(&vault);
    let before_documents = {
        let connection = Connection::open(&database).expect("open index before SQL queries");
        document_snapshot(&connection)
    };
    let before_sections = {
        let connection = Connection::open(&database).expect("open index before SQL queries");
        section_snapshot(&connection)
    };
    let before_schema = {
        let connection = Connection::open(&database).expect("open index before SQL queries");
        schema_snapshot(&connection)
    };

    let bounded = query_sql(
        &config,
        "SELECT path, title FROM documents ORDER BY path LIMIT 2",
        &sql_limits(2, 4_096),
    )
    .expect("run bounded SELECT query");
    assert_eq!(bounded.columns, ["path", "title"]);
    assert_eq!(
        bounded.rows,
        vec![
            vec![
                serde_json::json!("frontmatter.md"),
                serde_json::json!("Alpha")
            ],
            vec![
                serde_json::json!("nested/note.md"),
                serde_json::json!("Nested note")
            ],
        ]
    );
    assert_eq!(
        bounded.bytes,
        serde_json::to_vec(&bounded.rows)
            .expect("serialize bounded rows")
            .len()
    );

    let error = query_sql(
        &config,
        "SELECT path FROM documents ORDER BY path",
        &sql_limits(2, 4_096),
    )
    .expect_err("unbounded rows must exceed the configured row limit");
    assert!(matches!(
        error,
        JabfaError::SqlRowLimitExceeded { max_rows: 2 }
    ));

    let error = query_sql(
        &config,
        "SELECT path FROM documents ORDER BY path LIMIT 1",
        &sql_limits(2, 1),
    )
    .expect_err("result payload must exceed the configured byte limit");
    assert!(matches!(
        error,
        JabfaError::SqlByteLimitExceeded { max_bytes: 1 }
    ));

    let timeout = SqlQueryLimits {
        timeout: Duration::from_nanos(1),
        ..sql_limits(10, 4_096)
    };
    let error = query_sql(
        &config,
        "WITH RECURSIVE counter(x) AS (
             VALUES(1) UNION ALL SELECT x + 1 FROM counter
         ) SELECT sum(x) FROM counter",
        &timeout,
    )
    .expect_err("recursive query must exceed the execution-time limit");
    assert!(matches!(error, JabfaError::SqlQueryTimeout));

    let escaped_database = temp.path().join("escaped.sqlite3");
    let escaped_file = temp.path().join("escaped.txt");
    let rejected = vec![
        "INSERT INTO documents (path) VALUES ('pwned.md')".to_owned(),
        "UPDATE documents SET title = 'pwned'".to_owned(),
        "DELETE FROM documents".to_owned(),
        "CREATE TABLE pwned (id INTEGER)".to_owned(),
        "DROP TABLE documents".to_owned(),
        "PRAGMA writable_schema = ON".to_owned(),
        "PRAGMA table_info(documents)".to_owned(),
        format!(
            "ATTACH DATABASE '{}' AS escaped",
            escaped_database.display()
        ),
        "SELECT load_extension('/tmp/escaped')".to_owned(),
        "SELECT readfile('/etc/passwd')".to_owned(),
        format!("SELECT writefile('{}', 'escape')", escaped_file.display()),
        "SELECT 1; DELETE FROM documents".to_owned(),
    ];
    for sql in rejected {
        let result = query_sql(&config, &sql, &sql_limits(10, 4_096));
        assert!(result.is_err(), "unsafe SQL was accepted: {sql}");
    }

    assert!(
        !escaped_database.exists(),
        "ATTACH created an external file"
    );
    assert!(!escaped_file.exists(), "writefile created an external file");
    let connection = Connection::open(&database).expect("open index after rejected SQL");
    assert_eq!(document_snapshot(&connection), before_documents);
    assert_eq!(section_snapshot(&connection), before_sections);
    assert_eq!(schema_snapshot(&connection), before_schema);
    assert_eq!(vault_manifest(&vault), before_vault, "source vault changed");
}

#[test]
fn m2_cli_sql_json_and_human_output() {
    let temp = TempDir::new().expect("create temporary directory");
    let vault = basic_vault(&temp);
    let database = temp.path().join("index.sqlite3");
    let config = IndexConfig::new(&vault, &database);
    index_vault(&config).expect("index basic vault");
    let before = vault_manifest(&vault);
    let sql = "SELECT path, title FROM documents ORDER BY path LIMIT 2";
    let limits = sql_limits(2, 4_096);

    let expected = query_sql(&config, sql, &limits).expect("SQL through library");
    let output = Command::new(env!("CARGO_BIN_EXE_jabfa"))
        .arg("sql")
        .arg(&vault)
        .arg(sql)
        .arg("--database")
        .arg(&database)
        .arg("--max-rows")
        .arg("2")
        .arg("--max-bytes")
        .arg("4096")
        .arg("--timeout-ms")
        .arg("1000")
        .arg("--json")
        .output()
        .expect("run CLI SQL");
    assert!(
        output.status.success(),
        "SQL stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let actual: serde_json::Value = serde_json::from_slice(&output.stdout).expect("SQL JSON");
    assert_eq!(
        actual,
        serde_json::to_value(expected).expect("SQL JSON value")
    );

    let output = Command::new(env!("CARGO_BIN_EXE_jabfa"))
        .arg("sql")
        .arg(&vault)
        .arg(sql)
        .arg("--database")
        .arg(&database)
        .arg("--max-rows")
        .arg("2")
        .arg("--max-bytes")
        .arg("4096")
        .arg("--timeout-ms")
        .arg("1000")
        .output()
        .expect("run human CLI SQL");
    assert!(
        output.status.success(),
        "human SQL stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert_eq!(
        String::from_utf8(output.stdout).expect("human SQL stdout"),
        "path\ttitle\nfrontmatter.md\tAlpha\nnested/note.md\tNested note\n"
    );
    assert_eq!(vault_manifest(&vault), before, "source vault changed");
}

#[test]
fn m2_cli_search_json_and_human_output() {
    let temp = TempDir::new().expect("create temporary directory");
    let vault = basic_vault(&temp);
    let database = temp.path().join("index.sqlite3");
    let config = IndexConfig::new(&vault, &database);
    index_vault(&config).expect("index basic vault");
    let before = vault_manifest(&vault);

    let expected = search_sections(&config, "a", 1).expect("search through library");
    let output = Command::new(env!("CARGO_BIN_EXE_jabfa"))
        .arg("search")
        .arg(&vault)
        .arg("a")
        .arg("--database")
        .arg(&database)
        .arg("--limit")
        .arg("1")
        .arg("--json")
        .output()
        .expect("run CLI search");
    assert!(
        output.status.success(),
        "search stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let actual: serde_json::Value = serde_json::from_slice(&output.stdout).expect("search JSON");
    assert_eq!(
        actual,
        serde_json::to_value(expected).expect("search JSON value")
    );

    let expected = search_sections(&config, "relevant", 3).expect("search human expectation");
    let output = Command::new(env!("CARGO_BIN_EXE_jabfa"))
        .arg("search")
        .arg(&vault)
        .arg("relevant")
        .arg("--database")
        .arg(&database)
        .arg("--limit")
        .arg("3")
        .output()
        .expect("run human CLI search");
    assert!(
        output.status.success(),
        "human search stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).expect("human search stdout");
    assert!(stdout.contains("frontmatter.md:13-23"));
    assert!(stdout.contains("Alpha > Search"));
    assert!(stdout.contains(&expected.results[0].snippet));
    assert_eq!(vault_manifest(&vault), before, "source vault changed");
}

#[test]
fn m2_cli_context_json_and_human_output() {
    let temp = TempDir::new().expect("create temporary directory");
    let vault = retrieval_vault(&temp);
    let database = temp.path().join("index.sqlite3");
    let config = IndexConfig::new(&vault, &database);
    index_vault(&config).expect("index retrieval vault");
    let before = vault_manifest(&vault);

    let expected = build_context(&config, "brain agents", 90).expect("context through library");
    let output = Command::new(env!("CARGO_BIN_EXE_jabfa"))
        .arg("context")
        .arg(&vault)
        .arg("brain agents")
        .arg("--database")
        .arg(&database)
        .arg("--max-tokens")
        .arg("90")
        .arg("--json")
        .output()
        .expect("run CLI context");
    assert!(
        output.status.success(),
        "context stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let actual: serde_json::Value = serde_json::from_slice(&output.stdout).expect("context JSON");
    assert_eq!(
        actual,
        serde_json::to_value(expected).expect("context JSON value")
    );

    let expected =
        build_context(&config, "brain agents", 1_000).expect("human context expectation");
    let output = Command::new(env!("CARGO_BIN_EXE_jabfa"))
        .arg("context")
        .arg(&vault)
        .arg("brain agents")
        .arg("--database")
        .arg(&database)
        .arg("--max-tokens")
        .arg("1000")
        .output()
        .expect("run human CLI context");
    assert!(
        output.status.success(),
        "human context stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert_eq!(
        String::from_utf8(output.stdout).expect("human context stdout"),
        expected.context
    );
    assert_eq!(vault_manifest(&vault), before, "source vault changed");
}

#[test]
fn at_10_cli_json_schema_is_stable_and_stderr_is_separate() {
    let temp = TempDir::new().expect("create temporary directory");
    let vault = basic_vault(&temp);
    let database = temp.path().join("index.sqlite3");
    let before = vault_manifest(&vault);

    let index = run_json_command(
        Command::new(env!("CARGO_BIN_EXE_jabfa"))
            .arg("index")
            .arg(&vault)
            .arg("--database")
            .arg(&database),
    );
    assert_versioned_fields(
        &index,
        &[
            "diagnostics",
            "documents_indexed",
            "files",
            "generation",
            "output_schema_version",
            "sections_indexed",
        ],
    );

    let status = run_json_command(
        Command::new(env!("CARGO_BIN_EXE_jabfa"))
            .arg("status")
            .arg(&vault)
            .arg("--database")
            .arg(&database),
    );
    assert_versioned_fields(
        &status,
        &[
            "documents",
            "generation",
            "output_schema_version",
            "schema_version",
            "sections",
            "state",
        ],
    );
    assert_eq!(status["schema_version"], "1");

    let search = run_json_command(
        Command::new(env!("CARGO_BIN_EXE_jabfa"))
            .arg("search")
            .arg(&vault)
            .arg("relevant")
            .arg("--database")
            .arg(&database)
            .arg("--limit")
            .arg("3"),
    );
    assert_versioned_fields(&search, &["generation", "output_schema_version", "results"]);
    assert_eq!(
        search["results"].as_array().expect("search results").len(),
        1
    );
    assert_object_fields(
        &search["results"][0],
        &[
            "content_hash",
            "end_line",
            "heading_path",
            "path",
            "score",
            "section_id",
            "snippet",
            "start_line",
        ],
    );

    let context = run_json_command(
        Command::new(env!("CARGO_BIN_EXE_jabfa"))
            .arg("context")
            .arg(&vault)
            .arg("relevant")
            .arg("--database")
            .arg(&database)
            .arg("--max-tokens")
            .arg("500"),
    );
    assert_versioned_fields(
        &context,
        &[
            "context",
            "deduplicated",
            "estimated_tokens",
            "generation",
            "included",
            "omitted",
            "output_schema_version",
            "truncated",
        ],
    );

    let sql = run_json_command(
        Command::new(env!("CARGO_BIN_EXE_jabfa"))
            .arg("sql")
            .arg(&vault)
            .arg("SELECT path FROM documents ORDER BY path LIMIT 1")
            .arg("--database")
            .arg(&database)
            .arg("--max-rows")
            .arg("1")
            .arg("--max-bytes")
            .arg("4096")
            .arg("--timeout-ms")
            .arg("1000"),
    );
    assert_versioned_fields(&sql, &["bytes", "columns", "output_schema_version", "rows"]);
    assert_eq!(vault_manifest(&vault), before, "source vault changed");

    let malformed_temp = TempDir::new().expect("create malformed temporary directory");
    let malformed = malformed_vault(&malformed_temp);
    let malformed_database = temp.path().join("malformed.sqlite3");
    let output = Command::new(env!("CARGO_BIN_EXE_jabfa"))
        .arg("index")
        .arg(&malformed)
        .arg("--database")
        .arg(&malformed_database)
        .arg("--json")
        .output()
        .expect("run malformed JSON index");
    assert!(
        output.status.success(),
        "scoped diagnostics must not fail indexing"
    );
    assert!(
        !output.stdout.contains(&0x1b),
        "ANSI decoration in JSON output"
    );
    let diagnostic_report: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("diagnostic report JSON");
    assert_versioned_fields(
        &diagnostic_report,
        &[
            "diagnostics",
            "documents_indexed",
            "files",
            "generation",
            "output_schema_version",
            "sections_indexed",
        ],
    );
    assert_object_fields(
        &diagnostic_report["diagnostics"][0],
        &["kind", "message", "path"],
    );
    let stderr = String::from_utf8(output.stderr).expect("diagnostic stderr");
    assert!(stderr.contains("Broken.md"));
    assert!(stderr.contains("invalid YAML frontmatter"));
    assert!(!stderr.contains('\u{1b}'), "ANSI decoration on stderr");

    let missing_database = temp.path().join("missing.sqlite3");
    let output = Command::new(env!("CARGO_BIN_EXE_jabfa"))
        .arg("status")
        .arg(&vault)
        .arg("--database")
        .arg(&missing_database)
        .arg("--json")
        .output()
        .expect("run missing-database JSON status");
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("index database does not exist"));

    let output = Command::new(env!("CARGO_BIN_EXE_jabfa"))
        .arg("sql")
        .arg(&vault)
        .arg("DELETE FROM documents")
        .arg("--database")
        .arg(&database)
        .arg("--json")
        .output()
        .expect("run rejected JSON SQL");
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("SQL statement is not allowed"));

    let output = Command::new(env!("CARGO_BIN_EXE_jabfa"))
        .arg("search")
        .output()
        .expect("run invalid CLI usage");
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Usage:"));
}

#[test]
fn at_11_get_and_related_enforce_bounds() {
    let temp = TempDir::new().expect("create temporary directory");
    let vault = obsidian_vault(&temp);
    let database = temp.path().join("index.sqlite3");
    let config = IndexConfig::new(&vault, &database);
    index_vault(&config).expect("index Obsidian vault");
    let before = vault_manifest(&vault);
    let source = include_str!("fixtures/obsidian/Home.md");

    let document = get_document(&config, Path::new("Home.md"), 4_096).expect("get document");
    assert_eq!(document.output_schema_version, 1);
    assert_eq!(document.generation, 1);
    assert_eq!(document.path, Path::new("Home.md"));
    assert_eq!(document.start_line, 1);
    assert_eq!(document.end_line, 11);
    assert_eq!(document.content, source);
    assert_eq!(document.bytes, source.len());
    assert_eq!(document.content_hash.len(), 64);

    let section = get_section(&config, Path::new("Home.md"), 4, 11, 4_096)
        .expect("get exact indexed section");
    let expected_section = format!(
        "{}\n",
        source.lines().skip(3).collect::<Vec<_>>().join("\n")
    );
    assert_eq!(section.content, expected_section);
    assert_eq!(section.start_line, 4);
    assert_eq!(section.end_line, 11);
    assert_eq!(section.bytes, expected_section.len());
    assert!(get_section(&config, Path::new("Home.md"), 5, 11, 4_096).is_err());
    assert!(matches!(
        get_document(&config, Path::new("Home.md"), 4),
        Err(JabfaError::SourceByteLimitExceeded { max_bytes: 4 })
    ));
    assert!(get_document(&config, Path::new("../Home.md"), 4_096).is_err());

    let links = list_links(&config, Path::new("Home.md"), 10).expect("list forward links");
    assert_eq!(links.output_schema_version, 1);
    assert_eq!(links.generation, 1);
    assert_eq!(links.links.len(), 6);
    assert_eq!(links.links[0].line_number, 6);
    assert_eq!(
        links.links[0].target_path.as_deref(),
        Some(Path::new("projects/a/Shared.md"))
    );
    assert_eq!(links.links[4].resolution, "ambiguous");
    assert_eq!(links.links[5].resolution, "broken");
    assert!(matches!(
        list_links(&config, Path::new("Home.md"), 5),
        Err(JabfaError::LinkResultLimitExceeded { max_links: 5 })
    ));

    let limits = RelatedLimits {
        max_depth: 1,
        max_nodes: 10,
        max_bytes: 4_096,
    };
    let related = related_documents(&config, Path::new("Home.md"), &limits)
        .expect("traverse related documents");
    assert_eq!(related.output_schema_version, 1);
    assert_eq!(related.generation, 1);
    assert_eq!(
        related
            .nodes
            .iter()
            .map(|node| (node.path.as_path(), node.depth))
            .collect::<Vec<_>>(),
        vec![
            (Path::new("Home.md"), 0),
            (Path::new("projects/a/Shared.md"), 1),
            (Path::new("sub/Local.md"), 1),
        ]
    );
    assert_eq!(related.diagnostics.len(), 2);
    assert_eq!(related.diagnostics[0].resolution, "ambiguous");
    assert_eq!(related.diagnostics[1].resolution, "broken");
    assert!(related.bytes <= 4_096);

    let root_only = related_documents(
        &config,
        Path::new("Home.md"),
        &RelatedLimits {
            max_depth: 0,
            ..limits
        },
    )
    .expect("bound traversal to root");
    assert_eq!(root_only.nodes.len(), 1);
    assert_eq!(root_only.diagnostics.len(), 0);
    assert!(matches!(
        related_documents(
            &config,
            Path::new("Home.md"),
            &RelatedLimits {
                max_nodes: 2,
                ..limits
            }
        ),
        Err(JabfaError::GraphNodeLimitExceeded { max_nodes: 2 })
    ));
    assert!(matches!(
        related_documents(
            &config,
            Path::new("Home.md"),
            &RelatedLimits {
                max_bytes: 1,
                ..limits
            }
        ),
        Err(JabfaError::GraphByteLimitExceeded { max_bytes: 1 })
    ));
    assert_eq!(vault_manifest(&vault), before, "source vault changed");
}

#[test]
fn m2_cli_get_links_and_related_output() {
    let temp = TempDir::new().expect("create temporary directory");
    let vault = obsidian_vault(&temp);
    let database = temp.path().join("index.sqlite3");
    let config = IndexConfig::new(&vault, &database);
    index_vault(&config).expect("index Obsidian vault");
    let before = vault_manifest(&vault);

    let expected =
        get_section(&config, Path::new("Home.md"), 4, 11, 4_096).expect("section through library");
    let output = Command::new(env!("CARGO_BIN_EXE_jabfa"))
        .arg("get")
        .arg(&vault)
        .arg("Home.md")
        .arg("--database")
        .arg(&database)
        .arg("--start-line")
        .arg("4")
        .arg("--end-line")
        .arg("11")
        .arg("--max-bytes")
        .arg("4096")
        .arg("--json")
        .output()
        .expect("run CLI get");
    assert!(
        output.status.success(),
        "get stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let actual: serde_json::Value = serde_json::from_slice(&output.stdout).expect("get JSON");
    assert_eq!(
        actual,
        serde_json::to_value(&expected).expect("get JSON value")
    );

    let output = Command::new(env!("CARGO_BIN_EXE_jabfa"))
        .arg("get")
        .arg(&vault)
        .arg("Home.md")
        .arg("--database")
        .arg(&database)
        .arg("--start-line")
        .arg("4")
        .arg("--end-line")
        .arg("11")
        .output()
        .expect("run human CLI get");
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert_eq!(
        String::from_utf8(output.stdout).expect("human get stdout"),
        expected.content
    );

    let expected = list_links(&config, Path::new("Home.md"), 10).expect("links through library");
    let output = Command::new(env!("CARGO_BIN_EXE_jabfa"))
        .arg("links")
        .arg(&vault)
        .arg("Home.md")
        .arg("--database")
        .arg(&database)
        .arg("--limit")
        .arg("10")
        .arg("--json")
        .output()
        .expect("run CLI links");
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let actual: serde_json::Value = serde_json::from_slice(&output.stdout).expect("links JSON");
    assert_eq!(
        actual,
        serde_json::to_value(&expected).expect("links JSON value")
    );

    let output = Command::new(env!("CARGO_BIN_EXE_jabfa"))
        .arg("links")
        .arg(&vault)
        .arg("Home.md")
        .arg("--database")
        .arg(&database)
        .output()
        .expect("run human CLI links");
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).expect("human links stdout");
    assert!(stdout.contains("6\twiki\tprojects/a/Shared#Details\tresolved\tprojects/a/Shared.md"));
    assert!(stdout.contains("11\twiki\tMissing\tbroken\t-"));

    let limits = RelatedLimits {
        max_depth: 1,
        max_nodes: 10,
        max_bytes: 4_096,
    };
    let expected =
        related_documents(&config, Path::new("Home.md"), &limits).expect("related through library");
    let output = Command::new(env!("CARGO_BIN_EXE_jabfa"))
        .arg("related")
        .arg(&vault)
        .arg("Home.md")
        .arg("--database")
        .arg(&database)
        .arg("--max-depth")
        .arg("1")
        .arg("--max-nodes")
        .arg("10")
        .arg("--max-bytes")
        .arg("4096")
        .arg("--json")
        .output()
        .expect("run CLI related");
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let actual: serde_json::Value = serde_json::from_slice(&output.stdout).expect("related JSON");
    assert_eq!(
        actual,
        serde_json::to_value(&expected).expect("related JSON value")
    );

    let output = Command::new(env!("CARGO_BIN_EXE_jabfa"))
        .arg("related")
        .arg(&vault)
        .arg("Home.md")
        .arg("--database")
        .arg(&database)
        .arg("--max-depth")
        .arg("1")
        .output()
        .expect("run human CLI related");
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).expect("human related stdout");
    assert!(stdout.contains("0\tHome.md"));
    assert!(stdout.contains("1\tprojects/a/Shared.md"));
    assert!(stdout.contains("diagnostic\tHome.md:10\tambiguous\tShared"));
    assert!(stdout.contains("diagnostic\tHome.md:11\tbroken\tMissing"));
    assert_eq!(vault_manifest(&vault), before, "source vault changed");
}

#[test]
fn at_12_read_only_commands_preserve_entire_vault() {
    let temp = TempDir::new().expect("create temporary directory");
    let vault = obsidian_vault(&temp);
    let database = temp.path().join("index.sqlite3");
    let config = IndexConfig::new(&vault, &database);
    index_vault(&config).expect("index Obsidian vault");
    let before = vault_manifest(&vault);

    let status = run_json_command(
        Command::new(env!("CARGO_BIN_EXE_jabfa"))
            .arg("status")
            .arg(&vault)
            .arg("--database")
            .arg(&database),
    );
    assert_eq!(status["generation"], 1);
    assert_eq!(vault_manifest(&vault), before, "status changed vault");

    let search = run_json_command(
        Command::new(env!("CARGO_BIN_EXE_jabfa"))
            .arg("search")
            .arg(&vault)
            .arg("Shared")
            .arg("--database")
            .arg(&database),
    );
    assert!(!search["results"].as_array().expect("results").is_empty());
    assert_eq!(vault_manifest(&vault), before, "search changed vault");

    let context = run_json_command(
        Command::new(env!("CARGO_BIN_EXE_jabfa"))
            .arg("context")
            .arg(&vault)
            .arg("Shared")
            .arg("--database")
            .arg(&database),
    );
    assert!(context["included"].as_u64().expect("included") >= 1);
    assert_eq!(vault_manifest(&vault), before, "context changed vault");

    let sql = run_json_command(
        Command::new(env!("CARGO_BIN_EXE_jabfa"))
            .arg("sql")
            .arg(&vault)
            .arg("SELECT path FROM documents ORDER BY path LIMIT 1")
            .arg("--database")
            .arg(&database),
    );
    assert_eq!(sql["rows"].as_array().expect("rows").len(), 1);
    assert_eq!(vault_manifest(&vault), before, "SQL changed vault");

    let get = run_json_command(
        Command::new(env!("CARGO_BIN_EXE_jabfa"))
            .arg("get")
            .arg(&vault)
            .arg("Home.md")
            .arg("--database")
            .arg(&database)
            .arg("--start-line")
            .arg("4")
            .arg("--end-line")
            .arg("11"),
    );
    assert_eq!(get["start_line"], 4);
    assert_eq!(vault_manifest(&vault), before, "get changed vault");

    let links = run_json_command(
        Command::new(env!("CARGO_BIN_EXE_jabfa"))
            .arg("links")
            .arg(&vault)
            .arg("Home.md")
            .arg("--database")
            .arg(&database),
    );
    assert_eq!(links["links"].as_array().expect("links").len(), 6);
    assert_eq!(vault_manifest(&vault), before, "links changed vault");

    let related = run_json_command(
        Command::new(env!("CARGO_BIN_EXE_jabfa"))
            .arg("related")
            .arg(&vault)
            .arg("Home.md")
            .arg("--database")
            .arg(&database)
            .arg("--max-depth")
            .arg("1"),
    );
    assert_eq!(related["nodes"].as_array().expect("nodes").len(), 3);
    assert_eq!(vault_manifest(&vault), before, "related changed vault");

    let read_only_human = [
        ("status", vec![]),
        ("search", vec!["Shared"]),
        ("context", vec!["Shared"]),
        ("sql", vec!["SELECT path FROM documents LIMIT 1"]),
        ("get", vec!["Home.md"]),
        ("links", vec!["Home.md"]),
        ("related", vec!["Home.md"]),
    ];
    for (command, arguments) in read_only_human {
        let mut invocation = Command::new(env!("CARGO_BIN_EXE_jabfa"));
        invocation.arg(command).arg(&vault);
        if command == "status" {
            invocation.arg("--database").arg(&database);
            let output = invocation.output().expect("run human status");
            assert!(output.status.success());
            assert!(output.stderr.is_empty());
            assert_eq!(vault_manifest(&vault), before, "human status changed vault");
            continue;
        }
        invocation.args(arguments);
        invocation.arg("--database").arg(&database);
        let output = invocation.output().expect("run human read-only command");
        assert!(
            output.status.success(),
            "human {command} stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
        assert_eq!(
            vault_manifest(&vault),
            before,
            "human {command} changed vault"
        );
    }
}

#[test]
fn at_13_incremental_add_change_delete_matches_full_rebuild() {
    let temp = TempDir::new().expect("create temporary directory");
    let vault = basic_vault(&temp);
    let incremental_database = temp.path().join("incremental.sqlite3");
    let full_database = temp.path().join("full.sqlite3");
    let incremental = IndexConfig::new(&vault, &incremental_database);

    let report = index_vault_incremental(&incremental).expect("initial incremental index");
    assert_eq!(report.generation, 1);
    assert_incremental_matches_full(&vault, &incremental_database, &full_database);
    let unchanged_id = document_id(&incremental_database, "frontmatter.md");

    fs::write(
        vault.join("added.md"),
        "# Added\n\nIncremental added body.\n",
    )
    .expect("add Markdown source");
    let report = index_vault_incremental(&incremental).expect("incremental added document");
    assert_eq!(report.generation, 2);
    assert_incremental_matches_full(&vault, &incremental_database, &full_database);
    assert_eq!(
        document_id(&incremental_database, "frontmatter.md"),
        unchanged_id
    );

    fs::write(
        vault.join("nested/note.md"),
        "# Nested note\n\nChanged incremental body.\n",
    )
    .expect("change Markdown source");
    let report = index_vault_incremental(&incremental).expect("incremental changed document");
    assert_eq!(report.generation, 3);
    assert_incremental_matches_full(&vault, &incremental_database, &full_database);
    assert_eq!(
        document_id(&incremental_database, "frontmatter.md"),
        unchanged_id
    );

    fs::remove_file(vault.join("plain.md")).expect("delete Markdown source");
    let report = index_vault_incremental(&incremental).expect("incremental deleted document");
    assert_eq!(report.generation, 4);
    assert_incremental_matches_full(&vault, &incremental_database, &full_database);
    assert_eq!(
        document_id(&incremental_database, "frontmatter.md"),
        unchanged_id
    );

    let connection = Connection::open(&incremental_database).expect("open final incremental index");
    let deleted: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM documents WHERE path = 'plain.md'",
            [],
            |row| row.get(0),
        )
        .expect("count deleted document");
    assert_eq!(deleted, 0);
    let changed = search_sections(&incremental, "incremental", 10).expect("search changed content");
    assert_eq!(changed.results.len(), 2);
}

#[test]
fn at_14_rename_handling_preserves_identity_where_configured() {
    let temp = TempDir::new().expect("create temporary directory");
    let vault = obsidian_vault(&temp);
    let incremental_database = temp.path().join("incremental.sqlite3");
    let full_database = temp.path().join("full.sqlite3");
    let config = IndexConfig::new(&vault, &incremental_database);
    index_vault_incremental(&config).expect("initial incremental index");
    let old_id = document_id(&incremental_database, "sub/Local.md");
    let source = fs::read_to_string(vault.join("sub/Local.md")).expect("read rename source");

    fs::create_dir(vault.join("renamed")).expect("create rename directory");
    fs::rename(vault.join("sub/Local.md"), vault.join("renamed/Local.md"))
        .expect("rename Markdown source");
    let report = index_vault_incremental(&config).expect("incremental rename");
    assert_eq!(report.generation, 2);
    assert_eq!(
        document_id(&incremental_database, "renamed/Local.md"),
        old_id
    );
    assert!(matches!(
        get_document(&config, Path::new("sub/Local.md"), 4_096),
        Err(JabfaError::DocumentNotFound(_))
    ));
    let renamed =
        get_document(&config, Path::new("renamed/Local.md"), 4_096).expect("get renamed source");
    assert_eq!(renamed.content, source);

    let links = list_links(&config, Path::new("Home.md"), 10).expect("links after rename");
    assert_eq!(
        links.links[2].target_path.as_deref(),
        Some(Path::new("renamed/Local.md"))
    );
    assert_eq!(links.links[2].resolution, "resolved");
    assert_eq!(links.links[3].resolution, "broken");
    let results =
        search_sections(&config, "\"Local section content\"", 10).expect("search renamed section");
    assert_eq!(results.results.len(), 1);
    assert_eq!(results.results[0].path, Path::new("renamed/Local.md"));
    assert_incremental_matches_full(&vault, &incremental_database, &full_database);
}

#[test]
fn at_14_rename_identity_does_not_depend_on_rowid_reuse() {
    let temp = TempDir::new().expect("create temporary directory");
    let vault = basic_vault(&temp);
    let incremental_database = temp.path().join("incremental.sqlite3");
    let full_database = temp.path().join("full.sqlite3");
    let config = IndexConfig::new(&vault, &incremental_database);
    index_vault_incremental(&config).expect("initial incremental index");
    let old_id = document_id(&incremental_database, "frontmatter.md");
    assert_eq!(old_id, 1);

    fs::rename(
        vault.join("frontmatter.md"),
        vault.join("nested/frontmatter.md"),
    )
    .expect("rename lowest-ID document");
    index_vault_incremental(&config).expect("incremental lowest-ID rename");
    assert_eq!(
        document_id(&incremental_database, "nested/frontmatter.md"),
        old_id
    );
    assert_incremental_matches_full(&vault, &incremental_database, &full_database);
}

#[test]
fn at_15_watch_updates_are_atomic_and_debounced() {
    let temp = TempDir::new().expect("create temporary directory");
    let vault = basic_vault(&temp);
    let database = temp.path().join("index.sqlite3");
    let config = IndexConfig::new(&vault, &database);
    let shutdown = Arc::new(AtomicBool::new(false));
    let (report_sender, report_receiver) = mpsc::channel();
    let watcher_config = config.clone();
    let watcher_shutdown = Arc::clone(&shutdown);
    let watcher = thread::spawn(move || {
        watch_vault(
            &watcher_config,
            Duration::from_millis(200),
            watcher_shutdown,
            move |report| {
                report_sender.send(report).expect("send watch report");
                Ok(())
            },
        )
    });

    let initial = report_receiver
        .recv_timeout(Duration::from_secs(5))
        .expect("initial watch report");
    assert_eq!(initial.generation, 1);

    let reader_done = Arc::new(AtomicBool::new(false));
    let reader_config = config.clone();
    let reader_stop = Arc::clone(&reader_done);
    let reader = thread::spawn(move || {
        while !reader_stop.load(Ordering::Relaxed) {
            let status = index_status(&reader_config).expect("concurrent index status");
            assert_eq!(status.state, "complete");
            assert!(status.generation >= 1);
            thread::yield_now();
        }
    });

    for content in [
        "# Details\n\nFirst rapid update.\n",
        "# Details\n\nSecond rapid update.\n",
        "# Details\n\nThird rapid update with watchfinal.\n",
    ] {
        fs::write(vault.join("plain.md"), content).expect("write watched source");
        thread::sleep(Duration::from_millis(10));
    }
    let updated = report_receiver
        .recv_timeout(Duration::from_secs(10))
        .expect("debounced watch report");
    assert_eq!(updated.generation, 2);
    assert_eq!(updated.documents_indexed, 3);
    assert!(
        report_receiver.recv_timeout(Duration::from_millis(400))
            == Err(mpsc::RecvTimeoutError::Timeout),
        "rapid events produced more than one index generation"
    );
    let results = search_sections(&config, "watchfinal", 10).expect("search watched content");
    assert_eq!(results.results.len(), 1);
    assert_eq!(results.results[0].path, Path::new("plain.md"));

    shutdown.store(true, Ordering::Relaxed);
    watcher
        .join()
        .expect("join watcher")
        .expect("watcher result");
    reader_done.store(true, Ordering::Relaxed);
    reader.join().expect("join concurrent reader");
}

#[test]
fn m3_cli_watch_streams_committed_json_reports() {
    let temp = TempDir::new().expect("create temporary directory");
    let vault = basic_vault(&temp);
    let database = temp.path().join("index.sqlite3");
    let mut child = CliChild(
        Command::new(env!("CARGO_BIN_EXE_jabfa"))
            .arg("index")
            .arg(&vault)
            .arg("--database")
            .arg(&database)
            .arg("--watch")
            .arg("--debounce-ms")
            .arg("100")
            .arg("--json")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn CLI watcher"),
    );
    let stdout = child.0.stdout.take().expect("capture watcher stdout");
    let (line_sender, line_receiver) = mpsc::channel();
    let reader = thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            line_sender
                .send(line.expect("read watcher JSON line"))
                .expect("send watcher JSON line");
        }
    });

    let initial = line_receiver
        .recv_timeout(Duration::from_secs(5))
        .expect("initial CLI watch line");
    let initial: serde_json::Value = serde_json::from_str(&initial).expect("initial watch JSON");
    assert_eq!(initial["generation"], 1);
    assert_eq!(initial["output_schema_version"], 1);

    fs::write(
        vault.join("plain.md"),
        "# Details\n\nCLI watch update with cliwatchfinal.\n",
    )
    .expect("write CLI watched source");
    let updated = line_receiver
        .recv_timeout(Duration::from_secs(10))
        .expect("updated CLI watch line");
    let updated: serde_json::Value = serde_json::from_str(&updated).expect("updated watch JSON");
    assert_eq!(updated["generation"], 2);

    child.0.kill().expect("stop CLI watcher");
    child.0.wait().expect("wait for CLI watcher");
    reader.join().expect("join CLI stdout reader");
    let mut stderr = String::new();
    child
        .0
        .stderr
        .take()
        .expect("capture watcher stderr")
        .read_to_string(&mut stderr)
        .expect("read watcher stderr");
    assert!(stderr.is_empty(), "watcher stderr: {stderr}");
}

#[test]
fn at_16_doctor_detects_staleness_and_broken_links() {
    let temp = TempDir::new().expect("create temporary directory");
    let healthy_vault = temp.path().join("healthy-vault");
    fs::create_dir(&healthy_vault).expect("create healthy vault");
    fs::write(
        healthy_vault.join("Healthy.md"),
        "# Healthy\n\nNo unresolved links.\n",
    )
    .expect("write healthy note");
    let healthy_database = temp.path().join("healthy.sqlite3");
    index_vault(&IndexConfig::new(&healthy_vault, &healthy_database)).expect("index healthy vault");
    let healthy =
        doctor(&IndexConfig::new(&healthy_vault, &healthy_database)).expect("doctor healthy vault");
    assert!(healthy.healthy);
    assert!(healthy.diagnostics.is_empty());

    let obsidian_temp = TempDir::new().expect("create Obsidian temporary directory");
    let vault = obsidian_vault(&obsidian_temp);
    let database = obsidian_temp.path().join("index.sqlite3");
    let config = IndexConfig::new(&vault, &database);
    index_vault(&config).expect("index doctor vault");
    fs::write(
        vault.join("projects/a/Shared.md"),
        "# Shared A\n\n## Details\nChanged without indexing.\n",
    )
    .expect("change indexed source");
    fs::remove_file(vault.join("sub/Local.md")).expect("remove indexed source");
    fs::write(vault.join("Unindexed.md"), "# Unindexed\n").expect("add unindexed source");
    let before = vault_manifest(&vault);

    let report = doctor(&config).expect("doctor stale vault");
    assert_eq!(report.output_schema_version, 1);
    assert_eq!(report.generation, 1);
    assert!(!report.healthy);
    let kinds = report
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.kind.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        kinds,
        BTreeSet::from([
            "ambiguous_link",
            "broken_link",
            "changed",
            "missing",
            "unindexed",
        ])
    );
    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.path == Path::new("Home.md")
                && diagnostic.message.contains("Missing"))
    );
    assert_eq!(vault_manifest(&vault), before, "doctor changed vault");

    index_vault_incremental(&config).expect("reindex doctor vault");
    let report = doctor(&config).expect("doctor rebuilt vault");
    assert!(report.diagnostics.iter().all(|diagnostic| {
        matches!(diagnostic.kind.as_str(), "broken_link" | "ambiguous_link")
    }));
}

#[test]
fn m3_cli_doctor_json_and_human_output() {
    let temp = TempDir::new().expect("create temporary directory");
    let vault = obsidian_vault(&temp);
    let database = temp.path().join("index.sqlite3");
    let config = IndexConfig::new(&vault, &database);
    index_vault(&config).expect("index doctor CLI vault");
    let expected = doctor(&config).expect("doctor through library");

    let output = Command::new(env!("CARGO_BIN_EXE_jabfa"))
        .arg("doctor")
        .arg(&vault)
        .arg("--database")
        .arg(&database)
        .arg("--json")
        .output()
        .expect("run CLI doctor");
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let actual: serde_json::Value = serde_json::from_slice(&output.stdout).expect("doctor JSON");
    assert_eq!(
        actual,
        serde_json::to_value(&expected).expect("doctor JSON value")
    );

    let output = Command::new(env!("CARGO_BIN_EXE_jabfa"))
        .arg("doctor")
        .arg(&vault)
        .arg("--database")
        .arg(&database)
        .output()
        .expect("run human CLI doctor");
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).expect("human doctor stdout");
    assert!(stdout.contains("Generation 1: 2 diagnostics."));
    assert!(stdout.contains("Home.md\tbroken_link"));
    assert!(stdout.contains("Home.md\tambiguous_link"));
}

#[test]
fn at_19_structured_write_rejects_stale_or_outside_root_changes() {
    let temp = TempDir::new().expect("create temporary directory");
    let vault = basic_vault(&temp);
    let database = temp.path().join("index.sqlite3");
    let config = IndexConfig::new(&vault, &database);
    index_vault(&config).expect("index write-safety fixture");
    let before_vault = vault_manifest(&vault);
    let before_index = {
        let connection = Connection::open(&database).expect("open write-safety index");
        normalized_index_snapshot(&connection)
    };

    let outside = temp.path().join("outside.md");
    let traversal = create_learning(
        &config,
        &CreateLearningRequest {
            path: PathBuf::from("../outside.md"),
            title: "Outside".to_owned(),
            body: "Must not be written.".to_owned(),
            tags: Vec::new(),
        },
    );
    assert!(matches!(traversal, Err(JabfaError::PathOutsideVault(_))));
    assert!(!outside.exists());

    let raw = create_learning(
        &config,
        &CreateLearningRequest {
            path: PathBuf::from("raw/learning.md"),
            title: "Raw".to_owned(),
            body: "Must not be written.".to_owned(),
            tags: Vec::new(),
        },
    );
    assert!(matches!(raw, Err(JabfaError::ImmutableRawPath(_))));

    let stale = patch_section(
        &config,
        &PatchSectionRequest {
            path: PathBuf::from("frontmatter.md"),
            start_line: 13,
            end_line: 23,
            expected_hash: "0".repeat(64),
            replacement: "## Search\n\nMust not be written.\n".to_owned(),
        },
    );
    assert!(matches!(stale, Err(JabfaError::StaleCitation { .. })));

    let missing = patch_section(
        &config,
        &PatchSectionRequest {
            path: PathBuf::from("Missing.md"),
            start_line: 1,
            end_line: 2,
            expected_hash: "0".repeat(64),
            replacement: "# Missing\n".to_owned(),
        },
    );
    assert!(matches!(missing, Err(JabfaError::DocumentNotFound(_))));

    assert_eq!(vault_manifest(&vault), before_vault, "source vault changed");
    assert_eq!(
        index_status(&config)
            .expect("status after rejects")
            .generation,
        1
    );
    let connection = Connection::open(&database).expect("open unchanged write-safety index");
    assert_eq!(normalized_index_snapshot(&connection), before_index);
}

#[test]
fn m5_create_learning_and_patch_section_use_temporary_vaults() {
    let temp = TempDir::new().expect("create temporary directory");
    let vault = basic_vault(&temp);
    let database = temp.path().join("index.sqlite3");
    let full_database = temp.path().join("full.sqlite3");
    let config = IndexConfig::new(&vault, &database);
    index_vault(&config).expect("index structured-write fixture");

    let created = create_learning(
        &config,
        &CreateLearningRequest {
            path: PathBuf::from("learnings/local-first.md"),
            title: "Local first".to_owned(),
            body: "A deterministic learning body.".to_owned(),
            tags: vec!["rust".to_owned(), "testing".to_owned()],
        },
    )
    .expect("create learning in temporary vault");
    assert_eq!(created.output_schema_version, 1);
    assert_eq!(created.generation, 2);
    assert_eq!(created.path, Path::new("learnings/local-first.md"));
    let created_path = vault.join("learnings/local-first.md");
    let created_source = fs::read_to_string(&created_path).expect("read created learning");
    assert!(created_source.contains("title: \"Local first\""));
    assert!(created_source.contains("- \"rust\""));
    assert!(created_source.contains("# Local first\n\nA deterministic learning body."));
    assert_eq!(created.bytes, created_source.len());
    assert_eq!(
        created.content_hash,
        blake3::hash(created_source.as_bytes()).to_hex().to_string()
    );
    let found = search_sections(&config, "deterministic", 10).expect("search created learning");
    let citation = found
        .results
        .iter()
        .find(|result| result.path == Path::new("learnings/local-first.md"))
        .expect("created learning citation");

    let patched = patch_section(
        &config,
        &PatchSectionRequest {
            path: citation.path.clone(),
            start_line: citation.start_line,
            end_line: citation.end_line,
            expected_hash: citation.content_hash.clone(),
            replacement: "# Local first\n\nRevised body with patchedmarker.\n".to_owned(),
        },
    )
    .expect("patch indexed learning section");
    assert_eq!(patched.generation, 3);
    let patched_source = fs::read_to_string(&created_path).expect("read patched learning");
    assert!(patched_source.contains("Revised body with patchedmarker."));
    assert!(!patched_source.contains("A deterministic learning body."));
    assert_eq!(
        patched.content_hash,
        blake3::hash(patched_source.as_bytes()).to_hex().to_string()
    );
    let results = search_sections(&config, "patchedmarker", 10).expect("search patched learning");
    assert_eq!(results.results.len(), 1);
    assert_eq!(
        results.results[0].path,
        Path::new("learnings/local-first.md")
    );

    let before_stale = patched_source.clone();
    let stale = patch_section(
        &config,
        &PatchSectionRequest {
            path: citation.path.clone(),
            start_line: citation.start_line,
            end_line: citation.end_line,
            expected_hash: citation.content_hash.clone(),
            replacement: "# Local first\n\nStale overwrite.\n".to_owned(),
        },
    );
    assert!(matches!(stale, Err(JabfaError::StaleCitation { .. })));
    assert_eq!(
        fs::read_to_string(&created_path).expect("read after stale patch"),
        before_stale
    );
    assert_incremental_matches_full(&vault, &database, &full_database);
}

#[test]
fn m5_configured_catalog_and_log_generation() {
    let temp = TempDir::new().expect("create temporary directory");
    let vault = basic_vault(&temp);
    let database = temp.path().join("index.sqlite3");
    let full_database = temp.path().join("full.sqlite3");
    let config = IndexConfig::new(&vault, &database).with_generated_outputs(
        Some(PathBuf::from("Catalog.md")),
        Some(PathBuf::from("Log.md")),
    );
    index_vault(&config).expect("index generated-output fixture");

    let response = create_learning(
        &config,
        &CreateLearningRequest {
            path: PathBuf::from("learnings/indexed.md"),
            title: "Indexed learning".to_owned(),
            body: "Catalog generation test body.".to_owned(),
            tags: vec!["testing".to_owned()],
        },
    )
    .expect("create learning with generated outputs");
    assert_eq!(response.generation, 4);
    assert_eq!(response.path, Path::new("learnings/indexed.md"));
    let catalog = fs::read_to_string(vault.join("Catalog.md")).expect("read generated catalog");
    assert!(catalog.starts_with("# Learning catalog\n\n"));
    assert!(catalog.contains("- [[learnings/indexed]] — Indexed learning\n"));
    let log = fs::read_to_string(vault.join("Log.md")).expect("read generated log");
    assert!(log.starts_with("# Learning log\n\n"));
    assert!(log.contains("- `learnings/indexed.md` — Indexed learning\n"));
    let links = list_links(&config, Path::new("Catalog.md"), 10).expect("catalog links");
    assert_eq!(links.links.len(), 1);
    assert_eq!(links.links[0].resolution, "resolved");
    assert_eq!(
        links.links[0].target_path.as_deref(),
        Some(Path::new("learnings/indexed.md"))
    );
    assert_incremental_matches_full(&vault, &database, &full_database);
}

#[test]
fn m5_cli_structured_writes_require_explicit_opt_in() {
    let temp = TempDir::new().expect("create temporary directory");
    let vault = basic_vault(&temp);
    let database = temp.path().join("index.sqlite3");
    let config = IndexConfig::new(&vault, &database);
    index_vault(&config).expect("index CLI write fixture");
    let before = vault_manifest(&vault);
    let learning = PathBuf::from("learnings/cli.md");

    let output = Command::new(env!("CARGO_BIN_EXE_jabfa"))
        .arg("create-learning")
        .arg(&vault)
        .arg(&learning)
        .arg("--database")
        .arg(&database)
        .arg("--title")
        .arg("CLI learning")
        .arg("--body")
        .arg("CLI write body")
        .arg("--json")
        .output()
        .expect("run write without opt-in");
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(
        vault_manifest(&vault),
        before,
        "write without opt-in changed vault"
    );
    assert!(!vault.join(&learning).exists());

    let output = Command::new(env!("CARGO_BIN_EXE_jabfa"))
        .arg("create-learning")
        .arg(&vault)
        .arg(&learning)
        .arg("--database")
        .arg(&database)
        .arg("--title")
        .arg("CLI learning")
        .arg("--body")
        .arg("CLI write body with clilearningmarker")
        .arg("--tag")
        .arg("testing")
        .arg("--allow-write")
        .arg("--json")
        .output()
        .expect("run opted-in CLI create");
    assert!(
        output.status.success(),
        "create stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let created: serde_json::Value = serde_json::from_slice(&output.stdout).expect("create JSON");
    assert_object_fields(
        &created,
        &[
            "bytes",
            "content_hash",
            "generation",
            "output_schema_version",
            "path",
        ],
    );
    assert_eq!(created["generation"], 2);
    assert_eq!(created["path"], "learnings/cli.md");

    let found = search_sections(&config, "clilearningmarker", 1).expect("CLI write citation");
    let citation = &found.results[0];
    let output = Command::new(env!("CARGO_BIN_EXE_jabfa"))
        .arg("patch-section")
        .arg(&vault)
        .arg(&citation.path)
        .arg("--database")
        .arg(&database)
        .arg("--start-line")
        .arg(citation.start_line.to_string())
        .arg("--end-line")
        .arg(citation.end_line.to_string())
        .arg("--expected-hash")
        .arg(&citation.content_hash)
        .arg("--replacement")
        .arg("# CLI learning\n\nPatched by CLI with clipatchmarker.\n")
        .arg("--allow-write")
        .arg("--json")
        .output()
        .expect("run opted-in CLI patch");
    assert!(
        output.status.success(),
        "patch stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let patched: serde_json::Value = serde_json::from_slice(&output.stdout).expect("patch JSON");
    assert_eq!(patched["generation"], 3);
    assert_eq!(patched["path"], "learnings/cli.md");
    assert!(
        fs::read_to_string(vault.join(&learning))
            .expect("read CLI patched note")
            .contains("clipatchmarker")
    );

    let output = Command::new(env!("CARGO_BIN_EXE_jabfa"))
        .arg("create-learning")
        .arg(&vault)
        .arg("learnings/human.md")
        .arg("--database")
        .arg(&database)
        .arg("--title")
        .arg("Human write")
        .arg("--body")
        .arg("Human output body")
        .arg("--allow-write")
        .output()
        .expect("run human CLI create");
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("human create stdout"),
        "Created learnings/human.md in generation 4 (71 bytes).\n"
    );
}

#[test]
fn brain_structured_write_matches_schema_and_registry_contract() {
    let temp = TempDir::new().expect("create temporary directory");
    let vault = temp.path().join("vault");
    fs::create_dir_all(vault.join("wiki/learnings")).expect("create brain fixture directories");
    fs::write(
        vault.join("wiki/index.md"),
        "# Index\n\n## Learnings\n\n- [[existing-learning]] — Existing learning (2026-01-01)\n\n## Entities\n\n- [[Existing]] — Existing entity (2026-01-01)\n",
    )
    .expect("write brain index fixture");
    fs::write(
        vault.join("wiki/log.md"),
        "# Log\n\n## [2026-01-01] learn | Existing learning\n\n- Existing log bullet.\n",
    )
    .expect("write brain log fixture");
    let database = temp.path().join("index.sqlite3");
    let full_database = temp.path().join("full.sqlite3");
    let config = IndexConfig::new(&vault, &database);
    index_vault(&config).expect("index brain-write fixture");

    let response = create_brain_learning(
        &config,
        &BrainLearningRequest {
            path: PathBuf::from("wiki/learnings/2026-11-16-write-canary.md"),
            title: "Structured Write Canary".to_owned(),
            finding: "Structured writes can create valid learning records.".to_owned(),
            context: "A temporary vault canary exercised the complete workflow.".to_owned(),
            consequences: "Use the narrow operation for future learning creation.".to_owned(),
            origin: "Test session: structured brain writes".to_owned(),
            created: "2026-11-16".to_owned(),
            tags: vec!["project".to_owned(), "dogfood".to_owned()],
        },
    )
    .expect("create schema-valid brain learning");
    assert_eq!(response.generation, 4);
    let note = fs::read_to_string(vault.join("wiki/learnings/2026-11-16-write-canary.md"))
        .expect("read brain canary");
    assert!(note.contains("type: learning"));
    assert!(note.contains("created: 2026-11-16"));
    assert!(note.contains("updated: 2026-11-16"));
    assert!(note.contains("origin: \"Test session: structured brain writes\""));
    assert!(note.contains("**The finding.** Structured writes can create valid learning records."));
    assert!(
        note.contains("**Context.** A temporary vault canary exercised the complete workflow.")
    );
    assert!(
        note.contains("**Consequences.** Use the narrow operation for future learning creation.")
    );
    let index = fs::read_to_string(vault.join("wiki/index.md")).expect("read updated brain index");
    assert!(index.contains("- [[2026-11-16-write-canary]] — Structured writes can create valid learning records. (2026-11-16)"));
    let learnings = index
        .split("## Learnings")
        .nth(1)
        .expect("learnings section");
    assert!(
        learnings
            .split("## Entities")
            .next()
            .expect("learning entries")
            .contains("write-canary")
    );
    let log = fs::read_to_string(vault.join("wiki/log.md")).expect("read updated brain log");
    assert!(log.contains("## [2026-11-16] learn | Structured Write Canary"));
    assert!(log.contains("- Structured writes can create valid learning records."));
    assert!(log.ends_with("- Updated `wiki/learnings/2026-11-16-write-canary.md`, `wiki/index.md`, and `wiki/log.md`.\n"));
    assert_incremental_matches_full(&vault, &database, &full_database);
}

#[test]
fn brain_structured_write_preserves_following_section_spacing() {
    let temp = TempDir::new().expect("create temporary directory");
    let vault = temp.path().join("vault");
    fs::create_dir_all(vault.join("wiki/learnings")).expect("create spacing fixture directories");
    fs::write(
        vault.join("wiki/index.md"),
        "# Index\n\n## Learnings\n\n- [[existing-learning]] — Existing learning (2026-01-01)\n\n## Entities\n\n- [[Existing]] — Existing entity (2026-01-01)\n",
    )
    .expect("write spacing index fixture");
    fs::write(vault.join("wiki/log.md"), "# Log\n").expect("write spacing log fixture");
    let database = temp.path().join("index.sqlite3");
    let config = IndexConfig::new(&vault, &database);
    index_vault(&config).expect("index spacing fixture");
    create_brain_learning(
        &config,
        &BrainLearningRequest {
            path: PathBuf::from("wiki/learnings/2026-11-16-spacing.md"),
            title: "Spacing".to_owned(),
            finding: "Section spacing must remain intact.".to_owned(),
            context: "A following category heading exercises the boundary.".to_owned(),
            consequences: "Keep a blank line after inserted entries.".to_owned(),
            origin: "Test session: spacing".to_owned(),
            created: "2026-11-16".to_owned(),
            tags: Vec::new(),
        },
    )
    .expect("create spacing learning");
    let index = fs::read_to_string(vault.join("wiki/index.md")).expect("read spacing index");
    assert!(index.contains("(2026-11-16)\n\n## Entities"));
}

#[test]
fn brain_structured_write_preflights_registry_before_writing() {
    let temp = TempDir::new().expect("create temporary directory");
    let vault = temp.path().join("vault");
    fs::create_dir_all(vault.join("wiki/learnings")).expect("create brain preflight directories");
    fs::write(
        vault.join("wiki/index.md"),
        "# Index\n\n## Learnings\n\n- [[existing-learning]] — Existing learning (2026-01-01)\n",
    )
    .expect("write preflight index fixture");
    let database = temp.path().join("index.sqlite3");
    let config = IndexConfig::new(&vault, &database);
    index_vault(&config).expect("index preflight fixture");
    let before_index =
        fs::read_to_string(vault.join("wiki/index.md")).expect("read preflight index");

    let result = create_brain_learning(
        &config,
        &BrainLearningRequest {
            path: PathBuf::from("wiki/learnings/2026-11-16-no-write.md"),
            title: "No Partial Write".to_owned(),
            finding: "Missing registries must fail before a source commit.".to_owned(),
            context: "The log is deliberately absent.".to_owned(),
            consequences: "No learning or index entry should be created.".to_owned(),
            origin: "Test session: preflight".to_owned(),
            created: "2026-11-16".to_owned(),
            tags: Vec::new(),
        },
    );
    assert!(matches!(result, Err(JabfaError::DocumentNotFound(_))));
    assert!(!vault.join("wiki/learnings/2026-11-16-no-write.md").exists());
    assert_eq!(
        fs::read_to_string(vault.join("wiki/index.md")).expect("read unchanged index"),
        before_index
    );
    assert_eq!(
        index_status(&config)
            .expect("preflight generation")
            .generation,
        1
    );
}

#[test]
fn brain_cli_structured_write_requires_opt_in() {
    let temp = TempDir::new().expect("create temporary directory");
    let vault = temp.path().join("vault");
    fs::create_dir_all(vault.join("wiki/learnings")).expect("create brain CLI directories");
    fs::write(
        vault.join("wiki/index.md"),
        "# Index\n\n## Learnings\n\n- [[existing-learning]] — Existing learning (2026-01-01)\n",
    )
    .expect("write brain CLI index");
    fs::write(vault.join("wiki/log.md"), "# Log\n").expect("write brain CLI log");
    let database = temp.path().join("index.sqlite3");
    let config = IndexConfig::new(&vault, &database);
    index_vault(&config).expect("index brain CLI fixture");
    let learning = PathBuf::from("wiki/learnings/2026-11-16-cli-canary.md");
    let before = vault_manifest(&vault);

    let output = Command::new(env!("CARGO_BIN_EXE_jabfa"))
        .arg("brain-create-learning")
        .arg(&vault)
        .arg(&learning)
        .arg("--database")
        .arg(&database)
        .arg("--title")
        .arg("CLI Canary")
        .arg("--finding")
        .arg("CLI creation works.")
        .arg("--context")
        .arg("Temporary fixture.")
        .arg("--consequences")
        .arg("Use the structured command.")
        .arg("--origin")
        .arg("Test session: CLI")
        .arg("--created")
        .arg("2026-11-16")
        .output()
        .expect("run brain write without opt-in");
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(vault_manifest(&vault), before);

    let output = Command::new(env!("CARGO_BIN_EXE_jabfa"))
        .arg("brain-create-learning")
        .arg(&vault)
        .arg(&learning)
        .arg("--database")
        .arg(&database)
        .arg("--title")
        .arg("CLI Canary")
        .arg("--finding")
        .arg("CLI creation works.")
        .arg("--context")
        .arg("Temporary fixture.")
        .arg("--consequences")
        .arg("Use the structured command.")
        .arg("--origin")
        .arg("Test session: CLI")
        .arg("--created")
        .arg("2026-11-16")
        .arg("--tag")
        .arg("dogfood")
        .arg("--allow-write")
        .arg("--json")
        .output()
        .expect("run opted-in brain CLI write");
    assert!(
        output.status.success(),
        "brain CLI stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let response: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("brain write JSON");
    assert_eq!(response["generation"], 4);
    assert_eq!(response["path"], "wiki/learnings/2026-11-16-cli-canary.md");
    assert_eq!(response["output_schema_version"], 1);
    let log = fs::read_to_string(vault.join("wiki/log.md")).expect("read brain CLI log");
    assert!(log.contains("## [2026-11-16] learn | CLI Canary"));
}

#[test]
fn at_20_failed_write_has_consistent_recovery_behavior() {
    let temp = TempDir::new().expect("create temporary directory");
    let vault = basic_vault(&temp);
    let database = temp.path().join("index.sqlite3");
    let recovery_database = database.clone();
    let full_database = temp.path().join("full.sqlite3");
    let config = IndexConfig::new(&vault, &database);
    index_vault(&config).expect("index recovery fixture");
    let found = search_sections(&config, "relevant", 1).expect("recovery citation search");
    let citation = &found.results[0];

    let index_lock = Connection::open(&database).expect("open index lock connection");
    index_lock
        .execute_batch("BEGIN IMMEDIATE")
        .expect("hold index write lock");
    let failure = patch_section(
        &config,
        &PatchSectionRequest {
            path: citation.path.clone(),
            start_line: citation.start_line,
            end_line: citation.end_line,
            expected_hash: citation.content_hash.clone(),
            replacement: "## Search\n\nRecovered source body with recoverymarker.\n".to_owned(),
        },
    );
    assert!(matches!(
        failure,
        Err(JabfaError::WriteRecoveryRequired { .. })
    ));
    drop(index_lock);
    let source = fs::read_to_string(vault.join("frontmatter.md")).expect("read committed source");
    assert!(source.contains("Recovered source body with recoverymarker."));
    assert!(!source.contains("Find the **relevant** section."));

    let recovery = IndexConfig::new(&vault, &recovery_database);
    let before_recovery = doctor(&recovery).expect("diagnose index fault");
    assert!(
        before_recovery
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == "changed"
                && diagnostic.path == Path::new("frontmatter.md"))
    );
    index_vault_incremental(&recovery).expect("recover committed source into index");
    let after_recovery = doctor(&recovery).expect("verify recovered index");
    assert!(
        after_recovery
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.kind != "changed")
    );
    let results = search_sections(&recovery, "recoverymarker", 1).expect("search recovered source");
    assert_eq!(results.results.len(), 1);
    assert_incremental_matches_full(&vault, &recovery_database, &full_database);

    let source_temp = TempDir::new().expect("create source-failure directory");
    let source_vault = basic_vault(&source_temp);
    let source_database = source_temp.path().join("source.sqlite3");
    let source_config = IndexConfig::new(&source_vault, &source_database);
    index_vault(&source_config).expect("index source-failure fixture");
    let found = search_sections(&source_config, "Markdown", 1).expect("source-failure citation");
    let citation = &found.results[0];
    fs::remove_file(source_vault.join("plain.md")).expect("remove source for fault");
    fs::create_dir(source_vault.join("plain.md")).expect("inject source-path failure");
    let before_fault = vault_manifest(&source_vault);
    let failure = patch_section(
        &source_config,
        &PatchSectionRequest {
            path: citation.path.clone(),
            start_line: citation.start_line,
            end_line: citation.end_line,
            expected_hash: citation.content_hash.clone(),
            replacement: "# Details\n\nMust not commit.\n".to_owned(),
        },
    );
    assert!(matches!(failure, Err(JabfaError::Io { .. })));
    assert_eq!(vault_manifest(&source_vault), before_fault);
    assert_eq!(
        index_status(&source_config)
            .expect("source-failure index status")
            .generation,
        1
    );
}

#[test]
fn m3_cli_index_supports_incremental_and_full_rebuilds() {
    let temp = TempDir::new().expect("create temporary directory");
    let vault = basic_vault(&temp);
    let database = temp.path().join("index.sqlite3");

    let initial = run_json_command(
        Command::new(env!("CARGO_BIN_EXE_jabfa"))
            .arg("index")
            .arg(&vault)
            .arg("--database")
            .arg(&database),
    );
    assert_eq!(initial["generation"], 1);
    assert_eq!(initial["documents_indexed"], 3);

    fs::write(vault.join("added.md"), "# Added\n\nCLI incremental body.\n")
        .expect("add CLI Markdown source");
    let incremental = run_json_command(
        Command::new(env!("CARGO_BIN_EXE_jabfa"))
            .arg("index")
            .arg(&vault)
            .arg("--database")
            .arg(&database),
    );
    assert_eq!(incremental["generation"], 2);
    assert_eq!(incremental["documents_indexed"], 4);
    let status = run_json_command(
        Command::new(env!("CARGO_BIN_EXE_jabfa"))
            .arg("status")
            .arg(&vault)
            .arg("--database")
            .arg(&database),
    );
    assert_eq!(status["documents"], 4);

    let full = run_json_command(
        Command::new(env!("CARGO_BIN_EXE_jabfa"))
            .arg("index")
            .arg(&vault)
            .arg("--database")
            .arg(&database)
            .arg("--full"),
    );
    assert_eq!(full["generation"], 3);
    assert_eq!(full["documents_indexed"], 4);
}

#[test]
fn m1_cli_index_and_status_json_and_human_output() {
    let temp = TempDir::new().expect("create temporary directory");
    let vault = basic_vault(&temp);
    let database = temp.path().join("index.sqlite3");
    let before = vault_manifest(&vault);

    let output = Command::new(env!("CARGO_BIN_EXE_jabfa"))
        .arg("index")
        .arg(&vault)
        .arg("--database")
        .arg(&database)
        .arg("--json")
        .output()
        .expect("run CLI index");
    assert!(
        output.status.success(),
        "index stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let index: serde_json::Value = serde_json::from_slice(&output.stdout).expect("index JSON");
    assert_eq!(index["generation"], 1);
    assert_eq!(index["documents_indexed"], 3);
    assert_eq!(index["sections_indexed"], 8);
    assert_eq!(
        index["diagnostics"].as_array().expect("diagnostics").len(),
        0
    );

    let output = Command::new(env!("CARGO_BIN_EXE_jabfa"))
        .arg("status")
        .arg(&vault)
        .arg("--database")
        .arg(&database)
        .arg("--json")
        .output()
        .expect("run CLI status");
    assert!(
        output.status.success(),
        "status stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let status: serde_json::Value = serde_json::from_slice(&output.stdout).expect("status JSON");
    assert_eq!(status["generation"], 1);
    assert_eq!(status["state"], "complete");
    assert_eq!(status["documents"], 3);
    assert_eq!(status["sections"], 8);

    let human_database = temp.path().join("human.sqlite3");
    let output = Command::new(env!("CARGO_BIN_EXE_jabfa"))
        .arg("index")
        .arg(&vault)
        .arg("--database")
        .arg(&human_database)
        .output()
        .expect("run human CLI index");
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("human index stdout"),
        "Indexed 3 documents and 8 sections in generation 1.\n"
    );
    let output = Command::new(env!("CARGO_BIN_EXE_jabfa"))
        .arg("status")
        .arg(&vault)
        .arg("--database")
        .arg(&human_database)
        .output()
        .expect("run human CLI status");
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("human status stdout"),
        "Generation 1 (complete): 3 documents, 8 sections.\n"
    );
    assert_eq!(vault_manifest(&vault), before, "source vault changed");
}

#[test]
fn m1_cli_reports_clear_errors_and_exit_codes() {
    let temp = TempDir::new().expect("create temporary directory");
    let vault = basic_vault(&temp);
    let missing_vault = temp.path().join("missing-vault");
    let database = temp.path().join("index.sqlite3");

    let output = Command::new(env!("CARGO_BIN_EXE_jabfa"))
        .arg("index")
        .arg(&missing_vault)
        .arg("--database")
        .arg(&database)
        .arg("--json")
        .output()
        .expect("run missing-vault CLI");
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("vault root is not a directory"));

    let output = Command::new(env!("CARGO_BIN_EXE_jabfa"))
        .arg("status")
        .arg(&vault)
        .arg("--database")
        .arg(&database)
        .arg("--json")
        .output()
        .expect("run missing-database CLI");
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("index database does not exist"));
}

struct CliChild(Child);

impl Drop for CliChild {
    fn drop(&mut self) {
        if self.0.try_wait().expect("check CLI child").is_none() {
            self.0.kill().expect("kill CLI child");
            self.0.wait().expect("wait for CLI child");
        }
    }
}

fn run_json_command(command: &mut Command) -> serde_json::Value {
    let output = command.arg("--json").output().expect("run JSON command");
    assert!(
        output.status.success(),
        "JSON command stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert!(output.stdout.ends_with(b"\n"), "JSON output lacks newline");
    assert!(
        !output.stdout.contains(&0x1b),
        "ANSI decoration in JSON output"
    );
    serde_json::from_slice(&output.stdout).expect("valid JSON stdout")
}

fn assert_versioned_fields(value: &serde_json::Value, expected: &[&str]) {
    assert_object_fields(value, expected);
    assert_eq!(value["output_schema_version"], 1);
}

fn assert_object_fields(value: &serde_json::Value, expected: &[&str]) {
    let object = value.as_object().expect("JSON object");
    let mut actual = object.keys().map(String::as_str).collect::<Vec<_>>();
    let mut expected = expected.to_vec();
    actual.sort_unstable();
    expected.sort_unstable();
    assert_eq!(actual, expected);
}

fn sql_limits(max_rows: usize, max_bytes: usize) -> SqlQueryLimits {
    SqlQueryLimits {
        max_rows,
        max_bytes,
        timeout: Duration::from_secs(1),
    }
}

type DocumentSnapshot = Vec<(String, Option<String>, Option<String>)>;
type SectionSnapshot = Vec<(String, String, i64, i64, String)>;

fn document_snapshot(connection: &Connection) -> DocumentSnapshot {
    let mut statement = connection
        .prepare("SELECT path, title, frontmatter FROM documents ORDER BY path")
        .expect("prepare document snapshot");
    statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .expect("query document snapshot")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("collect document snapshot")
}

fn section_snapshot(connection: &Connection) -> SectionSnapshot {
    let mut statement = connection
        .prepare(
            "SELECT documents.path, heading_path, start_line, end_line, text
             FROM sections JOIN documents ON documents.id = sections.document_id
             ORDER BY documents.path, start_line",
        )
        .expect("prepare section snapshot");
    statement
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        })
        .expect("query section snapshot")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("collect section snapshot")
}

fn schema_snapshot(connection: &Connection) -> Vec<(String, String, Option<String>)> {
    let mut statement = connection
        .prepare(
            "SELECT type, name, sql FROM sqlite_master
             ORDER BY type, name",
        )
        .expect("prepare schema snapshot");
    statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .expect("query schema snapshot")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("collect schema snapshot")
}

fn document_id(database: &Path, path: &str) -> i64 {
    Connection::open(database)
        .expect("open document identity database")
        .query_row("SELECT id FROM documents WHERE path = ?1", [path], |row| {
            row.get(0)
        })
        .expect("query document identity")
}

fn assert_incremental_matches_full(vault: &Path, incremental: &Path, full: &Path) {
    index_vault(&IndexConfig::new(vault, full)).expect("clean full rebuild");
    let incremental_connection =
        Connection::open(incremental).expect("open incremental normalized snapshot");
    let full_connection = Connection::open(full).expect("open full normalized snapshot");
    assert_eq!(
        normalized_index_snapshot(&incremental_connection),
        normalized_index_snapshot(&full_connection)
    );
}

fn normalized_index_snapshot(connection: &Connection) -> serde_json::Value {
    let mut tag_statement = connection
        .prepare(
            "SELECT documents.path, tags.tag FROM tags
             JOIN documents ON documents.id = tags.document_id
             ORDER BY documents.path, tags.tag",
        )
        .expect("prepare normalized tags");
    let tags = tag_statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .expect("query normalized tags")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("collect normalized tags");
    let mut task_statement = connection
        .prepare(
            "SELECT documents.path, tasks.text, tasks.completed, tasks.line_number FROM tasks
             JOIN documents ON documents.id = tasks.document_id
             ORDER BY documents.path, tasks.line_number, tasks.text",
        )
        .expect("prepare normalized tasks");
    let tasks = task_statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, bool>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })
        .expect("query normalized tasks")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("collect normalized tasks");
    let mut link_statement = connection
        .prepare(
            "SELECT sources.path, links.line_number, links.kind, links.raw_target,
                    links.target_heading, links.resolution, targets.path
             FROM links
             JOIN documents sources ON sources.id = links.source_document_id
             LEFT JOIN documents targets ON targets.id = links.target_document_id
             ORDER BY sources.path, links.line_number, links.kind, links.raw_target",
        )
        .expect("prepare normalized links");
    let links = link_statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, Option<String>>(6)?,
            ))
        })
        .expect("query normalized links")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("collect normalized links");
    let mut search_statement = connection
        .prepare(
            "SELECT documents.path, section_search.heading, section_search.text
             FROM section_search
             JOIN documents ON documents.id = section_search.document_id
             ORDER BY documents.path, section_search.heading, section_search.text",
        )
        .expect("prepare normalized FTS");
    let search = search_statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .expect("query normalized FTS")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("collect normalized FTS");
    serde_json::json!({
        "documents": document_snapshot(connection),
        "sections": section_snapshot(connection),
        "tags": tags,
        "tasks": tasks,
        "links": links,
        "section_search": search,
    })
}

fn metadata_value(connection: &Connection, key: &str) -> String {
    connection
        .query_row(
            "SELECT value FROM index_metadata WHERE key = ?1",
            [key],
            |row| row.get(0),
        )
        .expect("query index metadata")
}
