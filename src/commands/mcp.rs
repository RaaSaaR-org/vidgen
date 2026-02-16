use crate::error::{VidgenError, VidgenResult};
use crate::mcp::McServer;
use rmcp::ServiceExt;

pub async fn run() -> VidgenResult<()> {
    // Send tracing output to stderr so it doesn't corrupt the MCP stdio JSON-RPC channel.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing_subscriber::filter::LevelFilter::INFO.into()),
        )
        .with_writer(std::io::stderr)
        .init();

    let server = McServer::new();
    let service = server
        .serve(rmcp::transport::stdio())
        .await
        .map_err(|e| VidgenError::Other(format!("MCP server error: {e}")))?;
    service
        .waiting()
        .await
        .map_err(|e| VidgenError::Other(format!("MCP server error: {e}")))?;
    Ok(())
}
