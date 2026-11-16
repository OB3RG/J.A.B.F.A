use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use clap::{Parser, Subcommand};
use jabfa::{
    BrainLearningRequest, CreateLearningRequest, IndexConfig, IndexReport, JabfaError,
    PatchSectionRequest, RelatedLimits, Result, SqlQueryLimits, build_context,
    build_context_for_paths, create_brain_learning, create_learning, doctor, get_document,
    get_section, index_status, index_vault, index_vault_incremental, list_links, patch_section,
    query_sql, related_documents, search_sections, search_subjects, watch_vault,
};

#[derive(Debug, Parser)]
#[command(name = "jabfa", version, about = "Just A Brain For AI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Index a Markdown vault into SQLite.
    Index {
        vault: PathBuf,
        #[arg(long)]
        database: PathBuf,
        #[arg(long, conflicts_with = "watch")]
        full: bool,
        #[arg(long)]
        watch: bool,
        #[arg(long, default_value_t = 100, requires = "watch")]
        debounce_ms: u64,
        #[arg(long)]
        json: bool,
    },
    /// Discover unique subjects before investigating their evidence.
    Subjects {
        vault: PathBuf,
        query: String,
        #[arg(long)]
        database: PathBuf,
        #[arg(long, default_value_t = 10)]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
    /// Search ranked heading-level sections.
    Search {
        vault: PathBuf,
        query: String,
        #[arg(long)]
        database: PathBuf,
        #[arg(long, default_value_t = 10)]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
    /// Build a deduplicated, token-bounded evidence bundle.
    Context {
        vault: PathBuf,
        query: String,
        #[arg(long)]
        database: PathBuf,
        #[arg(long, default_value_t = 2_000)]
        max_tokens: usize,
        #[arg(long = "path", value_name = "PATH")]
        paths: Vec<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Execute one bounded, read-only SQL statement.
    Sql {
        vault: PathBuf,
        query: String,
        #[arg(long)]
        database: PathBuf,
        #[arg(long, default_value_t = 100)]
        max_rows: usize,
        #[arg(long, default_value_t = 65_536)]
        max_bytes: usize,
        #[arg(long, default_value_t = 1_000)]
        timeout_ms: u64,
        #[arg(long)]
        json: bool,
    },
    /// Read a complete document or an exact indexed section range.
    Get {
        vault: PathBuf,
        path: PathBuf,
        #[arg(long)]
        database: PathBuf,
        #[arg(long, requires = "end_line")]
        start_line: Option<usize>,
        #[arg(long, requires = "start_line")]
        end_line: Option<usize>,
        #[arg(long, default_value_t = 1_048_576)]
        max_bytes: usize,
        #[arg(long)]
        json: bool,
    },
    /// List a document's outgoing links.
    Links {
        vault: PathBuf,
        path: PathBuf,
        #[arg(long)]
        database: PathBuf,
        #[arg(long, default_value_t = 100)]
        limit: usize,
        #[arg(long)]
        json: bool,
    },
    /// Traverse resolved incoming and outgoing links breadth first.
    Related {
        vault: PathBuf,
        path: PathBuf,
        #[arg(long)]
        database: PathBuf,
        #[arg(long, default_value_t = 2)]
        max_depth: usize,
        #[arg(long, default_value_t = 50)]
        max_nodes: usize,
        #[arg(long, default_value_t = 65_536)]
        max_bytes: usize,
        #[arg(long)]
        json: bool,
    },
    /// Create a schema-valid llm-brain learning and update its registry/log.
    BrainCreateLearning {
        vault: PathBuf,
        path: PathBuf,
        #[arg(long)]
        database: PathBuf,
        #[arg(long)]
        title: String,
        #[arg(long)]
        finding: String,
        #[arg(long)]
        context: String,
        #[arg(long)]
        consequences: String,
        #[arg(long)]
        origin: String,
        #[arg(long)]
        created: String,
        #[arg(long = "tag")]
        tags: Vec<String>,
        #[arg(long, required = true)]
        allow_write: bool,
        #[arg(long)]
        json: bool,
    },
    /// Create a structured learning note; requires explicit write opt-in.
    CreateLearning {
        vault: PathBuf,
        path: PathBuf,
        #[arg(long)]
        database: PathBuf,
        #[arg(long)]
        title: String,
        #[arg(long)]
        body: String,
        #[arg(long = "tag")]
        tags: Vec<String>,
        #[arg(long)]
        catalog_path: Option<PathBuf>,
        #[arg(long)]
        log_path: Option<PathBuf>,
        #[arg(long, required = true)]
        allow_write: bool,
        #[arg(long)]
        json: bool,
    },
    /// Replace an exact indexed section; requires explicit write opt-in.
    PatchSection {
        vault: PathBuf,
        path: PathBuf,
        #[arg(long)]
        database: PathBuf,
        #[arg(long)]
        start_line: usize,
        #[arg(long)]
        end_line: usize,
        #[arg(long)]
        expected_hash: String,
        #[arg(long)]
        replacement: String,
        #[arg(long, required = true)]
        allow_write: bool,
        #[arg(long)]
        json: bool,
    },
    /// Report source/index staleness and unresolved links.
    Doctor {
        vault: PathBuf,
        #[arg(long)]
        database: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Show the committed index generation and document counts.
    Status {
        vault: PathBuf,
        #[arg(long)]
        database: PathBuf,
        #[arg(long)]
        json: bool,
    },
}

fn print_index_report(report: &IndexReport, json: bool) -> Result<()> {
    for diagnostic in &report.diagnostics {
        eprintln!(
            "jabfa: {}: {}",
            diagnostic.path.display(),
            diagnostic.message
        );
    }
    if json {
        println!("{}", serde_json::to_string(&report)?);
    } else {
        println!(
            "Indexed {} documents and {} sections in generation {}.",
            report.documents_indexed, report.sections_indexed, report.generation
        );
    }
    std::io::stdout()
        .flush()
        .map_err(|error| JabfaError::Watch(format!("flush stdout: {error}")))?;
    Ok(())
}

fn sql_value_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => String::new(),
        serde_json::Value::String(value) => value.clone(),
        value => value.to_string(),
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("jabfa: {error}");
            ExitCode::from(1)
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Index {
            vault,
            database,
            full,
            watch,
            debounce_ms,
            json,
        } => {
            let config = IndexConfig::new(vault, database);
            if watch {
                let shutdown = Arc::new(AtomicBool::new(false));
                let signal_shutdown = Arc::clone(&shutdown);
                ctrlc::set_handler(move || {
                    signal_shutdown.store(true, Ordering::Relaxed);
                })
                .map_err(|error| JabfaError::Watch(error.to_string()))?;
                watch_vault(
                    &config,
                    Duration::from_millis(debounce_ms),
                    shutdown,
                    |report| print_index_report(&report, json),
                )?;
            } else {
                let report = if full {
                    index_vault(&config)?
                } else {
                    index_vault_incremental(&config)?
                };
                print_index_report(&report, json)?;
            }
        }
        Command::Subjects {
            vault,
            query,
            database,
            limit,
            json,
        } => {
            let response = search_subjects(&IndexConfig::new(vault, database), &query, limit)?;
            if json {
                println!("{}", serde_json::to_string(&response)?);
            } else {
                for result in response.results {
                    println!(
                        "{}\t{}\t{}\t{}\t{} section(s)",
                        result.path.display(),
                        result.title,
                        result.entry_type,
                        result.match_source,
                        result.matched_sections
                    );
                    let heading = if result.heading_path.is_empty() {
                        "Preamble".to_owned()
                    } else {
                        result.heading_path.join(" > ")
                    };
                    println!(
                        "{}:{}-{} {}",
                        result.path.display(),
                        result.start_line,
                        result.end_line,
                        heading
                    );
                    println!("{}\n", result.snippet);
                }
            }
        }
        Command::Search {
            vault,
            query,
            database,
            limit,
            json,
        } => {
            let response = search_sections(&IndexConfig::new(vault, database), &query, limit)?;
            if json {
                println!("{}", serde_json::to_string(&response)?);
            } else {
                for result in response.results {
                    let heading = if result.heading_path.is_empty() {
                        "Preamble".to_owned()
                    } else {
                        result.heading_path.join(" > ")
                    };
                    println!(
                        "{}:{}-{} {}",
                        result.path.display(),
                        result.start_line,
                        result.end_line,
                        heading
                    );
                    println!("{}\n", result.snippet);
                }
            }
        }
        Command::Context {
            vault,
            query,
            database,
            max_tokens,
            paths,
            json,
        } => {
            let config = IndexConfig::new(vault, database);
            let response = if paths.is_empty() {
                build_context(&config, &query, max_tokens)?
            } else {
                let paths = paths.iter().map(PathBuf::as_path).collect::<Vec<_>>();
                build_context_for_paths(&config, &query, &paths, max_tokens)?
            };
            if json {
                println!("{}", serde_json::to_string(&response)?);
            } else {
                print!("{}", response.context);
            }
        }
        Command::Sql {
            vault,
            query,
            database,
            max_rows,
            max_bytes,
            timeout_ms,
            json,
        } => {
            let limits = SqlQueryLimits {
                max_rows,
                max_bytes,
                timeout: Duration::from_millis(timeout_ms),
            };
            let response = query_sql(&IndexConfig::new(vault, database), &query, &limits)?;
            if json {
                println!("{}", serde_json::to_string(&response)?);
            } else {
                println!("{}", response.columns.join("\t"));
                for row in response.rows {
                    let values = row.iter().map(sql_value_text).collect::<Vec<_>>();
                    println!("{}", values.join("\t"));
                }
            }
        }
        Command::Get {
            vault,
            path,
            database,
            start_line,
            end_line,
            max_bytes,
            json,
        } => {
            let config = IndexConfig::new(vault, database);
            let response = match (start_line, end_line) {
                (Some(start_line), Some(end_line)) => {
                    get_section(&config, &path, start_line, end_line, max_bytes)?
                }
                _ => get_document(&config, &path, max_bytes)?,
            };
            if json {
                println!("{}", serde_json::to_string(&response)?);
            } else {
                print!("{}", response.content);
            }
        }
        Command::Links {
            vault,
            path,
            database,
            limit,
            json,
        } => {
            let response = list_links(&IndexConfig::new(vault, database), &path, limit)?;
            if json {
                println!("{}", serde_json::to_string(&response)?);
            } else {
                for link in response.links {
                    let target = link
                        .target_path
                        .as_deref()
                        .map(|path| path.display().to_string())
                        .unwrap_or_else(|| "-".to_owned());
                    println!(
                        "{}\t{}\t{}\t{}\t{}",
                        link.line_number, link.kind, link.raw_target, link.resolution, target
                    );
                }
            }
        }
        Command::Related {
            vault,
            path,
            database,
            max_depth,
            max_nodes,
            max_bytes,
            json,
        } => {
            let limits = RelatedLimits {
                max_depth,
                max_nodes,
                max_bytes,
            };
            let response = related_documents(&IndexConfig::new(vault, database), &path, &limits)?;
            if json {
                println!("{}", serde_json::to_string(&response)?);
            } else {
                for node in response.nodes {
                    println!("{}\t{}", node.depth, node.path.display());
                }
                for diagnostic in response.diagnostics {
                    println!(
                        "diagnostic\t{}:{}\t{}\t{}",
                        diagnostic.path.display(),
                        diagnostic.line_number,
                        diagnostic.resolution,
                        diagnostic.raw_target
                    );
                }
            }
        }
        Command::BrainCreateLearning {
            vault,
            path,
            database,
            title,
            finding,
            context,
            consequences,
            origin,
            created,
            tags,
            allow_write: _,
            json,
        } => {
            let response = create_brain_learning(
                &IndexConfig::new(vault, database),
                &BrainLearningRequest {
                    path,
                    title,
                    finding,
                    context,
                    consequences,
                    origin,
                    created,
                    tags,
                },
            )?;
            if json {
                println!("{}", serde_json::to_string(&response)?);
            } else {
                println!(
                    "Created {} and updated brain registry in generation {} ({} bytes).",
                    response.path.display(),
                    response.generation,
                    response.bytes
                );
            }
        }
        Command::CreateLearning {
            vault,
            path,
            database,
            title,
            body,
            tags,
            catalog_path,
            log_path,
            allow_write: _,
            json,
        } => {
            let config =
                IndexConfig::new(vault, database).with_generated_outputs(catalog_path, log_path);
            let response = create_learning(
                &config,
                &CreateLearningRequest {
                    path,
                    title,
                    body,
                    tags,
                },
            )?;
            if json {
                println!("{}", serde_json::to_string(&response)?);
            } else {
                println!(
                    "Created {} in generation {} ({} bytes).",
                    response.path.display(),
                    response.generation,
                    response.bytes
                );
            }
        }
        Command::PatchSection {
            vault,
            path,
            database,
            start_line,
            end_line,
            expected_hash,
            replacement,
            allow_write: _,
            json,
        } => {
            let response = patch_section(
                &IndexConfig::new(vault, database),
                &PatchSectionRequest {
                    path,
                    start_line,
                    end_line,
                    expected_hash,
                    replacement,
                },
            )?;
            if json {
                println!("{}", serde_json::to_string(&response)?);
            } else {
                println!(
                    "Patched {} in generation {} ({} bytes).",
                    response.path.display(),
                    response.generation,
                    response.bytes
                );
            }
        }
        Command::Doctor {
            vault,
            database,
            json,
        } => {
            let response = doctor(&IndexConfig::new(vault, database))?;
            if json {
                println!("{}", serde_json::to_string(&response)?);
            } else {
                println!(
                    "Generation {}: {} diagnostics.",
                    response.generation,
                    response.diagnostics.len()
                );
                for diagnostic in response.diagnostics {
                    println!(
                        "{}\t{}\t{}",
                        diagnostic.path.display(),
                        diagnostic.kind,
                        diagnostic.message
                    );
                }
            }
        }
        Command::Status {
            vault,
            database,
            json,
        } => {
            let status = index_status(&IndexConfig::new(vault, database))?;
            if json {
                println!("{}", serde_json::to_string(&status)?);
            } else {
                println!(
                    "Generation {} ({}): {} documents, {} sections.",
                    status.generation, status.state, status.documents, status.sections
                );
            }
        }
    }
    Ok(())
}
