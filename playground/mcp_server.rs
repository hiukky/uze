//! Deterministic stdio MCP server for the manually dogfoodable playground.
//! It performs no network or filesystem access: the point is to validate that
//! a real harness received an MCP capability from one UZE-managed package.

use rmcp::{
    ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
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
struct PlaygroundServer {
    tool_router: ToolRouter<Self>,
}

impl PlaygroundServer {
    fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_router]
impl PlaygroundServer {
    #[tool(description = "Echoes text exactly as supplied. Useful for proving MCP tool wiring.")]
    fn echo(&self, Parameters(args): Parameters<EchoArgs>) -> String {
        args.text
    }

    #[tool(description = "Adds two integer values and returns their exact sum.")]
    fn add(&self, Parameters(args): Parameters<AddArgs>) -> String {
        (args.left + args.right).to_string()
    }

    #[tool(description = "Returns a deterministic status value for the playground MCP server.")]
    fn status(&self, Parameters(_args): Parameters<StatusArgs>) -> String {
        "UZE_PLAYGROUND_MCP_READY".to_owned()
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for PlaygroundServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions("Deterministic playground MCP tools")
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let server = PlaygroundServer::new().serve(stdio()).await?;
    server.waiting().await?;
    Ok(())
}
