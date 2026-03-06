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
            // Echo back the client's requested protocol version for compatibility.
            // Fall back to the baseline MCP spec version if not provided.
            let client_version = req.params.as_ref()
                .and_then(|p| p.get("protocolVersion"))
                .and_then(|v| v.as_str())
                .unwrap_or("2024-11-05");

            let result = InitializeResult {
                protocol_version: client_version.into(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::protocol::JsonRpcRequest;
    use serde_json::{json, Value};

    fn test_store() -> std::sync::Arc<Store> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "dendrites_stdio_test_{}_{}.db",
            std::process::id(),
            id
        ));
        std::sync::Arc::new(Store::open(&path).unwrap())
    }

    fn make_request(method: &str, params: Option<Value>) -> JsonRpcRequest {
        JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: method.into(),
            params,
        }
    }

    #[test]
    fn test_initialize_echoes_client_version() {
        let store = test_store();
        let req = make_request(
            "initialize",
            Some(json!({"protocolVersion": "2024-11-05"})),
        );
        let resp = handle_request("/tmp/test-stdio", &store, &req);
        let result = resp.result.unwrap();
        assert_eq!(result["protocolVersion"], "2024-11-05");
        assert!(result["serverInfo"]["name"].as_str().unwrap().contains("dendrites"));
        assert!(result["capabilities"]["tools"].is_object());
        assert!(result["capabilities"]["resources"].is_object());
        assert!(result["capabilities"]["prompts"].is_object());
    }

    #[test]
    fn test_initialize_falls_back_to_baseline_version() {
        let store = test_store();
        let req = make_request("initialize", Some(json!({})));
        let resp = handle_request("/tmp/test-stdio", &store, &req);
        let result = resp.result.unwrap();
        assert_eq!(result["protocolVersion"], "2024-11-05");
    }

    #[test]
    fn test_ping_returns_empty_object() {
        let store = test_store();
        let req = make_request("ping", None);
        let resp = handle_request("/tmp/test-stdio", &store, &req);
        assert!(resp.error.is_none());
        assert_eq!(resp.result.unwrap(), json!({}));
    }

    #[test]
    fn test_unknown_method_returns_error() {
        let store = test_store();
        let req = make_request("nonexistent/method", None);
        let resp = handle_request("/tmp/test-stdio", &store, &req);
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32601);
    }

    #[test]
    fn test_tools_list_returns_all_tools() {
        let store = test_store();
        let req = make_request("tools/list", None);
        let resp = handle_request("/tmp/test-stdio", &store, &req);
        let result = resp.result.unwrap();
        let tools = result["tools"].as_array().unwrap();
        let names: Vec<&str> = tools.iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"get_model"));
        assert!(names.contains(&"model_health"));
        assert!(names.contains(&"scrutinize"));
        assert!(names.contains(&"set_model"));
        assert!(names.contains(&"refactor"));
    }

    #[test]
    fn test_prompts_list_returns_guidelines() {
        let store = test_store();
        let req = make_request("prompts/list", None);
        let resp = handle_request("/tmp/test-stdio", &store, &req);
        let result = resp.result.unwrap();
        let prompts = result["prompts"].as_array().unwrap();
        assert_eq!(prompts.len(), 1);
        assert_eq!(prompts[0]["name"], "dendrites_guidelines");
    }

    #[test]
    fn test_resources_list_returns_entries() {
        let store = test_store();
        let req = make_request("resources/list", None);
        let resp = handle_request("/tmp/test-stdio", &store, &req);
        let result = resp.result.unwrap();
        assert!(result["resources"].is_array());
    }

    #[test]
    fn test_tools_call_missing_params_returns_error() {
        let store = test_store();
        let req = make_request("tools/call", None);
        let resp = handle_request("/tmp/test-stdio", &store, &req);
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32602);
    }

    #[test]
    fn test_tools_call_model_health() {
        let store = test_store();
        let req = make_request(
            "tools/call",
            Some(json!({"name": "model_health", "arguments": {}})),
        );
        let resp = handle_request("/tmp/test-stdio", &store, &req);
        assert!(resp.error.is_none());
        let result = resp.result.unwrap();
        let content = result["content"].as_array().unwrap();
        let text = content[0]["text"].as_str().unwrap();
        let health: Value = serde_json::from_str(text).unwrap();
        assert!(health["score"].is_number());
    }

    #[test]
    fn test_prompts_get_nonexistent_returns_error() {
        let store = test_store();
        let req = make_request(
            "prompts/get",
            Some(json!({"name": "nonexistent_prompt"})),
        );
        let resp = handle_request("/tmp/test-stdio", &store, &req);
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32602);
    }
}
