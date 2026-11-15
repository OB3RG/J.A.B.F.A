use std::path::PathBuf;

use clap::Parser;
use jabfa::{IndexConfig, JabfaMcp};
use rmcp::ServiceExt;

#[derive(Debug, Parser)]
#[command(name = "jabfa-mcp", version, about = "JABFA local stdio MCP server")]
struct Args {
    #[arg(long)]
    vault: PathBuf,
    #[arg(long)]
    database: PathBuf,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let server = JabfaMcp::new(IndexConfig::new(args.vault, args.database))
        .serve(rmcp::transport::stdio())
        .await?;
    server.waiting().await?;
    Ok(())
}
