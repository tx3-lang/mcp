use std::env;
use dotenv::dotenv;
use rmcp::transport::sse_server::SseServer;
use tracing_subscriber::{
    layer::SubscriberExt,
    util::SubscriberInitExt,
    {self},
};

#[path = "../tools/mod.rs"]
mod tools;
use tools::protocol::ProtocolTool;

const BIND_ADDRESS: &str = "127.0.0.1:3001";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();

    let registry_url = env::var("TX3_REGISTRY_URL").expect("TX3_REGISTRY_URL must be set in the environment");
    let trp_url = env::var("TRP_URL").expect("TRP_URL must be set in the environment");
    let trp_key = env::var("TRP_KEY").expect("TRP_KEY must be set in the environment");

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "debug".to_string().into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let ct = SseServer::serve(BIND_ADDRESS.parse()?)
        .await?
        .with_service(move || ProtocolTool::new(&registry_url, &trp_url, &trp_key));

    tokio::signal::ctrl_c().await?;
    ct.cancel();
    Ok(())
}