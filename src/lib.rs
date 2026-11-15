//! JABFA: Just A Brain For AI.
//!
//! This crate is being developed test-first. See the repository test plan for
//! the public behavior currently covered by acceptance tests.

mod brain;
mod context;
mod doctor;
mod error;
mod index;
mod links;
mod mcp;
mod model;
mod navigation;
mod parser;
mod query;
mod sql;
mod watch;
mod writes;

/// Version of the shared machine-readable success-response schemas.
pub const OUTPUT_SCHEMA_VERSION: u32 = 1;

pub use brain::{BrainLearningRequest, create_brain_learning};
pub use context::{ContextResponse, build_context};
pub use doctor::{DoctorResponse, doctor};
pub use error::{JabfaError, Result};
pub use index::{
    IndexConfig, IndexDiagnostic, IndexReport, IndexStatus, index_status, index_vault,
    index_vault_incremental,
};
pub use mcp::JabfaMcp;
pub use model::{Link, LinkKind, ParsedDocument, Section, Task};
pub use navigation::{
    LinkEntry, LinksResponse, RelatedDiagnostic, RelatedLimits, RelatedNode, RelatedResponse,
    SourceResponse, get_document, get_section, list_links, related_documents,
};
pub use parser::parse_markdown;
pub use query::{SearchResponse, SearchResult, search_sections};
pub use sql::{SqlQueryLimits, SqlResponse, query_sql};
pub use watch::watch_vault;
pub use writes::{
    CreateLearningRequest, PatchSectionRequest, WriteResponse, create_learning, patch_section,
};
