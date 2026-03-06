use serde_json::{json, Value};

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
                          layer_violations, impact_analysis, aggregate_quality, dependency_graph, \
                          field_usage, method_search, shared_fields) \
                          and arbitrary Datalog queries. All relations have a `state` column \
                          ('desired' | 'actual') for set-differencing. Relations: \
                          context, context_dep, entity, service, service_dep, event, \
                          value_object, repository, invariant, field, method, method_param, vo_rule."
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
                            "field_usage",
                            "method_search",
                            "shared_fields",
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
                    "field_type": {
                        "type": "string",
                        "description": "Field type to search for (required for field_usage)"
                    },
                    "method_name": {
                        "type": "string",
                        "description": "Method name to search for (required for method_search)"
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
    store: &Store,
    workspace_path: &str,
    name: &str,
    args: &Value,
) -> ToolCallResult {
    match name {
        "get_model" => {
            let canonical = crate::store::cozo::canonicalize_path(workspace_path);

            // Build overview from Datalog relations — no in-memory DomainRegistry
            let desired_overview = build_model_overview(store, &canonical, "desired");
            let actual_overview = build_model_overview(store, &canonical, "actual");

            let has_actual = actual_overview.get("bounded_contexts")
                .and_then(|v| v.as_array())
                .is_some_and(|a| !a.is_empty());

            // Use pure Datalog diff for sync check
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

            let overview = json!({
                "desired": desired_overview,
                "actual": if has_actual { actual_overview } else { json!(null) },
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
                "field_usage" => {
                    let field_type = match args["field_type"].as_str() {
                        Some(t) => t,
                        None => return error_result("'field_type' parameter is required for field_usage".into()),
                    };
                    match store.run_datalog(
                        &format!(
                            "?[ctx, owner_kind, owner, field_name] := \
                                *field{{workspace: $ws, context: ctx, owner_kind, owner, \
                                       name: field_name, field_type: '{}', state: 'desired'}}",
                            field_type.replace('\''  , "''")
                        ),
                        &canonical,
                    ) {
                        Ok(rows) => {
                            let items: Vec<_> = rows.iter().map(|r| json!({
                                "context": r[0], "owner_kind": r[1],
                                "owner": r[2], "field": r[3],
                            })).collect();
                            text_result(json!({
                                "analysis": "field_usage",
                                "field_type": field_type,
                                "usages": items,
                                "count": items.len(),
                            }).to_string())
                        }
                        Err(e) => error_result(format!("Field usage query failed: {e}")),
                    }
                }
                "method_search" => {
                    let method_name = match args["method_name"].as_str() {
                        Some(n) => n,
                        None => return error_result("'method_name' parameter is required for method_search".into()),
                    };
                    match store.run_datalog(
                        &format!(
                            "?[ctx, owner_kind, owner, return_type] := \
                                *method{{workspace: $ws, context: ctx, owner_kind, owner, \
                                        name: '{}', state: 'desired', return_type}}",
                            method_name.replace('\'', "''")
                        ),
                        &canonical,
                    ) {
                        Ok(rows) => {
                            let items: Vec<_> = rows.iter().map(|r| json!({
                                "context": r[0], "owner_kind": r[1],
                                "owner": r[2], "return_type": r[3],
                            })).collect();
                            text_result(json!({
                                "analysis": "method_search",
                                "method_name": method_name,
                                "matches": items,
                                "count": items.len(),
                            }).to_string())
                        }
                        Err(e) => error_result(format!("Method search query failed: {e}")),
                    }
                }
                "shared_fields" => {
                    // Find field names shared between entities and events
                    // (potential event-sourcing alignment opportunities)
                    match store.run_datalog(
                        "entity_field[ctx, owner, name, ft] := \
                            *field{workspace: $ws, context: ctx, owner_kind: 'entity', \
                                   owner, name, field_type: ft, state: 'desired'} \
                         event_field[ctx, owner, name, ft] := \
                            *field{workspace: $ws, context: ctx, owner_kind: 'event', \
                                   owner, name, field_type: ft, state: 'desired'} \
                         ?[ctx, entity, event, field_name, field_type] := \
                            entity_field[ctx, entity, field_name, field_type], \
                            event_field[ctx, event, field_name, field_type]",
                        &canonical,
                    ) {
                        Ok(rows) => {
                            let items: Vec<_> = rows.iter().map(|r| json!({
                                "context": r[0], "entity": r[1],
                                "event": r[2], "field": r[3], "type": r[4],
                            })).collect();
                            text_result(json!({
                                "analysis": "shared_fields",
                                "shared": items,
                                "count": items.len(),
                                "insight": if items.is_empty() {
                                    "No shared fields between entities and events."
                                } else {
                                    "Shared fields suggest event-sourcing alignment. Events carry entity state."
                                }
                            }).to_string())
                        }
                        Err(e) => error_result(format!("Shared fields query failed: {e}")),
                    }
                }
                _ => error_result(format!("Unknown analysis type: '{}'. Valid types: transitive_deps, circular_deps, layer_violations, impact_analysis, aggregate_quality, dependency_graph, field_usage, method_search, shared_fields, datalog", analysis)),
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
        &format!("?[ctx, name, description, aggregate_root] := \
            *entity{{workspace: $ws, context: ctx, name, description, aggregate_root, state: '{state}'}}"),
        workspace,
    ).unwrap_or_default();

    let services = store.run_datalog(
        &format!("?[ctx, name, description, kind] := \
            *service{{workspace: $ws, context: ctx, name, description, kind, state: '{state}'}}"),
        workspace,
    ).unwrap_or_default();

    let events = store.run_datalog(
        &format!("?[ctx, name, description, source] := \
            *event{{workspace: $ws, context: ctx, name, description, source, state: '{state}'}}"),
        workspace,
    ).unwrap_or_default();

    let value_objects = store.run_datalog(
        &format!("?[ctx, name, description] := \
            *value_object{{workspace: $ws, context: ctx, name, description, state: '{state}'}}"),
        workspace,
    ).unwrap_or_default();

    let repositories = store.run_datalog(
        &format!("?[ctx, name, aggregate] := \
            *repository{{workspace: $ws, context: ctx, name, aggregate, state: '{state}'}}"),
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
                    "aggregate_root": e[3] == "true",
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
                json!({"name": s[1], "description": s[2], "kind": s[3], "methods": svc_methods})
            })
            .collect();

        let ctx_events: Vec<Value> = events.iter()
            .filter(|r| r[0] == *ctx_name)
            .map(|ev| {
                let evt_fields: Vec<Value> = fields.iter()
                    .filter(|f| f[0] == *ctx_name && f[1] == "event" && f[2] == ev[1])
                    .map(|f| json!({"name": f[3], "type": f[4], "required": f[5] == "true"}))
                    .collect();
                json!({"name": ev[1], "description": ev[2], "source": ev[3], "fields": evt_fields})
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
                json!({"name": vo[1], "description": vo[2], "fields": vo_fields, "validation_rules": rules})
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
                json!({"name": repo[1], "aggregate": repo[2], "methods": repo_methods})
            })
            .collect();

        json!({
            "name": ctx_name, "description": ctx_row[1], "module": ctx_row[2],
            "entities": ctx_entities, "services": ctx_services, "events": ctx_events,
            "value_objects": ctx_vos, "repositories": ctx_repos, "depends_on": deps,
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
        let result = call_tool(&store, ws, "get_model", &json!({}));
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
        let store = test_store();
        let ws = "/tmp/test-no-actual";
        store.save_desired(ws, &test_model()).unwrap();
        let result = call_tool(&store, ws, "get_model", &json!({}));
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

        let result = call_tool(&store, ws, "scrutinize", &json!({
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

        let result = call_tool(&store, ws, "scrutinize", &json!({
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

        let result = call_tool(&store, ws, "scrutinize", &json!({
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

        let result = call_tool(&store, ws, "scrutinize", &json!({
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
        let store = test_store();
        let result = call_tool(&store, "/tmp/x", "scrutinize", &json!({
            "analysis": "transitive_deps"
        }));
        assert_eq!(result.is_error, Some(true));
    }
}
