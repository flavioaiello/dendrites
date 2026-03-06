use anyhow::Result;
use serde_json::json;
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::domain::model::DomainModel;
use crate::mcp::{protocol::*, prompts, resources, tools, write_tools};
use crate::store::Store;

/// Load the desired model from store, falling back to an empty model.
fn load_model(store: &Store, workspace_path: &str) -> DomainModel {
    store.load_desired(workspace_path).ok().flatten()
        .unwrap_or_else(|| DomainModel::empty(workspace_path))
}

/// List of write-tool names used to route `tools/call` to the mutable path.
const WRITE_TOOLS: &[&str] = &[
    "set_model",
    "refactor",
];

/// Run the MCP server over stdio (stdin/stdout), the standard transport for
/// VS Code / GitHub Copilot MCP integration.
pub async fn run(workspace_path: String, store: std::sync::Arc<Store>) -> Result<()> {
    let stdin = BufReader::new(io::stdin());
    let mut stdout = io::stdout();
    let mut lines = stdin.lines();

    tracing::info!("Dendrites stdio transport ready");

    while let Some(line) = lines.next_line().await? {
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        tracing::debug!("← {}", line);

        let request: JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let resp = JsonRpcResponse::error(None, -32700, format!("Parse error: {e}"));
                send(&mut stdout, &resp).await?;
                continue;
            }
        };

        let response = handle_request(&workspace_path, &store, &request);

        // Notifications (no id) don't get a response
        if request.id.is_some() {
            send(&mut stdout, &response).await?;
        }
    }

    Ok(())
}

fn handle_request(
    workspace_path: &str,
    store: &Store,
    req: &JsonRpcRequest,
) -> JsonRpcResponse {
    match req.method.as_str() {
        // ── Lifecycle ──────────────────────────────────────────────
        "initialize" => {
            let result = InitializeResult {
                protocol_version: "2025-03-26".into(),
                capabilities: ServerCapabilities {
                    tools: Some(ToolsCapability {}),
                    resources: Some(ResourcesCapability {}),
                    prompts: Some(PromptsCapability {}),
                },
                server_info: ServerInfo {
                    name: format!("dendrites ({})", load_model(store, workspace_path).name),
                    version: env!("CARGO_PKG_VERSION").into(),
                },
            };
            JsonRpcResponse::success(req.id.clone(), serde_json::to_value(result).unwrap())
        }

        // notifications — no response needed
        "notifications/initialized" | "initialized" => {
            JsonRpcResponse::success(req.id.clone(), json!({}))
        }

        // ── Tools ──────────────────────────────────────────────────
        "tools/list" => {
            let mut all_tools = tools::list_tools();
            all_tools.extend(write_tools::list_write_tools());
            let result = ToolsListResult { tools: all_tools };
            JsonRpcResponse::success(req.id.clone(), serde_json::to_value(result).unwrap())
        }

        "tools/call" => {
            let params: ToolCallParams = match req.params.as_ref() {
                Some(p) => match serde_json::from_value(p.clone()) {
                    Ok(p) => p,
                    Err(e) => {
                        return JsonRpcResponse::error(
                            req.id.clone(),
                            -32602,
                            format!("Invalid params: {e}"),
                        );
                    }
                },
                None => {
                    return JsonRpcResponse::error(
                        req.id.clone(),
                        -32602,
                        "Missing params",
                    );
                }
            };

            let result = if WRITE_TOOLS.contains(&params.name.as_str()) {
                write_tools::call_write_tool(workspace_path, store, &params.name, &params.arguments)
            } else {
                tools::call_tool(store, workspace_path, &params.name, &params.arguments)
            };
            JsonRpcResponse::success(req.id.clone(), serde_json::to_value(result).unwrap())
        }

        // ── Resources ──────────────────────────────────────────────
        "resources/list" => {
            let result = ResourcesListResult {
                resources: resources::list_resources(store, workspace_path),
            };
            JsonRpcResponse::success(req.id.clone(), serde_json::to_value(result).unwrap())
        }

        "resources/read" => {
            let params: ResourceReadParams = match req.params.as_ref() {
                Some(p) => match serde_json::from_value(p.clone()) {
                    Ok(p) => p,
                    Err(e) => {
                        return JsonRpcResponse::error(
                            req.id.clone(),
                            -32602,
                            format!("Invalid params: {e}"),
                        );
                    }
                },
                None => {
                    return JsonRpcResponse::error(
                        req.id.clone(),
                        -32602,
                        "Missing params",
                    );
                }
            };

            let result = resources::read_resource(store, workspace_path, &params.uri);
            JsonRpcResponse::success(req.id.clone(), serde_json::to_value(result).unwrap())
        }

        // ── Prompts ─────────────────────────────────────────────────────
        "prompts/list" => {
            let result = PromptsListResult {
                prompts: prompts::list_prompts(),
            };
            JsonRpcResponse::success(req.id.clone(), serde_json::to_value(result).unwrap())
        }

        "prompts/get" => {
            let params: PromptGetParams = match req.params.as_ref() {
                Some(p) => match serde_json::from_value(p.clone()) {
                    Ok(p) => p,
                    Err(e) => {
                        return JsonRpcResponse::error(
                            req.id.clone(),
                            -32602,
                            format!("Invalid params: {e}"),
                        );
                    }
                },
                None => {
                    return JsonRpcResponse::error(
                        req.id.clone(),
                        -32602,
                        "Missing params",
                    );
                }
            };

            let model = load_model(store, workspace_path);
            match prompts::get_prompt(&model, store, workspace_path, &params.name) {
                Some(result) => {
                    JsonRpcResponse::success(req.id.clone(), serde_json::to_value(result).unwrap())
                }
                None => JsonRpcResponse::error(
                    req.id.clone(),
                    -32602,
                    format!("Prompt not found: {}", params.name),
                ),
            }
        }

        // ── Ping (required by MCP spec) ────────────────────────────
        "ping" => JsonRpcResponse::success(req.id.clone(), json!({})),

        // ── Unknown ────────────────────────────────────────────────
        method => JsonRpcResponse::error(
            req.id.clone(),
            -32601,
            format!("Method not found: {method}"),
        ),
    }
}

async fn send(stdout: &mut io::Stdout, resp: &JsonRpcResponse) -> Result<()> {
    let json = serde_json::to_string(resp)?;
    tracing::debug!("→ {}", json);
    stdout.write_all(json.as_bytes()).await?;
    stdout.write_all(b"\n").await?;
    stdout.flush().await?;
    Ok(())
}
