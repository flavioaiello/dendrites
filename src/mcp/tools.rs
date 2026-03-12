use serde_json::{json, Value};

use crate::mcp::protocol::*;
use crate::store::Store;

/// Returns the list of tools the Dendrites server exposes.
pub fn list_tools() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "show_model".into(),
            description: "Returns the domain model for the specified state, including bounded \
                          contexts, entities, services, events, rules, and conventions. \
                          Use 'desired' to see the target model, 'actual' to see what is \
                          implemented. Shows pending changes status when viewing desired."
                .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["desired", "actual"],
                        "description": "Which model state to show (default: desired)"
                    }
                },
                "required": []
            }),
        },
        ToolDefinition {
            name: "query_model".into(),
            description: "Run an arbitrary Datalog query over the domain model knowledge graph. \
                          All relations have a `state` column ('desired' | 'actual') for \
                          set-differencing. Use $ws to reference the current workspace. \
                          Relations: context, context_dep, entity, service, service_dep, event, \
                          value_object, repository, module, invariant, field, method, method_param, vo_rule."
                .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Datalog query. Use $ws to reference the current workspace. \
                                        Example: ?[name] := *entity{workspace: $ws, name}"
                    }
                },
                "required": ["query"]
            }),
        },
    ]
}

/// Dispatches a tool call and returns the result.
pub fn call_tool(
    store: &Store,
    workspace_path: &str,
    name: &str,
    args: &Value,
) -> ToolCallResult {
    match name {
        "show_model" => {
            let action = args.get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("desired");
            let canonical = crate::store::cozo::canonicalize_path(workspace_path);

            match action {
                "desired" => {
                    let overview = build_model_overview(store, &canonical, "desired");

                    let actual_overview = build_model_overview(store, &canonical, "actual");
                    let has_actual = actual_overview.get("bounded_contexts")
                        .and_then(|v| v.as_array())
                        .is_some_and(|a| !a.is_empty());

                    let (status, pending_count) = if has_actual {
                        let changes = store.diff_graph(workspace_path).ok()
                            .and_then(|v| v.get("pending_changes").cloned())
                            .and_then(|v| v.as_array().cloned())
                            .unwrap_or_default();
                        if changes.is_empty() {
                            ("in_sync", 0)
                        } else {
                            ("pending_changes", changes.len())
                        }
                    } else {
                        ("no_actual", 0)
                    };

                    let result = json!({
                        "state": "desired",
                        "model": overview,
                        "status": status,
                        "pending_change_count": pending_count,
                    });
                    text_result(serde_json::to_string(&result).unwrap())
                }
                "actual" => {
                    let overview = build_model_overview(store, &canonical, "actual");
                    let has_actual = overview.get("bounded_contexts")
                        .and_then(|v| v.as_array())
                        .is_some_and(|a| !a.is_empty());

                    if !has_actual {
                        let result = json!({
                            "state": "actual",
                            "model": null,
                            "message": "No actual model exists. Run scan_model to extract it from source code."
                        });
                        text_result(serde_json::to_string(&result).unwrap())
                    } else {
                        let result = json!({
                            "state": "actual",
                            "model": overview,
                        });
                        text_result(serde_json::to_string(&result).unwrap())
                    }
                }
                _ => error_result(format!("Unknown action '{action}'. Use 'desired' or 'actual'.")),
            }
        }

        "query_model" => {
            let query = match args["query"].as_str() {
                Some(q) => q,
                None => return error_result("'query' parameter is required".into()),
            };
            let canonical = crate::store::cozo::canonicalize_path(workspace_path);
            match store.run_datalog_full(query, &canonical) {
                Ok((headers, rows)) => text_result(json!({
                    "headers": headers,
                    "rows": rows,
                    "row_count": rows.len(),
                }).to_string()),
                Err(e) => error_result(format!("Datalog query failed: {e}")),
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

/// Build a model overview purely from Datalog relations — replaces DomainRegistry.
pub fn build_model_overview(store: &Store, workspace: &str, state: &str) -> Value {
    // Load project metadata
    let project = store.run_datalog(
        "?[name, description, tech_stack_json, conventions_json, rules_json] := \
            *project{workspace: $ws, name, description, tech_stack_json, conventions_json, rules_json}",
        workspace,
    ).unwrap_or_default();

    let (proj_name, proj_desc, tech, conventions, rules) = if let Some(row) = project.first() {
        (
            row[0].clone(),
            row[1].clone(),
            serde_json::from_str::<Value>(&row[2]).unwrap_or(json!({})),
            serde_json::from_str::<Value>(&row[3]).unwrap_or(json!({})),
            serde_json::from_str::<Value>(&row[4]).unwrap_or(json!([])),
        )
    } else {
        return json!({});
    };

    // Query all contexts
    let contexts = store.run_datalog(
        &format!("?[name, description, module_path] := \
            *context{{workspace: $ws, name, description, module_path, state: '{state}'}}"),
        workspace,
    ).unwrap_or_default();

    let context_deps = store.run_datalog(
        &format!("?[from_ctx, to_ctx] := \
            *context_dep{{workspace: $ws, from_ctx, to_ctx, state: '{state}'}}"),
        workspace,
    ).unwrap_or_default();

    let entities = store.run_datalog(
        &format!("?[ctx, name, description, module, aggregate_root] := \
            *entity{{workspace: $ws, context: ctx, name, description, module, aggregate_root, state: '{state}'}}"),
        workspace,
    ).unwrap_or_default();

    let services = store.run_datalog(
        &format!("?[ctx, name, description, module, kind] := \
            *service{{workspace: $ws, context: ctx, name, description, module, kind, state: '{state}'}}"),
        workspace,
    ).unwrap_or_default();

    let events = store.run_datalog(
        &format!("?[ctx, name, description, module, source] := \
            *event{{workspace: $ws, context: ctx, name, description, module, source, state: '{state}'}}"),
        workspace,
    ).unwrap_or_default();

    let value_objects = store.run_datalog(
        &format!("?[ctx, name, description, module] := \
            *value_object{{workspace: $ws, context: ctx, name, description, module, state: '{state}'}}"),
        workspace,
    ).unwrap_or_default();

    let repositories = store.run_datalog(
        &format!("?[ctx, name, aggregate, module] := \
            *repository{{workspace: $ws, context: ctx, name, aggregate, module, state: '{state}'}}"),
        workspace,
    ).unwrap_or_default();

    let fields = store.run_datalog(
        &format!("?[ctx, owner_kind, owner, name, field_type, required] := \
            *field{{workspace: $ws, context: ctx, owner_kind, owner, name, field_type, required, state: '{state}'}}"),
        workspace,
    ).unwrap_or_default();

    let methods = store.run_datalog(
        &format!("?[ctx, owner_kind, owner, name, description, return_type] := \
            *method{{workspace: $ws, context: ctx, owner_kind, owner, name, description, return_type, state: '{state}'}}"),
        workspace,
    ).unwrap_or_default();

    let method_params = store.run_datalog(
        &format!("?[ctx, owner_kind, owner, method, name, param_type, required] := \
            *method_param{{workspace: $ws, context: ctx, owner_kind, owner, method, name, param_type, required, state: '{state}'}}"),
        workspace,
    ).unwrap_or_default();

    let invariants = store.run_datalog(
        &format!("?[ctx, entity, text] := \
            *invariant{{workspace: $ws, context: ctx, entity, text, state: '{state}'}}"),
        workspace,
    ).unwrap_or_default();

    let vo_rules = store.run_datalog(
        &format!("?[ctx, vo, text] := \
            *vo_rule{{workspace: $ws, context: ctx, value_object: vo, text, state: '{state}'}}"),
        workspace,
    ).unwrap_or_default();

    let modules = store.run_datalog(
        &format!("?[ctx, name, path, public, file_path, description] := \
            *module{{workspace: $ws, context: ctx, name, state: '{state}', path, public, file_path, description}}"),
        workspace,
    ).unwrap_or_default();

    // Assemble per-context JSON
    let bc_json: Vec<Value> = contexts.iter().map(|ctx_row| {
        let ctx_name = &ctx_row[0];

        let deps: Vec<&str> = context_deps.iter()
            .filter(|r| r[0] == *ctx_name)
            .map(|r| r[1].as_str())
            .collect();

        let ctx_entities: Vec<Value> = entities.iter()
            .filter(|r| r[0] == *ctx_name)
            .map(|e| {
                let ent_name = &e[1];
                let ent_fields: Vec<Value> = fields.iter()
                    .filter(|f| f[0] == *ctx_name && f[1] == "entity" && f[2] == *ent_name)
                    .map(|f| json!({"name": f[3], "type": f[4], "required": f[5] == "true"}))
                    .collect();
                let ent_methods: Vec<Value> = methods.iter()
                    .filter(|m| m[0] == *ctx_name && m[1] == "entity" && m[2] == *ent_name)
                    .map(|m| {
                        let params: Vec<Value> = method_params.iter()
                            .filter(|p| p[0] == *ctx_name && p[1] == "entity" && p[2] == *ent_name && p[3] == m[3])
                            .map(|p| json!({"name": p[4], "type": p[5], "required": p[6] == "true"}))
                            .collect();
                        json!({"name": m[3], "description": m[4], "return_type": m[5], "parameters": params})
                    })
                    .collect();
                let ent_invariants: Vec<&str> = invariants.iter()
                    .filter(|i| i[0] == *ctx_name && i[1] == *ent_name)
                    .map(|i| i[2].as_str())
                    .collect();
                json!({
                    "name": ent_name, "description": e[2],
                    "module": e[3],
                    "aggregate_root": e[4] == "true",
                    "fields": ent_fields, "methods": ent_methods,
                    "invariants": ent_invariants,
                })
            })
            .collect();

        let ctx_services: Vec<Value> = services.iter()
            .filter(|r| r[0] == *ctx_name)
            .map(|s| {
                let svc_methods: Vec<Value> = methods.iter()
                    .filter(|m| m[0] == *ctx_name && m[1] == "service" && m[2] == s[1])
                    .map(|m| {
                        let params: Vec<Value> = method_params.iter()
                            .filter(|p| p[0] == *ctx_name && p[1] == "service" && p[2] == s[1] && p[3] == m[3])
                            .map(|p| json!({"name": p[4], "type": p[5], "required": p[6] == "true"}))
                            .collect();
                        json!({"name": m[3], "description": m[4], "return_type": m[5], "parameters": params})
                    })
                    .collect();
                json!({"name": s[1], "description": s[2], "module": s[3], "kind": s[4], "methods": svc_methods})
            })
            .collect();

        let ctx_events: Vec<Value> = events.iter()
            .filter(|r| r[0] == *ctx_name)
            .map(|ev| {
                let evt_fields: Vec<Value> = fields.iter()
                    .filter(|f| f[0] == *ctx_name && f[1] == "event" && f[2] == ev[1])
                    .map(|f| json!({"name": f[3], "type": f[4], "required": f[5] == "true"}))
                    .collect();
                json!({"name": ev[1], "description": ev[2], "module": ev[3], "source": ev[4], "fields": evt_fields})
            })
            .collect();

        let ctx_vos: Vec<Value> = value_objects.iter()
            .filter(|r| r[0] == *ctx_name)
            .map(|vo| {
                let vo_fields: Vec<Value> = fields.iter()
                    .filter(|f| f[0] == *ctx_name && f[1] == "value_object" && f[2] == vo[1])
                    .map(|f| json!({"name": f[3], "type": f[4], "required": f[5] == "true"}))
                    .collect();
                let rules: Vec<&str> = vo_rules.iter()
                    .filter(|r| r[0] == *ctx_name && r[1] == vo[1])
                    .map(|r| r[2].as_str())
                    .collect();
                json!({"name": vo[1], "description": vo[2], "module": vo[3], "fields": vo_fields, "validation_rules": rules})
            })
            .collect();

        let ctx_repos: Vec<Value> = repositories.iter()
            .filter(|r| r[0] == *ctx_name)
            .map(|repo| {
                let repo_methods: Vec<Value> = methods.iter()
                    .filter(|m| m[0] == *ctx_name && m[1] == "repository" && m[2] == repo[1])
                    .map(|m| {
                        let params: Vec<Value> = method_params.iter()
                            .filter(|p| p[0] == *ctx_name && p[1] == "repository" && p[2] == repo[1] && p[3] == m[3])
                            .map(|p| json!({"name": p[4], "type": p[5], "required": p[6] == "true"}))
                            .collect();
                        json!({"name": m[3], "description": m[4], "return_type": m[5], "parameters": params})
                    })
                    .collect();
                json!({"name": repo[1], "aggregate": repo[2], "module": repo[3], "methods": repo_methods})
            })
            .collect();

        json!({
            "name": ctx_name, "description": ctx_row[1], "module": ctx_row[2],
            "entities": ctx_entities, "services": ctx_services, "events": ctx_events,
            "value_objects": ctx_vos, "repositories": ctx_repos,
            "modules": modules.iter()
                .filter(|m| m[0] == *ctx_name)
                .map(|m| json!({"name": m[1], "path": m[2], "public": m[3] == "true", "file_path": m[4], "description": m[5]}))
                .collect::<Vec<Value>>(),
            "depends_on": deps,
        })
    }).collect();

    json!({
        "project": proj_name,
        "description": proj_desc,
        "tech": tech,
        "bounded_contexts": bc_json,
        "rules": rules,
        "conventions": conventions,
    })
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
                    ownership: Ownership::default(),
                    aggregates: vec![],
                    policies: vec![],
                    read_models: vec![],
                    modules: vec![],
                    entities: vec![Entity {
                        name: "User".into(),
                        description: "A user".into(),
                        module: String::new(),
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
                        module: String::new(),
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
                    ownership: Ownership::default(),
                    aggregates: vec![],
                    policies: vec![],
                    read_models: vec![],
                    modules: vec![],
                    entities: vec![],
                    value_objects: vec![],
                    services: vec![],
                    repositories: vec![],
                    events: vec![],
                    dependencies: vec!["Identity".into()],
                },
            ],
            external_systems: vec![],
            architectural_decisions: vec![],
            ownership: Ownership::default(),
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
        let store = test_store();
        let result = call_tool(&store, "/tmp/test-tools", "nonexistent_tool", &json!({}));
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
        let result = call_tool(&store, ws, "show_model", &json!({}));
        let text = match &result.content[0] {
            ContentBlock::Text { text } => text,
        };
        let parsed: Value = serde_json::from_str(text).unwrap();
        assert_eq!(parsed["state"], "desired");
        assert!(parsed.get("model").is_some());
        assert_eq!(parsed["status"], "in_sync");
        assert_eq!(parsed["pending_change_count"], 0);

        // Also verify actual action
        let result = call_tool(&store, ws, "show_model", &json!({"action": "actual"}));
        let text = match &result.content[0] {
            ContentBlock::Text { text } => text,
        };
        let parsed: Value = serde_json::from_str(text).unwrap();
        assert_eq!(parsed["state"], "actual");
        assert!(parsed.get("model").is_some());
    }

    #[test]
    fn test_overview_no_actual_shows_status() {
        let store = test_store();
        let ws = "/tmp/test-no-actual";
        store.save_desired(ws, &test_model()).unwrap();
        let result = call_tool(&store, ws, "show_model", &json!({"action": "actual"}));
        let text = match &result.content[0] {
            ContentBlock::Text { text } => text,
        };
        let parsed: Value = serde_json::from_str(text).unwrap();
        assert_eq!(parsed["state"], "actual");
        assert_eq!(parsed["model"], json!(null));
    }

    #[test]
    fn test_query_model_circular_deps() {
        let store = test_store();
        let ws = "/tmp/test-query-circular";
        let mut model = test_model();
        if let Some(identity) = model.bounded_contexts.iter_mut().find(|c| c.name == "Identity") {
            identity.dependencies.push("Billing".into());
        }
        store.save_desired(ws, &model).unwrap();

        let result = call_tool(&store, ws, "query_model", &json!({
            "query": "?[a, b] := *context_dep{workspace: $ws, from_ctx: a, to_ctx: b, state: 'desired'}, *context_dep{workspace: $ws, from_ctx: b, to_ctx: a, state: 'desired'}"
        }));
        let text = match &result.content[0] {
            ContentBlock::Text { text } => text,
        };
        let parsed: Value = serde_json::from_str(text).unwrap();
        assert!(parsed["row_count"].as_u64().unwrap() > 0);
    }

    #[test]
    fn test_query_model_transitive_deps() {
        let store = test_store();
        let ws = "/tmp/test-query-trans";
        let mut model = test_model();
        model.bounded_contexts.push(BoundedContext {
            name: "Notifications".into(),
            description: "".into(),
            module_path: "src/notifications".into(),
            ownership: Ownership::default(),
            aggregates: vec![],
            policies: vec![],
            read_models: vec![],
            modules: vec![],
            entities: vec![],
            value_objects: vec![],
            services: vec![],
            repositories: vec![],
            events: vec![],
            dependencies: vec!["Billing".into()],
        });
        store.save_desired(ws, &model).unwrap();

        let result = call_tool(&store, ws, "query_model", &json!({
            "query": "?[to_ctx] := *context_dep{workspace: $ws, from_ctx: 'Notifications', to_ctx, state: 'desired'}"
        }));
        let text = match &result.content[0] {
            ContentBlock::Text { text } => text,
        };
        let parsed: Value = serde_json::from_str(text).unwrap();
        assert!(parsed["row_count"].as_u64().unwrap() > 0);
    }

    #[test]
    fn test_query_model_datalog() {
        let store = test_store();
        let ws = "/tmp/test-query-datalog";
        let model = test_model();
        store.save_desired(ws, &model).unwrap();

        let result = call_tool(&store, ws, "query_model", &json!({
            "query": "?[name] := *entity{workspace: $ws, name}"
        }));
        let text = match &result.content[0] {
            ContentBlock::Text { text } => text,
        };
        let parsed: Value = serde_json::from_str(text).unwrap();
        assert!(parsed["row_count"].as_u64().unwrap() > 0);
    }

    #[test]
    fn test_query_model_dependency_graph() {
        let store = test_store();
        let ws = "/tmp/test-query-graph";
        let model = test_model();
        store.save_desired(ws, &model).unwrap();

        let result = call_tool(&store, ws, "query_model", &json!({
            "query": "?[from_ctx, to_ctx] := *context_dep{workspace: $ws, from_ctx, to_ctx, state: 'desired'}"
        }));
        let text = match &result.content[0] {
            ContentBlock::Text { text } => text,
        };
        let parsed: Value = serde_json::from_str(text).unwrap();
        assert!(parsed["row_count"].as_u64().unwrap() > 0);
    }

    #[test]
    fn test_query_model_missing_param() {
        let store = test_store();
        let result = call_tool(&store, "/tmp/x", "query_model", &json!({}));
        assert_eq!(result.is_error, Some(true));
    }
}
