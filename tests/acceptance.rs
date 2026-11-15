mod common;

use std::collections::BTreeSet;
use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;

use common::{
    assert_section, basic_vault, malformed_vault, obsidian_vault, retrieval_vault, vault_manifest,
};
use jabfa::{
    IndexConfig, JabfaError, RelatedLimits, SqlQueryLimits, build_context, doctor, get_document,
    get_section, index_status, index_vault, index_vault_incremental, list_links, parse_markdown,
    query_sql, related_documents, search_sections, watch_vault,
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
    let basic = basic_vault(&temp);
    let basic_database = temp.path().join("basic.sqlite3");
    index_vault(&IndexConfig::new(&basic, &basic_database)).expect("index healthy vault");
    let healthy = doctor(&IndexConfig::new(&basic, &basic_database)).expect("doctor healthy vault");
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
