//! Minimal, deterministic stdio MCP server used only for UZE's opt-in MCP
//! conformance tests (see ADR-007). Exposes exactly one tool,
//! `uze_conformance`, returning a value the *test* controls via the
//! `UZE_MCP_CONFORMANCE_PROOF` environment variable — not a hardcoded
//! constant, and deliberately not shared with the Agent Skills `PROOF`
//! token, so the two conformance suites stay independently verifiable.
//! No network, database, external API, credentials, or LLM involved.

use rmcp::{
    ServerHandler, ServiceExt,
    model::{ServerCapabilities, ServerInfo},
    tool,
    transport::io::stdio,
};

#[derive(Debug, Clone)]
struct ConformanceServer {
    proof: String,
}

/// No fields are read; the tool takes no meaningful input. A single optional
/// field exists only so the generated JSON Schema has a normal `properties`
/// object instead of the field-less shape some MCP clients drop tools for.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct NoArgs {
    #[allow(dead_code)]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    unused: Option<String>,
}

#[tool(tool_box)]
impl ConformanceServer {
    #[tool(description = "Returns the UZE MCP conformance proof value")]
    fn uze_conformance(&self, #[tool(aggr)] _args: NoArgs) -> String {
        self.proof.clone()
    }
}

#[tool(tool_box)]
impl ServerHandler for ConformanceServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            // Some MCP clients only attempt tool discovery when the server
            // explicitly advertises tool support here, even though this
            // server always answers `tools/list`/`tools/call` regardless.
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            instructions: Some("UZE MCP conformance fixture".into()),
            ..Default::default()
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let proof = std::env::var("UZE_MCP_CONFORMANCE_PROOF")
        .unwrap_or_else(|_| "UZE_MCP_CONFORMANCE_DEFAULT_PROOF".to_owned());
    let server = ConformanceServer { proof }.serve(stdio()).await?;
    server.waiting().await?;
    Ok(())
}
