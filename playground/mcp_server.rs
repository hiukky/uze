//! Deterministic stdio MCP server for the manually dogfoodable UZE playground.
//! It performs no network or filesystem access: the point is to validate that
//! a real harness received an MCP capability from one UZE-managed package.

use rmcp::{
    ServerHandler, ServiceExt,
    model::{ServerCapabilities, ServerInfo},
    tool,
    transport::io::stdio,
};

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct EchoArgs {
    text: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct AddArgs {
    left: i64,
    right: i64,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct StatusArgs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[allow(dead_code)]
    unused: Option<String>,
}

#[derive(Debug, Clone)]
struct PlaygroundServer;

#[tool(tool_box)]
impl PlaygroundServer {
    #[tool(description = "Echoes text exactly as supplied. Useful for proving MCP tool wiring.")]
    fn echo(&self, #[tool(aggr)] args: EchoArgs) -> String {
        args.text
    }

    #[tool(description = "Adds two integer values and returns their exact sum.")]
    fn add(&self, #[tool(aggr)] args: AddArgs) -> String {
        (args.left + args.right).to_string()
    }

    #[tool(description = "Returns a deterministic status value for the UZE playground MCP server.")]
    fn status(&self, #[tool(aggr)] _args: StatusArgs) -> String {
        "UZE_PLAYGROUND_MCP_READY".to_owned()
    }
}

#[tool(tool_box)]
impl ServerHandler for PlaygroundServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            instructions: Some("Deterministic UZE playground MCP tools".into()),
            ..Default::default()
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let server = PlaygroundServer.serve(stdio()).await?;
    server.waiting().await?;
    Ok(())
}
