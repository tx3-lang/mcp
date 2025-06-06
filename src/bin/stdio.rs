use std::env;
use dotenv::dotenv;
use anyhow::Result;
use rmcp::{ServiceExt, transport::stdio};
use tracing_subscriber::{self, EnvFilter};

#[path = "../tools/mod.rs"]
mod tools;
use tools::protocol::Protocol;

#[tokio::main]
async fn main() -> Result<()> {
    dotenv().ok();

    let trp_url = env::var("TRP_URL").expect("TRP_URL must be set in the environment");
    let trp_key = env::var("TRP_KEY").expect("TRP_KEY must be set in the environment");

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive(tracing::Level::DEBUG.into()))
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    tracing::info!("Starting MCP server");

    let service = Protocol::new(&trp_url, &trp_key).serve(stdio()).await.inspect_err(|e| {
        tracing::error!("serving error: {:?}", e);
    })?;

    service.waiting().await?;
    
    Ok(())
}