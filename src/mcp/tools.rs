use serde_json::{json, Value};

use crate::domain::model::DomainModel;
use crate::domain::registry::DomainRegistry;
use crate::mcp::protocol::*;
use crate::store::Store;

/// Returns the list of tools the Dendrites server exposes.
pub fn list_tools() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "get_model".into(),
            description: "Returns both the desired and actual domain models, including bounded \
                          contexts, entities, services, events, rules, and conventions. \
                          Shows pending changes status. \
                          Use this before writing any new code to understand the system structure."
                .into(),
            input_schema: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
        ToolDefinition {
            name: "scrutinize".into(),
            description: "Run Datalog-based analysis queries over the domain model knowledge graph. \
                          Supports predefined analyses (transitive_deps, circular_deps, \
                          layer_violations, impact_analysis, aggregate_quality, dependency_graph) \
                          and arbitrary Datalog queries. The domain model is decomposed into \
                          relations: context, context_dep, entity, entity_field, entity_method, \
                          method_param, invariant, service, service_dep, service_method, event, \
                          event_field, value_object, repository, arch_rule."
                .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "analysis": {
                        "type": "string",
                        "enum": [
                            "transitive_deps",
                            "circular_deps",
                            "layer_violations",
                            "impact_analysis",
                            "aggregate_quality",
                            "dependency_graph",
                            "datalog"
                        ],
                        "description": "Type of analysis to run"
                    },
                    "context": {
                        "type": "string",
                        "description": "Bounded context name (required for transitive_deps, impact_analysis)"
                    },
                    "entity": {
                        "type": "string",
                        "description": "Entity name (required for impact_analysis)"
                    },
                    "query": {
                        "type": "string",
                        "description": "Custom Datalog query (required for analysis=datalog). \
                                        Use $ws to reference the current workspace. \
                                        Example: ?[name] := *entity{workspace: $ws, name}"
                    }
                },
                "required": ["analysis"]
            }),
        },
    ]
}

/// Dispatches a tool call and returns the result.
pub fn call_tool(
    model: &DomainModel,
    store: &Store,
    workspace_path: &str,
    name: &str,
    args: &Value,
) -> ToolCallResult {
    let registry = DomainRegistry::new(model);

    match name {
        "get_model" => {
            let desired_summary = registry.architecture_summary();

            // Load actual model and produce dual-model overview
            let actual_summary = match store.load_actual(workspace_path) {
                Ok(Some(actual)) => {
                    let actual_reg = DomainRegistry::new(&actual);
                    Some(actual_reg.architecture_summary())
                }
                Ok(None) => None,
                Err(_) => None,
            };

            let (status, pending_count) = match &actual_summary {
                Some(actual_json) if actual_json == &desired_summary => {
                    ("in_sync", 0)
                }
                Some(_) => {
                    // Count changes via diff
                    let actual_model = store.load_actual(workspace_path)
                        .ok()
                        .flatten()
                        .unwrap_or_else(|| DomainModel::empty(workspace_path));
                    let changes = crate::domain::diff::diff_models(&actual_model, model);
                    ("pending_changes", changes.len())
                }
                None => ("no_actual", 0),
            };

            let overview = json!({
                "desired": serde_json::from_str::<Value>(&desired_summary).unwrap_or(json!({})),
                "actual": actual_summary
                    .as_ref()
                    .and_then(|s| serde_json::from_str::<Value>(s).ok())
                    .unwrap_or(json!(null)),
                "status": status,
                "pending_change_count": pending_count,
            });

            text_result(serde_json::to_string(&overview).unwrap())
        }

        "scrutinize" => {
            let analysis = args["analysis"].as_str().unwrap_or("");
            let canonical = crate::store::cozo::canonicalize_path(workspace_path);

            match analysis {
                "transitive_deps" => {
                    let context = match args["context"].as_str() {
                        Some(c) => c,
                        None => return error_result("'context' parameter is required for transitive_deps".into()),
                    };
                    match store.transitive_deps(&canonical, context) {
                        Ok(deps) => text_result(json!({
                            "analysis": "transitive_deps",
                            "context": context,
                            "dependencies": deps,
                            "count": deps.len(),
                        }).to_string()),
                        Err(e) => error_result(format!("Transitive deps query failed: {}", e)),
                    }
                }
                "circular_deps" => {
                    match store.circular_deps(&canonical) {
                        Ok(cycles) => {
                            let cycle_pairs: Vec<_> = cycles.iter()
                                .map(|(a, b)| json!({"from": a, "to": b}))
                                .collect();
                            text_result(json!({
                                "analysis": "circular_deps",
                                "cycles": cycle_pairs,
                                "has_cycles": !cycles.is_empty(),
                            }).to_string())
                        }
                        Err(e) => error_result(format!("Circular deps query failed: {}", e)),
                    }
                }
                "layer_violations" => {
                    match store.layer_violations(&canonical) {
                        Ok(violations) => {
                            let items: Vec<_> = violations.iter()
                                .map(|(ctx, svc, dep)| json!({
                                    "context": ctx,
                                    "domain_service": svc,
                                    "infrastructure_dependency": dep,
                                }))
                                .collect();
                            text_result(json!({
                                "analysis": "layer_violations",
                                "violations": items,
                                "count": violations.len(),
                            }).to_string())
                        }
                        Err(e) => error_result(format!("Layer violations query failed: {}", e)),
                    }
                }
                "impact_analysis" => {
                    let context = match args["context"].as_str() {
                        Some(c) => c,
                        None => return error_result("'context' parameter is required for impact_analysis".into()),
                    };
                    let entity = match args["entity"].as_str() {
                        Some(e) => e,
                        None => return error_result("'entity' parameter is required for impact_analysis".into()),
                    };
                    match store.impact_analysis(&canonical, context, entity) {
                        Ok(result) => text_result(json!({
                            "analysis": "impact_analysis",
                            "result": result,
                        }).to_string()),
                        Err(e) => error_result(format!("Impact analysis query failed: {}", e)),
                    }
                }
                "aggregate_quality" => {
                    match store.aggregate_roots_without_invariants(&canonical) {
                        Ok(roots) => {
                            let items: Vec<_> = roots.iter()
                                .map(|(ctx, ent)| json!({"context": ctx, "entity": ent}))
                                .collect();
                            text_result(json!({
                                "analysis": "aggregate_quality",
                                "aggregate_roots_without_invariants": items,
                                "count": roots.len(),
                                "recommendation": if roots.is_empty() {
                                    "All aggregate roots have invariants defined."
                                } else {
                                    "Consider adding domain invariants to protect these aggregate roots."
                                },
                            }).to_string())
                        }
                        Err(e) => error_result(format!("Aggregate quality query failed: {}", e)),
                    }
                }
                "dependency_graph" => {
                    match store.dependency_graph(&canonical) {
                        Ok(graph) => text_result(json!({
                            "analysis": "dependency_graph",
                            "graph": graph,
                        }).to_string()),
                        Err(e) => error_result(format!("Dependency graph query failed: {}", e)),
                    }
                }
                "datalog" => {
                    let query = match args["query"].as_str() {
                        Some(q) => q,
                        None => return error_result("'query' parameter is required for datalog analysis".into()),
                    };
                    match store.run_datalog_full(query, &canonical) {
                        Ok((headers, rows)) => text_result(json!({
                            "analysis": "datalog",
                            "headers": headers,
                            "rows": rows,
                            "row_count": rows.len(),
                        }).to_string()),
                        Err(e) => error_result(format!("Datalog query failed: {}", e)),
                    }
                }
                _ => error_result(format!("Unknown analysis type: '{}'. Valid types: transitive_deps, circular_deps, layer_violations, impact_analysis, aggregate_quality, dependency_graph, datalog", analysis)),
            }
        }

        _ => error_result(format!("Unknown tool: {}", name)),
    }
}

fn text_result(text: String) -> ToolCallResult {
    ToolCallResult {
        content: vec![ContentBlock::Text { text }],
        is_error: None,
    }
}

fn error_result(msg: String) -> ToolCallResult {
    ToolCallResult {
        content: vec![ContentBlock::Text { text: msg }],
        is_error: Some(true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::*;
    use std::env::temp_dir;

    fn test_store() -> Store {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = temp_dir()
            .join(format!("dendrites_tools_test_{}_{}.db", std::process::id(), id));
        Store::open(&path).unwrap()
    }

    fn test_model() -> DomainModel {
        DomainModel {
            name: "TestProject".into(),
            description: "Test".into(),
            bounded_contexts: vec![
                BoundedContext {
                    name: "Identity".into(),
                    description: "Auth context".into(),
                    module_path: "src/identity".into(),
                    entities: vec![Entity {
                        name: "User".into(),
                        description: "A user".into(),
                        aggregate_root: true,
                        fields: vec![Field {
                            name: "id".into(),
                            field_type: "UserId".into(),
                            required: true,
                            description: "".into(),
                        }],
                        methods: vec![],
                        invariants: vec!["Email must be unique".into()],
                    }],
                    value_objects: vec![],
                    services: vec![Service {
                        name: "AuthService".into(),
                        description: "Handles auth".into(),
                        kind: ServiceKind::Application,
                        methods: vec![],
                        dependencies: vec![],
                    }],
                    repositories: vec![],
                    events: vec![],
                    dependencies: vec![],
                },
                BoundedContext {
                    name: "Billing".into(),
                    description: "Billing context".into(),
                    module_path: "src/billing".into(),
                    entities: vec![],
                    value_objects: vec![],
                    services: vec![],
                    repositories: vec![],
                    events: vec![],
                    dependencies: vec!["Identity".into()],
                },
            ],
            rules: vec![ArchitecturalRule {
                id: "LAYER-001".into(),
                description: "Domain must not depend on infra".into(),
                severity: Severity::Error,
                scope: "domain".into(),
            }],
            tech_stack: TechStack::default(),
            conventions: Conventions {
                file_structure: FileStructure {
                    pattern: "src/{context}/{layer}/{type}.rs".into(),
                    layers: vec!["domain".into(), "application".into()],
                },
                ..Default::default()
            },
        }
    }

    #[test]
    fn test_unknown_tool() {
        let model = test_model();
        let store = test_store();
        let result = call_tool(&model, &store, "/tmp/test-tools", "nonexistent_tool", &json!({}));
        assert_eq!(result.is_error, Some(true));
    }

    #[test]
    fn test_list_tools_count() {
        let tools = list_tools();
        assert_eq!(tools.len(), 2);
    }

    #[test]
    fn test_overview_shows_desired_and_actual() {
        let model = test_model();
        let store = test_store();
        let ws = "/tmp/test-dual-overview";
        // Save desired + accept to create actual
        store.save_desired(ws, &model).unwrap();
        store.accept(ws).unwrap();
        let result = call_tool(&model, &store, ws, "get_model", &json!({}));
        let text = match &result.content[0] {
            ContentBlock::Text { text } => text,
        };
        let parsed: Value = serde_json::from_str(text).unwrap();
        assert!(parsed.get("desired").is_some());
        assert!(parsed.get("actual").is_some());
        assert_eq!(parsed["status"], "in_sync");
        assert_eq!(parsed["pending_change_count"], 0);
    }

    #[test]
    fn test_overview_no_actual_shows_status() {
        let model = test_model();
        let store = test_store();
        let result = call_tool(&model, &store, "/tmp/test-no-actual", "get_model", &json!({}));
        let text = match &result.content[0] {
            ContentBlock::Text { text } => text,
        };
        let parsed: Value = serde_json::from_str(text).unwrap();
        assert_eq!(parsed["status"], "no_actual");
        assert_eq!(parsed["actual"], json!(null));
    }

    #[test]
    fn test_query_model_circular_deps() {
        let store = test_store();
        let ws = "/tmp/test-query-circular";
        let mut model = test_model();
        // Create circular dependency: Identity → Billing → Identity
        // Billing already depends on Identity; add Identity → Billing
        if let Some(identity) = model.bounded_contexts.iter_mut().find(|c| c.name == "Identity") {
            identity.dependencies.push("Billing".into());
        }
        store.save_desired(ws, &model).unwrap();

        let result = call_tool(&model, &store, ws, "scrutinize", &json!({
            "analysis": "circular_deps"
        }));
        let text = match &result.content[0] {
            ContentBlock::Text { text } => text,
        };
        let parsed: Value = serde_json::from_str(text).unwrap();
        assert_eq!(parsed["analysis"], "circular_deps");
        assert_eq!(parsed["has_cycles"], true);
    }

    #[test]
    fn test_query_model_transitive_deps() {
        let store = test_store();
        let ws = "/tmp/test-query-trans";
        let mut model = test_model();
        // Add Notifications context depending on Billing
        // Billing already depends on Identity, so transitive from Billing: Identity
        // Then Notifications depends on Billing, so from Notifications: Billing, Identity
        model.bounded_contexts.push(BoundedContext {
            name: "Notifications".into(),
            description: "".into(),
            module_path: "src/notifications".into(),
            entities: vec![],
            value_objects: vec![],
            services: vec![],
            repositories: vec![],
            events: vec![],
            dependencies: vec!["Billing".into()],
        });
        store.save_desired(ws, &model).unwrap();

        let result = call_tool(&model, &store, ws, "scrutinize", &json!({
            "analysis": "transitive_deps",
            "context": "Notifications"
        }));
        let text = match &result.content[0] {
            ContentBlock::Text { text } => text,
        };
        let parsed: Value = serde_json::from_str(text).unwrap();
        assert_eq!(parsed["analysis"], "transitive_deps");
        // Notifications → Billing → Identity (transitive)
        let deps = parsed["dependencies"].as_array().unwrap();
        assert!(deps.contains(&json!("Billing")));
        assert!(deps.contains(&json!("Identity")));
    }

    #[test]
    fn test_query_model_datalog() {
        let store = test_store();
        let ws = "/tmp/test-query-datalog";
        let model = test_model();
        store.save_desired(ws, &model).unwrap();

        let result = call_tool(&model, &store, ws, "scrutinize", &json!({
            "analysis": "datalog",
            "query": "?[name] := *entity{workspace: $ws, name}"
        }));
        let text = match &result.content[0] {
            ContentBlock::Text { text } => text,
        };
        let parsed: Value = serde_json::from_str(text).unwrap();
        assert_eq!(parsed["analysis"], "datalog");
        assert!(parsed["row_count"].as_u64().unwrap() > 0);
    }

    #[test]
    fn test_query_model_dependency_graph() {
        let store = test_store();
        let ws = "/tmp/test-query-graph";
        let model = test_model();
        store.save_desired(ws, &model).unwrap();

        let result = call_tool(&model, &store, ws, "scrutinize", &json!({
            "analysis": "dependency_graph"
        }));
        let text = match &result.content[0] {
            ContentBlock::Text { text } => text,
        };
        let parsed: Value = serde_json::from_str(text).unwrap();
        assert_eq!(parsed["analysis"], "dependency_graph");
        assert!(parsed["graph"].get("nodes").is_some());
        assert!(parsed["graph"].get("edges").is_some());
    }

    #[test]
    fn test_query_model_missing_param() {
        let model = test_model();
        let store = test_store();
        let result = call_tool(&model, &store, "/tmp/x", "scrutinize", &json!({
            "analysis": "transitive_deps"
        }));
        assert_eq!(result.is_error, Some(true));
    }
}
