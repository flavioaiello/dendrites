use serde_json::{Value, json};

use crate::domain::model::*;
use crate::domain::to_snake;
use crate::mcp::protocol::*;
use crate::store::Store;

/// Returns the list of write tools the Dendrites server exposes.
pub fn list_write_tools() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "define".into(),
            description: "Create, update, or remove architecture elements: modules, entities, \
                          services, events, value objects, aggregates, repositories, and more. \
                          As you process code or conversation, call this whenever you discover \
                          structural relationships. \
                          Fields, methods, and invariants are merged (not replaced). \
                          All changes are auto-saved. \
                          Returns suggested file paths for created/updated artifacts. \
                          To remove an element, set action to 'remove'."
                .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "kind": {
                        "type": "string",
                        "enum": ["bounded_context", "aggregate", "entity", "policy", "read_model", "service", "event", "value_object", "repository", "external_system", "architectural_decision"],
                        "description": "Type of model element to create/update/remove"
                    },
                    "action": {
                        "type": "string",
                        "enum": ["upsert", "remove"],
                        "description": "Whether to create/update or remove the element (default: upsert)"
                    },
                    "context": { "type": "string", "description": "Bounded context name (required for entity, service, event)" },
                    "name": { "type": "string", "description": "Element name" },
                    "description": { "type": "string" },
                    "module_path": { "type": "string", "description": "Module path (bounded_context only)" },
                    "dependencies": {
                        "type": "array", "items": { "type": "string" },
                        "description": "Dependencies (bounded_context: allowed context deps; service: service deps)"
                    },
                    "ownership": {
                        "type": "object",
                        "properties": {
                            "team": { "type": "string" },
                            "owners": { "type": "array", "items": { "type": "string" } },
                            "rationale": { "type": "string" }
                        },
                        "description": "Ownership/team metadata"
                    },
                    "root_entity": { "type": "string", "description": "Aggregate root entity name" },
                    "entities": { "type": "array", "items": { "type": "string" }, "description": "Aggregate entity members" },
                    "value_objects": { "type": "array", "items": { "type": "string" }, "description": "Aggregate value object members" },
                    "policy_kind": { "type": "string", "enum": ["domain", "process_manager", "integration"], "description": "Policy classification" },
                    "triggers": { "type": "array", "items": { "type": "string" }, "description": "Policy trigger events" },
                    "commands": { "type": "array", "items": { "type": "string" }, "description": "Policy emitted commands" },
                    "consumed_by_contexts": { "type": "array", "items": { "type": "string" }, "description": "Contexts integrating with an external system" },
                    "kind_label": { "type": "string", "description": "Free-form kind label for external systems" },
                    "rationale": { "type": "string", "description": "Architectural rationale or boundary rationale" },
                    "title": { "type": "string", "description": "Architectural decision title" },
                    "status": { "type": "string", "enum": ["proposed", "accepted", "superseded", "deprecated"], "description": "Decision lifecycle status" },
                    "scope": { "type": "string", "description": "Decision scope" },
                    "date": { "type": "string", "description": "Decision date" },
                    "contexts": { "type": "array", "items": { "type": "string" }, "description": "Related bounded contexts" },
                    "consequences": { "type": "array", "items": { "type": "string" }, "description": "Decision consequences" },
                    "aggregate_root": { "type": "boolean", "description": "Entity only" },
                    "fields": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "name": { "type": "string" },
                                "type": { "type": "string" },
                                "required": { "type": "boolean" },
                                "description": { "type": "string" }
                            },
                            "required": ["name", "type"]
                        },
                        "description": "Fields (entity, event, value_object, read_model)"
                    },
                    "methods": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "name": { "type": "string" },
                                "description": { "type": "string" },
                                "parameters": {
                                    "type": "array",
                                    "items": {
                                        "type": "object",
                                        "properties": {
                                            "name": { "type": "string" },
                                            "type": { "type": "string" }
                                        },
                                        "required": ["name", "type"]
                                    }
                                },
                                "return_type": { "type": "string" }
                            },
                            "required": ["name"]
                        },
                        "description": "Methods (entity, service, repository)"
                    },
                    "invariants": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Invariants (entity only)"
                    },
                    "service_kind": {
                        "type": "string",
                        "enum": ["domain", "application", "infrastructure"],
                        "description": "Service layer classification (service only)"
                    },
                    "source": { "type": "string", "description": "Source entity (event only)" },
                    "validation_rules": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Validation rules (value_object only)"
                    },
                    "aggregate": { "type": "string", "description": "Aggregate entity name (repository only)" }
                },
                "required": ["kind", "name"]
            }),
        },
        ToolDefinition {
            name: "sync".into(),
            description: "Scan the workspace source code and populate the current architecture \
                          model from what is actually implemented. Extracts modules, structs, \
                          functions, imports, and call graphs from source files. \
                          Usually runs automatically via the file watcher."
                .into(),
            input_schema: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
        ToolDefinition {
            name: "refactor".into(),
            description: "Manage the refactoring lifecycle between planned and current architecture. \
                          Actions: \
                          'diagnose' — run full analysis (health score, all checks, drift, AST statistics) \
                          and return a prioritized action plan. Use this to start or continue improvement. \
                          'plan' (default) — compare current vs planned and produce a refactoring plan \
                          with code actions, file paths, priorities, and migration notes. \
                          'accept' — after implementing, promote planned → current. \
                          'reset' — discard planned changes, revert to current state."
                .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["diagnose", "plan", "accept", "reset"],
                        "description": "Refactoring lifecycle action (default: plan)"
                    }
                },
                "required": []
            }),
        },
        ToolDefinition {
            name: "constrain".into(),
            description: "Declare and evaluate architectural constraints. \
                          Assign modules to layers (e.g., domain, application, infrastructure), \
                          declare forbidden or allowed dependencies between layers or modules, \
                          list current constraints, or evaluate all constraints to find violations."
                .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["assign_layer", "remove_layer", "add_constraint", "remove_constraint", "list", "evaluate"],
                        "description": "Policy action to perform"
                    },
                    "context": { "type": "string", "description": "Bounded context name (for assign_layer/remove_layer)" },
                    "layer": { "type": "string", "description": "Layer name, e.g. 'domain', 'application', 'infrastructure' (for assign_layer)" },
                    "constraint_kind": {
                        "type": "string",
                        "enum": ["layer", "context"],
                        "description": "Whether the constraint applies to layers or specific contexts (for add_constraint/remove_constraint)"
                    },
                    "source": { "type": "string", "description": "Source layer or context name (for constraints)" },
                    "target": { "type": "string", "description": "Target layer or context name (for constraints)" },
                    "rule": {
                        "type": "string",
                        "enum": ["forbidden", "allowed"],
                        "description": "Whether the dependency is forbidden or explicitly allowed (default: forbidden)"
                    }
                },
                "required": ["action"]
            }),
        },
    ]
}

/// Dispatches a write tool call.
pub fn call_write_tool(
    workspace_path: &str,
    store: &Store,
    name: &str,
    args: &Value,
) -> ToolCallResult {
    dispatch_write_tool(workspace_path, store, name, args)
}

fn dispatch_write_tool(
    workspace_path: &str,
    store: &Store,
    name: &str,
    args: &Value,
) -> ToolCallResult {
    match name {
        "define" => {
            let kind = arg_str(args, "kind");
            let action = args
                .get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("upsert");

            match (kind.as_str(), action) {
                ("bounded_context", "upsert") => {
                    upsert_bounded_context(store, workspace_path, args)
                }
                ("bounded_context", "remove") => {
                    remove_bounded_context(store, workspace_path, args)
                }
                ("entity", "upsert") => upsert_entity(store, workspace_path, args),
                ("entity", "remove") => remove_entity(store, workspace_path, args),
                ("service", "upsert") => upsert_service(store, workspace_path, args),
                ("service", "remove") => remove_service(store, workspace_path, args),
                ("event", "upsert") => upsert_event(store, workspace_path, args),
                ("event", "remove") => remove_event(store, workspace_path, args),
                ("value_object", "upsert") => upsert_value_object(store, workspace_path, args),
                ("value_object", "remove") => remove_value_object(store, workspace_path, args),
                ("repository", "upsert") => upsert_repository(store, workspace_path, args),
                ("repository", "remove") => remove_repository(store, workspace_path, args),
                ("aggregate", "upsert") => upsert_aggregate(store, workspace_path, args),
                ("aggregate", "remove") => remove_aggregate(store, workspace_path, args),
                ("policy", "upsert") => upsert_policy(store, workspace_path, args),
                ("policy", "remove") => remove_policy(store, workspace_path, args),
                ("read_model", "upsert") => upsert_read_model(store, workspace_path, args),
                ("read_model", "remove") => remove_read_model(store, workspace_path, args),
                ("external_system", "upsert") => {
                    upsert_external_system(store, workspace_path, args)
                }
                ("external_system", "remove") => {
                    remove_external_system(store, workspace_path, args)
                }
                ("architectural_decision", "upsert") => {
                    upsert_architectural_decision(store, workspace_path, args)
                }
                ("architectural_decision", "remove") => {
                    remove_architectural_decision(store, workspace_path, args)
                }
                ("", _) => error_result("'kind' is required"),
                (_, action) => error_result(format!("Unknown action '{action}' for kind '{kind}'")),
            }
        }

        "sync" => {
            use crate::domain::analyze::scan_actual_model;

            let workspace_root = std::path::Path::new(workspace_path);
            let desired = store.load_desired(workspace_path).ok().flatten();

            match scan_actual_model(workspace_root, desired.as_ref()) {
                Ok(actual) => {
                    let entity_count: usize = actual
                        .bounded_contexts
                        .iter()
                        .map(|bc| bc.entities.len())
                        .sum();
                    let vo_count: usize = actual
                        .bounded_contexts
                        .iter()
                        .map(|bc| bc.value_objects.len())
                        .sum();
                    let svc_count: usize = actual
                        .bounded_contexts
                        .iter()
                        .map(|bc| bc.services.len())
                        .sum();
                    let repo_count: usize = actual
                        .bounded_contexts
                        .iter()
                        .map(|bc| bc.repositories.len())
                        .sum();
                    let event_count: usize = actual
                        .bounded_contexts
                        .iter()
                        .map(|bc| bc.events.len())
                        .sum();

                    match store.save_actual(workspace_path, &actual) {
                        Ok(()) => {
                            if store.load_desired(workspace_path).ok().flatten().is_none() {
                                let _ = store.save_desired(workspace_path, &actual);
                            }
                            let _ = store.compute_drift(workspace_path);
                            text_result(
                                json!({
                                    "status": "scanned",
                                    "message": format!(
                                        "Scanned {} contexts → {} entities, {} VOs, {} services, {} repos, {} events. Actual model updated.",
                                        actual.bounded_contexts.len(), entity_count, vo_count, svc_count, repo_count, event_count
                                    ),
                                    "contexts_scanned": actual.bounded_contexts.len(),
                                    "entities": entity_count,
                                    "value_objects": vo_count,
                                    "services": svc_count,
                                    "repositories": repo_count,
                                    "events": event_count,
                                })
                                .to_string(),
                            )
                        }
                        Err(e) => error_result(format!("Scan succeeded but save failed: {e}")),
                    }
                }
                Err(e) => error_result(format!("Scan failed: {e}")),
            }
        }

        "refactor" => {
            let action = args
                .get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("plan");

            match action {
                "diagnose" => {
                    diagnose_pipeline(store, workspace_path)
                }

                "plan" => {
                    // PHASE 3 GRAPH MIGRATION: Delegate diffing into Datalog
                    match store.diff_graph(workspace_path) {
                        Ok(diff_data) => {
                            let changes = diff_data["pending_changes"].as_array().unwrap();
                            if changes.is_empty() {
                                text_result(
                                    json!({
                                        "status": "in_sync",
                                        "message": "Current and planned architecture are in sync. Nothing to refactor."
                                    })
                                    .to_string(),
                                )
                            } else {
                                // Enrich diff with module paths, file suggestions, and priorities
                                let enriched = enrich_plan(store, workspace_path, changes);
                                text_result(serde_json::to_string_pretty(&enriched).unwrap())
                            }
                        }
                        Err(e) => error_result(format!("Diff generation failed: {e}")),
                    }
                }

                "accept" => {
                    match store.accept(workspace_path) {
                        Ok(()) => {
                            let _ = store.compute_drift(workspace_path);
                            text_result(
                                json!({
                                    "status": "accepted",
                                    "message": "Planned architecture promoted to current. Architecture is now in sync."
                                })
                                .to_string(),
                            )
                        }
                        Err(e) => error_result(format!("Failed to accept: {e}")),
                    }
                }

                "reset" => {
                    match store.reset(workspace_path) {
                        Ok(Some(_)) => text_result(
                            json!({
                                "status": "reset",
                                "message": "Planned architecture reverted to current. All pending changes discarded."
                            })
                            .to_string(),
                        ),
                        Ok(None) => error_result("No current architecture to reset to"),
                        Err(e) => error_result(format!("Failed to reset: {e}")),
                    }
                }

                _ => error_result(format!("Unknown action '{action}'. Use 'diagnose', 'plan', 'accept', or 'reset'.")),
            }
        }

        "constrain" => {
            let action = args
                .get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("list");

            match action {
                "assign_layer" => {
                    let context = arg_str(args, "context");
                    let layer = arg_str(args, "layer");
                    if context.is_empty() {
                        return error_result("'context' is required for assign_layer");
                    }
                    if layer.is_empty() {
                        return error_result("'layer' is required for assign_layer");
                    }
                    match store.upsert_layer_assignment(workspace_path, &context, &layer) {
                        Ok(()) => text_result(json!({
                            "message": format!("Assigned context '{}' to layer '{}'", context, layer),
                        }).to_string()),
                        Err(e) => error_result(format!("Failed to assign layer: {e}")),
                    }
                }

                "remove_layer" => {
                    let context = arg_str(args, "context");
                    if context.is_empty() {
                        return error_result("'context' is required for remove_layer");
                    }
                    match store.remove_layer_assignment(workspace_path, &context) {
                        Ok(true) => {
                            text_result(format!("Removed layer assignment for context '{context}'"))
                        }
                        Ok(false) => error_result(format!(
                            "No layer assignment found for context '{context}'"
                        )),
                        Err(e) => error_result(format!("Failed to remove layer assignment: {e}")),
                    }
                }

                "add_constraint" => {
                    let constraint_kind = arg_str(args, "constraint_kind");
                    let source = arg_str(args, "source");
                    let target = arg_str(args, "target");
                    let rule = args
                        .get("rule")
                        .and_then(|v| v.as_str())
                        .unwrap_or("forbidden");
                    if constraint_kind.is_empty() || source.is_empty() || target.is_empty() {
                        return error_result(
                            "'constraint_kind', 'source', and 'target' are required for add_constraint",
                        );
                    }
                    if constraint_kind != "layer" && constraint_kind != "context" {
                        return error_result("'constraint_kind' must be 'layer' or 'context'");
                    }
                    if rule != "forbidden" && rule != "allowed" {
                        return error_result("'rule' must be 'forbidden' or 'allowed'");
                    }
                    match store.upsert_dependency_constraint(workspace_path, &constraint_kind, &source, &target, rule) {
                        Ok(()) => text_result(json!({
                            "message": format!("{} dependency: {} → {} ({})", rule, source, target, constraint_kind),
                        }).to_string()),
                        Err(e) => error_result(format!("Failed to add constraint: {e}")),
                    }
                }

                "remove_constraint" => {
                    let constraint_kind = arg_str(args, "constraint_kind");
                    let source = arg_str(args, "source");
                    let target = arg_str(args, "target");
                    if constraint_kind.is_empty() || source.is_empty() || target.is_empty() {
                        return error_result(
                            "'constraint_kind', 'source', and 'target' are required for remove_constraint",
                        );
                    }
                    match store.remove_dependency_constraint(
                        workspace_path,
                        &constraint_kind,
                        &source,
                        &target,
                    ) {
                        Ok(true) => text_result(format!(
                            "Removed {constraint_kind} constraint: {source} → {target}"
                        )),
                        Ok(false) => error_result(format!(
                            "No {constraint_kind} constraint found: {source} → {target}"
                        )),
                        Err(e) => error_result(format!("Failed to remove constraint: {e}")),
                    }
                }

                "list" => {
                    let layers = store
                        .list_layer_assignments(workspace_path)
                        .unwrap_or_default();
                    let constraints = store
                        .list_dependency_constraints(workspace_path)
                        .unwrap_or_default();

                    let layer_items: Vec<Value> = layers
                        .iter()
                        .map(|(ctx, layer)| json!({"context": ctx, "layer": layer}))
                        .collect();
                    let constraint_items: Vec<Value> = constraints
                        .iter()
                        .map(|(kind, src, tgt, rule)| {
                            json!({
                                "constraint_kind": kind,
                                "source": src,
                                "target": tgt,
                                "rule": rule,
                            })
                        })
                        .collect();

                    text_result(
                        json!({
                            "layer_assignments": layer_items,
                            "dependency_constraints": constraint_items,
                        })
                        .to_string(),
                    )
                }

                "evaluate" => match store.evaluate_policy_violations(workspace_path) {
                    Ok(result) => text_result(result.to_string()),
                    Err(e) => error_result(format!("Policy evaluation failed: {e}")),
                },

                _ => error_result(format!(
                    "Unknown action '{action}'. Use 'assign_layer', 'remove_layer', 'add_constraint', 'remove_constraint', 'list', or 'evaluate'."
                )),
            }
        }

        _ => error_result(format!("Unknown write tool: {name}")),
    }
}

// ─── Kind handlers ─────────────────────────────────────────────────────────

fn upsert_bounded_context(store: &Store, workspace_path: &str, args: &Value) -> ToolCallResult {
    let ctx_name = arg_str(args, "name");
    if ctx_name.is_empty() {
        return error_result("'name' is required");
    }
    let existing = store.load_desired(workspace_path).ok().flatten();
    let current = existing.as_ref().and_then(|m| {
        m.bounded_contexts
            .iter()
            .find(|bc| bc.name.eq_ignore_ascii_case(&ctx_name))
    });
    let description = args
        .get("description")
        .and_then(|v| v.as_str())
        .map(String::from)
        .or_else(|| current.map(|bc| bc.description.clone()))
        .unwrap_or_default();
    let module_path = args
        .get("module_path")
        .and_then(|v| v.as_str())
        .map(String::from)
        .or_else(|| current.map(|bc| bc.module_path.clone()))
        .unwrap_or_default();
    let ownership = if args.get("ownership").is_some() {
        parse_ownership(args.get("ownership"))
    } else {
        current.map(|bc| bc.ownership.clone()).unwrap_or_default()
    };
    let dependencies = args
        .get("dependencies")
        .map(|v| parse_string_array(Some(v)))
        .or_else(|| current.map(|bc| bc.dependencies.clone()))
        .unwrap_or_default();
    if let Err(e) = store.upsert_context(
        workspace_path,
        &ctx_name,
        &description,
        &module_path,
        &dependencies,
        &ownership,
    ) {
        return error_result(format!("Failed to upsert bounded context: {e}"));
    }
    let action = if current.is_some() {
        "updated"
    } else {
        "created"
    };

    let all_ctx_names: Vec<String> = store
        .load_desired(workspace_path)
        .ok()
        .flatten()
        .map(|m| m.bounded_contexts.into_iter().map(|c| c.name).collect())
        .unwrap_or_default();
    let unknown_deps: Vec<&str> = dependencies
        .iter()
        .filter(|d| !all_ctx_names.iter().any(|c| c.eq_ignore_ascii_case(d)))
        .map(|d| d.as_str())
        .collect();

    let mut result = json!({
        "message": format!("{} bounded context '{}'", if action == "created" { "Created" } else { "Updated" }, ctx_name),
    });

    if !unknown_deps.is_empty() {
        result["dependency_warnings"] = json!(
            unknown_deps
                .iter()
                .map(|d| format!("Dependency '{}' references an undefined bounded context", d))
                .collect::<Vec<_>>()
        );
    }

    text_result(result.to_string())
}

fn remove_bounded_context(store: &Store, workspace_path: &str, args: &Value) -> ToolCallResult {
    let ctx_name = arg_str(args, "name");
    match store.remove_context(workspace_path, &ctx_name) {
        Ok(true) => text_result(format!("Removed bounded context '{ctx_name}'")),
        Ok(false) => error_result(format!("Bounded context '{ctx_name}' not found")),
        Err(e) => error_result(format!("Failed to remove bounded context: {e}")),
    }
}

fn upsert_entity(store: &Store, workspace_path: &str, args: &Value) -> ToolCallResult {
    let ctx_name = arg_str(args, "context");
    let entity_name = arg_str(args, "name");
    let existing = match require_context(store, workspace_path, &ctx_name) {
        Ok(()) => store.query_entity(workspace_path, &ctx_name, &entity_name),
        Err(result) => return result,
    };
    let mut entity = existing.clone().unwrap_or(Entity {
        name: entity_name.clone(),
        description: String::new(),
        aggregate_root: false,
        fields: vec![],
        methods: vec![],
        invariants: vec![],
        file_path: None,
        start_line: None,
        end_line: None,
    });
    if let Some(desc) = args.get("description").and_then(|v| v.as_str()) {
        entity.description = desc.to_string();
    }
    if let Some(agg) = args.get("aggregate_root").and_then(|v| v.as_bool()) {
        entity.aggregate_root = agg;
    }
    if let Some(fields) = args.get("fields").and_then(|v| v.as_array()) {
        merge_fields(&mut entity.fields, fields);
    }
    if let Some(methods) = args.get("methods").and_then(|v| v.as_array()) {
        merge_methods(&mut entity.methods, methods);
    }
    if let Some(invariants) = args.get("invariants").and_then(|v| v.as_array()) {
        for inv in invariants {
            if let Some(s) = inv.as_str()
                && !entity.invariants.iter().any(|i| i == s)
            {
                entity.invariants.push(s.to_string());
            }
        }
    }
    if let Err(e) = store.upsert_entity(workspace_path, &ctx_name, &entity) {
        return error_result(format!("Failed to upsert entity: {e}"));
    }
    text_result(json!({
        "message": format!("{} entity '{}' in '{}'", if existing.is_some() { "Updated" } else { "Created" }, entity_name, ctx_name),
        "suggested_path": suggested_path_for(store, workspace_path, &ctx_name, "entity", &entity_name)
    }).to_string())
}

fn remove_entity(store: &Store, workspace_path: &str, args: &Value) -> ToolCallResult {
    let ctx_name = arg_str(args, "context");
    let entity_name = arg_str(args, "name");
    match store.remove_entity(workspace_path, &ctx_name, &entity_name) {
        Ok(true) => text_result(format!("Removed entity '{entity_name}' from '{ctx_name}'")),
        Ok(false) => error_result(format!("Entity '{entity_name}' not found in '{ctx_name}'")),
        Err(e) => error_result(format!("Failed to remove entity: {e}")),
    }
}

fn upsert_service(store: &Store, workspace_path: &str, args: &Value) -> ToolCallResult {
    let ctx_name = arg_str(args, "context");
    let svc_name = arg_str(args, "name");
    let existing = match require_context(store, workspace_path, &ctx_name) {
        Ok(()) => store.query_service(workspace_path, &ctx_name, &svc_name),
        Err(result) => return result,
    };
    let mut service = existing.clone().unwrap_or(Service {
        name: svc_name.clone(),
        description: String::new(),
        kind: ServiceKind::Domain,
        methods: vec![],
        dependencies: vec![],
        file_path: None,
        start_line: None,
        end_line: None,
    });
    if let Some(desc) = args.get("description").and_then(|v| v.as_str()) {
        service.description = desc.to_string();
    }
    if args.get("service_kind").is_some() {
        service.kind = parse_service_kind(&arg_str(args, "service_kind"));
    }
    if let Some(deps) = args.get("dependencies").and_then(|v| v.as_array()) {
        service.dependencies = deps
            .iter()
            .filter_map(|d| d.as_str().map(String::from))
            .collect();
    }
    if let Some(methods) = args.get("methods").and_then(|v| v.as_array()) {
        merge_methods(&mut service.methods, methods);
    }
    if let Err(e) = store.upsert_service(workspace_path, &ctx_name, &service) {
        return error_result(format!("Failed to upsert service: {e}"));
    }
    text_result(json!({
        "message": format!("{} service '{}' in '{}'", if existing.is_some() { "Updated" } else { "Created" }, svc_name, ctx_name),
        "suggested_path": suggested_path_for(store, workspace_path, &ctx_name, "service", &svc_name)
    }).to_string())
}

fn remove_service(store: &Store, workspace_path: &str, args: &Value) -> ToolCallResult {
    let ctx_name = arg_str(args, "context");
    let svc_name = arg_str(args, "name");
    match store.remove_service(workspace_path, &ctx_name, &svc_name) {
        Ok(true) => text_result(format!("Removed service '{svc_name}' from '{ctx_name}'")),
        Ok(false) => error_result(format!("Service '{svc_name}' not found in '{ctx_name}'")),
        Err(e) => error_result(format!("Failed to remove service: {e}")),
    }
}

fn upsert_event(store: &Store, workspace_path: &str, args: &Value) -> ToolCallResult {
    let ctx_name = arg_str(args, "context");
    let event_name = arg_str(args, "name");

    let existing = match require_context(store, workspace_path, &ctx_name) {
        Ok(()) => store.query_event(workspace_path, &ctx_name, &event_name),
        Err(result) => return result,
    };
    let mut event = existing.clone().unwrap_or(DomainEvent {
        name: event_name.clone(),
        description: String::new(),
        fields: vec![],
        source: String::new(),
        file_path: None,
        start_line: None,
        end_line: None,
    });
    if let Some(desc) = args.get("description").and_then(|v| v.as_str()) {
        event.description = desc.to_string();
    }
    if let Some(src) = args.get("source").and_then(|v| v.as_str()) {
        event.source = src.to_string();
    }
    if let Some(fields) = args.get("fields").and_then(|v| v.as_array()) {
        merge_fields(&mut event.fields, fields);
    }
    if let Err(e) = store.upsert_event(workspace_path, &ctx_name, &event) {
        return error_result(format!("Failed to upsert event: {e}"));
    }
    text_result(json!({
        "message": format!("{} event '{}' in '{}'", if existing.is_some() { "Updated" } else { "Created" }, event_name, ctx_name),
        "suggested_path": suggested_path_for(store, workspace_path, &ctx_name, "event", &event_name)
    }).to_string())
}

fn remove_event(store: &Store, workspace_path: &str, args: &Value) -> ToolCallResult {
    let ctx_name = arg_str(args, "context");
    let event_name = arg_str(args, "name");
    match store.remove_event(workspace_path, &ctx_name, &event_name) {
        Ok(true) => text_result(format!("Removed event '{event_name}' from '{ctx_name}'")),
        Ok(false) => error_result(format!("Event '{event_name}' not found in '{ctx_name}'")),
        Err(e) => error_result(format!("Failed to remove event: {e}")),
    }
}

fn upsert_value_object(store: &Store, workspace_path: &str, args: &Value) -> ToolCallResult {
    let ctx_name = arg_str(args, "context");
    let vo_name = arg_str(args, "name");

    let existing = match require_context(store, workspace_path, &ctx_name) {
        Ok(()) => store.query_value_object(workspace_path, &ctx_name, &vo_name),
        Err(result) => return result,
    };
    let mut value_object = existing.clone().unwrap_or(ValueObject {
        name: vo_name.clone(),
        description: String::new(),
        fields: vec![],
        validation_rules: vec![],
        file_path: None,
        start_line: None,
        end_line: None,
    });
    if let Some(desc) = args.get("description").and_then(|v| v.as_str()) {
        value_object.description = desc.to_string();
    }
    if let Some(fields) = args.get("fields").and_then(|v| v.as_array()) {
        merge_fields(&mut value_object.fields, fields);
    }
    if let Some(rules) = args.get("validation_rules").and_then(|v| v.as_array()) {
        for rule in rules {
            if let Some(s) = rule.as_str()
                && !value_object.validation_rules.iter().any(|r| r == s)
            {
                value_object.validation_rules.push(s.to_string());
            }
        }
    }
    if let Err(e) = store.upsert_value_object(workspace_path, &ctx_name, &value_object) {
        return error_result(format!("Failed to upsert value object: {e}"));
    }
    text_result(json!({
        "message": format!("{} value object '{}' in '{}'", if existing.is_some() { "Updated" } else { "Created" }, vo_name, ctx_name),
        "suggested_path": suggested_path_for(store, workspace_path, &ctx_name, "value_object", &vo_name)
    }).to_string())
}

fn remove_value_object(store: &Store, workspace_path: &str, args: &Value) -> ToolCallResult {
    let ctx_name = arg_str(args, "context");
    let vo_name = arg_str(args, "name");
    match store.remove_value_object(workspace_path, &ctx_name, &vo_name) {
        Ok(true) => text_result(format!(
            "Removed value object '{vo_name}' from '{ctx_name}'"
        )),
        Ok(false) => error_result(format!(
            "Value object '{vo_name}' not found in '{ctx_name}'"
        )),
        Err(e) => error_result(format!("Failed to remove value object: {e}")),
    }
}

fn upsert_repository(store: &Store, workspace_path: &str, args: &Value) -> ToolCallResult {
    let ctx_name = arg_str(args, "context");
    let repo_name = arg_str(args, "name");

    let existing = match require_context(store, workspace_path, &ctx_name) {
        Ok(()) => store.query_repository(workspace_path, &ctx_name, &repo_name),
        Err(result) => return result,
    };
    let mut repository = existing.clone().unwrap_or(Repository {
        name: repo_name.clone(),
        aggregate: String::new(),
        methods: vec![],
        file_path: None,
        start_line: None,
        end_line: None,
    });
    if let Some(agg) = args.get("aggregate").and_then(|v| v.as_str()) {
        repository.aggregate = agg.to_string();
    }
    if let Some(methods) = args.get("methods").and_then(|v| v.as_array()) {
        merge_methods(&mut repository.methods, methods);
    }
    if let Err(e) = store.upsert_repository(workspace_path, &ctx_name, &repository) {
        return error_result(format!("Failed to upsert repository: {e}"));
    }
    text_result(json!({
        "message": format!("{} repository '{}' in '{}'", if existing.is_some() { "Updated" } else { "Created" }, repo_name, ctx_name),
        "suggested_path": suggested_path_for(store, workspace_path, &ctx_name, "repository", &repo_name)
    }).to_string())
}

fn remove_repository(store: &Store, workspace_path: &str, args: &Value) -> ToolCallResult {
    let ctx_name = arg_str(args, "context");
    let repo_name = arg_str(args, "name");
    match store.remove_repository(workspace_path, &ctx_name, &repo_name) {
        Ok(true) => text_result(format!(
            "Removed repository '{repo_name}' from '{ctx_name}'"
        )),
        Ok(false) => error_result(format!(
            "Repository '{repo_name}' not found in '{ctx_name}'"
        )),
        Err(e) => error_result(format!("Failed to remove repository: {e}")),
    }
}

fn upsert_aggregate(store: &Store, workspace_path: &str, args: &Value) -> ToolCallResult {
    let ctx_name = arg_str(args, "context");
    let aggregate_name = arg_str(args, "name");

    let existing = match require_context(store, workspace_path, &ctx_name) {
        Ok(()) => store.query_aggregate(workspace_path, &ctx_name, &aggregate_name),
        Err(result) => return result,
    };
    let mut aggregate = existing.clone().unwrap_or(Aggregate {
        name: aggregate_name.clone(),
        description: String::new(),
        root_entity: String::new(),
        entities: vec![],
        value_objects: vec![],
        ownership: Ownership::default(),
    });
    if let Some(description) = args.get("description").and_then(|v| v.as_str()) {
        aggregate.description = description.to_string();
    }
    if let Some(root_entity) = args.get("root_entity").and_then(|v| v.as_str()) {
        aggregate.root_entity = root_entity.to_string();
    }
    if args.get("entities").is_some() {
        aggregate.entities = parse_string_array(args.get("entities"));
    }
    if args.get("value_objects").is_some() {
        aggregate.value_objects = parse_string_array(args.get("value_objects"));
    }
    if args.get("ownership").is_some() {
        aggregate.ownership = parse_ownership(args.get("ownership"));
    }
    if let Err(e) = store.upsert_aggregate(workspace_path, &ctx_name, &aggregate) {
        return error_result(format!("Failed to upsert aggregate: {e}"));
    }
    text_result(json!({"message": format!("{} aggregate '{}' in '{}'", if existing.is_some() { "Updated" } else { "Created" }, aggregate_name, ctx_name)}).to_string())
}

fn remove_aggregate(store: &Store, workspace_path: &str, args: &Value) -> ToolCallResult {
    let ctx_name = arg_str(args, "context");
    let aggregate_name = arg_str(args, "name");
    match store.remove_aggregate(workspace_path, &ctx_name, &aggregate_name) {
        Ok(true) => text_result(format!(
            "Removed aggregate '{aggregate_name}' from '{ctx_name}'"
        )),
        Ok(false) => error_result(format!(
            "Aggregate '{aggregate_name}' not found in '{ctx_name}'"
        )),
        Err(e) => error_result(format!("Failed to remove aggregate: {e}")),
    }
}

fn upsert_policy(store: &Store, workspace_path: &str, args: &Value) -> ToolCallResult {
    let ctx_name = arg_str(args, "context");
    let policy_name = arg_str(args, "name");

    let existing = match require_context(store, workspace_path, &ctx_name) {
        Ok(()) => store.query_policy(workspace_path, &ctx_name, &policy_name),
        Err(result) => return result,
    };
    let mut policy = existing.clone().unwrap_or(Policy {
        name: policy_name.clone(),
        description: String::new(),
        kind: PolicyKind::Domain,
        triggers: vec![],
        commands: vec![],
        ownership: Ownership::default(),
    });
    if let Some(description) = args.get("description").and_then(|v| v.as_str()) {
        policy.description = description.to_string();
    }
    if args.get("policy_kind").is_some() {
        policy.kind = parse_policy_kind(&arg_str(args, "policy_kind"));
    }
    if args.get("triggers").is_some() {
        policy.triggers = parse_string_array(args.get("triggers"));
    }
    if args.get("commands").is_some() {
        policy.commands = parse_string_array(args.get("commands"));
    }
    if args.get("ownership").is_some() {
        policy.ownership = parse_ownership(args.get("ownership"));
    }
    if let Err(e) = store.upsert_policy(workspace_path, &ctx_name, &policy) {
        return error_result(format!("Failed to upsert policy: {e}"));
    }
    text_result(json!({"message": format!("{} policy '{}' in '{}'", if existing.is_some() { "Updated" } else { "Created" }, policy_name, ctx_name)}).to_string())
}

fn remove_policy(store: &Store, workspace_path: &str, args: &Value) -> ToolCallResult {
    let ctx_name = arg_str(args, "context");
    let policy_name = arg_str(args, "name");
    match store.remove_policy(workspace_path, &ctx_name, &policy_name) {
        Ok(true) => text_result(format!("Removed policy '{policy_name}' from '{ctx_name}'")),
        Ok(false) => error_result(format!("Policy '{policy_name}' not found in '{ctx_name}'")),
        Err(e) => error_result(format!("Failed to remove policy: {e}")),
    }
}

fn upsert_read_model(store: &Store, workspace_path: &str, args: &Value) -> ToolCallResult {
    let ctx_name = arg_str(args, "context");
    let read_model_name = arg_str(args, "name");

    let existing = match require_context(store, workspace_path, &ctx_name) {
        Ok(()) => store.query_read_model(workspace_path, &ctx_name, &read_model_name),
        Err(result) => return result,
    };
    let mut read_model = existing.clone().unwrap_or(ReadModel {
        name: read_model_name.clone(),
        description: String::new(),
        source: String::new(),
        fields: vec![],
        ownership: Ownership::default(),
    });
    if let Some(description) = args.get("description").and_then(|v| v.as_str()) {
        read_model.description = description.to_string();
    }
    if let Some(source) = args.get("source").and_then(|v| v.as_str()) {
        read_model.source = source.to_string();
    }
    if let Some(fields) = args.get("fields").and_then(|v| v.as_array()) {
        merge_fields(&mut read_model.fields, fields);
    }
    if args.get("ownership").is_some() {
        read_model.ownership = parse_ownership(args.get("ownership"));
    }
    if let Err(e) = store.upsert_read_model(workspace_path, &ctx_name, &read_model) {
        return error_result(format!("Failed to upsert read model: {e}"));
    }
    text_result(json!({"message": format!("{} read model '{}' in '{}'", if existing.is_some() { "Updated" } else { "Created" }, read_model_name, ctx_name)}).to_string())
}

fn remove_read_model(store: &Store, workspace_path: &str, args: &Value) -> ToolCallResult {
    let ctx_name = arg_str(args, "context");
    let read_model_name = arg_str(args, "name");
    match store.remove_read_model(workspace_path, &ctx_name, &read_model_name) {
        Ok(true) => text_result(format!(
            "Removed read model '{read_model_name}' from '{ctx_name}'"
        )),
        Ok(false) => error_result(format!(
            "Read model '{read_model_name}' not found in '{ctx_name}'"
        )),
        Err(e) => error_result(format!("Failed to remove read model: {e}")),
    }
}

fn upsert_external_system(store: &Store, workspace_path: &str, args: &Value) -> ToolCallResult {
    let system_name = arg_str(args, "name");
    let existing = store.query_external_system(workspace_path, &system_name);
    let mut system = existing.clone().unwrap_or(ExternalSystem {
        name: system_name.clone(),
        description: String::new(),
        kind: String::new(),
        consumed_by_contexts: vec![],
        rationale: String::new(),
        ownership: Ownership::default(),
    });
    if let Some(description) = args.get("description").and_then(|v| v.as_str()) {
        system.description = description.to_string();
    }
    if let Some(kind) = args.get("kind_label").and_then(|v| v.as_str()) {
        system.kind = kind.to_string();
    }
    if let Some(rationale) = args.get("rationale").and_then(|v| v.as_str()) {
        system.rationale = rationale.to_string();
    }
    if args.get("consumed_by_contexts").is_some() {
        system.consumed_by_contexts = parse_string_array(args.get("consumed_by_contexts"));
    }
    if args.get("ownership").is_some() {
        system.ownership = parse_ownership(args.get("ownership"));
    }
    if let Err(e) = store.upsert_external_system(workspace_path, &system) {
        return error_result(format!("Failed to upsert external system: {e}"));
    }
    text_result(json!({"message": format!("{} external system '{}'", if existing.is_some() { "Updated" } else { "Created" }, system_name)}).to_string())
}

fn remove_external_system(store: &Store, workspace_path: &str, args: &Value) -> ToolCallResult {
    let system_name = arg_str(args, "name");
    match store.remove_external_system(workspace_path, &system_name) {
        Ok(true) => text_result(format!("Removed external system '{system_name}'")),
        Ok(false) => error_result(format!("External system '{system_name}' not found")),
        Err(e) => error_result(format!("Failed to remove external system: {e}")),
    }
}

fn upsert_architectural_decision(
    store: &Store,
    workspace_path: &str,
    args: &Value,
) -> ToolCallResult {
    let decision_id = arg_str(args, "name");
    let existing = store.query_architectural_decision(workspace_path, &decision_id);
    let mut decision = existing.clone().unwrap_or(ArchitecturalDecision {
        id: decision_id.clone(),
        title: String::new(),
        status: DecisionStatus::Proposed,
        scope: String::new(),
        date: String::new(),
        rationale: String::new(),
        consequences: vec![],
        contexts: vec![],
        ownership: Ownership::default(),
    });
    if let Some(title) = args.get("title").and_then(|v| v.as_str()) {
        decision.title = title.to_string();
    }
    if args.get("status").is_some() {
        decision.status = parse_decision_status(&arg_str(args, "status"));
    }
    if let Some(scope) = args.get("scope").and_then(|v| v.as_str()) {
        decision.scope = scope.to_string();
    }
    if let Some(date) = args.get("date").and_then(|v| v.as_str()) {
        decision.date = date.to_string();
    }
    if let Some(rationale) = args.get("rationale").and_then(|v| v.as_str()) {
        decision.rationale = rationale.to_string();
    }
    if args.get("contexts").is_some() {
        decision.contexts = parse_string_array(args.get("contexts"));
    }
    if args.get("consequences").is_some() {
        decision.consequences = parse_string_array(args.get("consequences"));
    }
    if args.get("ownership").is_some() {
        decision.ownership = parse_ownership(args.get("ownership"));
    }
    if let Err(e) = store.upsert_architectural_decision(workspace_path, &decision) {
        return error_result(format!("Failed to upsert architectural decision: {e}"));
    }
    text_result(json!({"message": format!("{} architectural decision '{}'", if existing.is_some() { "Updated" } else { "Created" }, decision_id)}).to_string())
}

fn remove_architectural_decision(
    store: &Store,
    workspace_path: &str,
    args: &Value,
) -> ToolCallResult {
    let decision_id = arg_str(args, "name");
    match store.remove_architectural_decision(workspace_path, &decision_id) {
        Ok(true) => text_result(format!("Removed architectural decision '{decision_id}'")),
        Ok(false) => error_result(format!("Architectural decision '{decision_id}' not found")),
        Err(e) => error_result(format!("Failed to remove architectural decision: {e}")),
    }
}

// ─── Helpers ───────────────────────────────────────────────────────────────

fn text_result(text: impl Into<String>) -> ToolCallResult {
    ToolCallResult {
        content: vec![ContentBlock::Text { text: text.into() }],
        is_error: None,
    }
}

fn error_result(msg: impl Into<String>) -> ToolCallResult {
    ToolCallResult {
        content: vec![ContentBlock::Text { text: msg.into() }],
        is_error: Some(true),
    }
}

fn arg_str(args: &Value, key: &str) -> String {
    args.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

fn parse_ownership(val: Option<&Value>) -> Ownership {
    let Some(obj) = val else {
        return Ownership::default();
    };
    Ownership {
        team: obj
            .get("team")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        owners: obj
            .get("owners")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
        rationale: obj
            .get("rationale")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
    }
}

fn parse_string_array(val: Option<&Value>) -> Vec<String> {
    val.and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

fn parse_policy_kind(kind: &str) -> PolicyKind {
    match kind {
        "process_manager" => PolicyKind::ProcessManager,
        "integration" => PolicyKind::Integration,
        _ => PolicyKind::Domain,
    }
}

fn parse_service_kind(kind: &str) -> ServiceKind {
    match kind {
        "application" => ServiceKind::Application,
        "infrastructure" => ServiceKind::Infrastructure,
        _ => ServiceKind::Domain,
    }
}

fn parse_decision_status(status: &str) -> DecisionStatus {
    match status {
        "accepted" => DecisionStatus::Accepted,
        "superseded" => DecisionStatus::Superseded,
        "deprecated" => DecisionStatus::Deprecated,
        _ => DecisionStatus::Proposed,
    }
}

fn require_context(
    store: &Store,
    workspace_path: &str,
    ctx_name: &str,
) -> Result<(), ToolCallResult> {
    let exists = store
        .load_desired(workspace_path)
        .ok()
        .flatten()
        .map(|model| {
            model
                .bounded_contexts
                .iter()
                .any(|bc| bc.name.eq_ignore_ascii_case(ctx_name))
        })
        .unwrap_or(false);
    if exists {
        Ok(())
    } else {
        Err(error_result(format!(
            "Bounded context '{ctx_name}' not found"
        )))
    }
}

fn suggested_path_for(
    store: &Store,
    workspace_path: &str,
    context: &str,
    kind: &str,
    name: &str,
) -> String {
    let pattern = store
        .load_desired(workspace_path)
        .ok()
        .flatten()
        .map(|model| model.conventions.file_structure.pattern)
        .unwrap_or_default();
    suggest_path(&pattern, context, kind, name)
}

/// Compute the suggested file path for a domain artifact, using project conventions.
/// This replaces the standalone `suggest_file_path` tool — now implicit in every
/// `define` response for artifacts that live in files (entity, service, event).
fn suggest_path(pattern: &str, context: &str, kind: &str, name: &str) -> String {
    let layer = match kind {
        "entity" | "value_object" | "event" => "domain",
        "service" => "application",
        "repository" => "infrastructure",
        other => other,
    };
    if pattern.is_empty() {
        return format!("src/{}/{}/{}.rs", to_snake(context), layer, to_snake(name));
    }
    pattern
        .replace("{context}", &to_snake(context))
        .replace("{layer}", layer)
        .replace("{type}", &to_snake(name))
}

fn parse_fields(val: Option<&Value>) -> Vec<Field> {
    val.and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|f| {
                    Some(Field {
                        name: f.get("name")?.as_str()?.to_string(),
                        field_type: f.get("type")?.as_str()?.to_string(),
                        required: f.get("required").and_then(|v| v.as_bool()).unwrap_or(false),
                        description: f
                            .get("description")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_methods(val: Option<&Value>) -> Vec<Method> {
    val.and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|m| {
                    Some(Method {
                        name: m.get("name")?.as_str()?.to_string(),
                        description: m
                            .get("description")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        parameters: parse_fields(m.get("parameters")),
                        return_type: m
                            .get("return_type")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        file_path: None,
                        start_line: None,
                        end_line: None,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn merge_fields(existing: &mut Vec<Field>, new_fields: &[Value]) {
    for f in new_fields {
        let name = match f.get("name").and_then(|v| v.as_str()) {
            Some(n) => n,
            None => continue,
        };
        if let Some(existing_f) = existing.iter_mut().find(|ef| ef.name == name) {
            if let Some(t) = f.get("type").and_then(|v| v.as_str()) {
                existing_f.field_type = t.to_string();
            }
            if let Some(r) = f.get("required").and_then(|v| v.as_bool()) {
                existing_f.required = r;
            }
            if let Some(d) = f.get("description").and_then(|v| v.as_str()) {
                existing_f.description = d.to_string();
            }
        } else if let Some(field) = parse_fields(Some(&json!([f]))).into_iter().next() {
            existing.push(field);
        }
    }
}

fn merge_methods(existing: &mut Vec<Method>, new_methods: &[Value]) {
    for m in new_methods {
        let name = match m.get("name").and_then(|v| v.as_str()) {
            Some(n) => n,
            None => continue,
        };
        if let Some(existing_m) = existing.iter_mut().find(|em| em.name == name) {
            if let Some(d) = m.get("description").and_then(|v| v.as_str()) {
                existing_m.description = d.to_string();
            }
            if let Some(rt) = m.get("return_type").and_then(|v| v.as_str()) {
                existing_m.return_type = rt.to_string();
            }
        } else if let Some(method) = parse_methods(Some(&json!([m]))).into_iter().next() {
            existing.push(method);
        }
    }
}

// ── Refactor Plan Enrichment ──────────────────────────────────────────────

/// Priority ordering for change kinds. Lower = do first.
fn kind_priority(kind: &str) -> u8 {
    match kind {
        "context" => 0,
        "entity" => 1,
        "service" => 2,
        "repository" => 3,
        "value_object" => 4,
        "event" => 5,
        "invariant" | "field" | "method" => 6,
        _ => 7,
    }
}

/// Suggest a file path for a pending change based on context module_path and kind.
fn suggest_file(module_path: &str, kind: &str, name: &str) -> String {
    if module_path.is_empty() {
        return String::new();
    }
    let snake = to_snake(name);
    match kind {
        "context" => format!("{}/mod.rs", module_path),
        "entity" | "value_object" => format!("{}/model.rs", module_path),
        "service" => format!("{}/{}.rs", module_path, snake),
        "repository" => format!("{}/{}.rs", module_path, snake),
        "event" => format!("{}/events.rs", module_path),
        "field" | "method" | "invariant" => {
            // These belong to their owner; owner_kind determines the file
            format!("{}/mod.rs", module_path)
        }
        _ => String::new(),
    }
}

/// Enrich raw Datalog diff with suggested files, priorities, rationale, and health score.
/// Runs the full architectural analysis pipeline in one call.
///
/// Returns health, all invariant checks, actual-vs-desired drift, AST edge
/// statistics, and prioritized `next_actions` so the calling agent knows
/// exactly what to do next. This is the orchestrator for the self-improvement loop.
fn diagnose_pipeline(store: &Store, workspace_path: &str) -> ToolCallResult {
    let canonical = crate::store::cozo::canonicalize_path(workspace_path);

    // ── 1. Check if actual model exists ────────────────────────────────
    let has_actual = store
        .load_actual(workspace_path)
        .ok()
        .flatten()
        .is_some_and(|m| !m.bounded_contexts.is_empty());

    let has_desired = store
        .load_desired(workspace_path)
        .ok()
        .flatten()
        .is_some_and(|m| !m.bounded_contexts.is_empty());

    // ── 2. Health check ────────────────────────────────────────────────
    let health = store.model_health(&canonical).ok();
    let health_json = health.as_ref().map(|h| {
        json!({
            "score": h.score,
            "circular_deps": h.circular_deps,
            "layer_violations": h.layer_violations.iter().map(|v| json!({
                "context": v.context,
                "domain_service": v.domain_service,
                "infra_dependency": v.infra_dependency,
            })).collect::<Vec<_>>(),
            "missing_invariants": h.missing_invariants,
            "orphan_contexts": h.orphan_contexts,
            "god_contexts": h.god_contexts,
            "unsourced_events": h.unsourced_events,
        })
    });

    // ── 3. Invariant checks ────────────────────────────────────────────
    let circular = store.circular_deps(&canonical).unwrap_or_default();
    let layers = store.layer_violations(&canonical).unwrap_or_default();
    let agg_quality = store
        .aggregate_roots_without_invariants(&canonical)
        .unwrap_or_default();
    let policy = store.evaluate_policy_violations(&canonical).ok();

    let invariants = json!({
        "circular_deps": {
            "status": if circular.is_empty() { "pass" } else { "fail" },
            "count": circular.len(),
            "cycles": circular.iter().map(|(a, b)| json!({"from": a, "to": b})).collect::<Vec<_>>(),
        },
        "layer_violations": {
            "status": if layers.is_empty() { "pass" } else { "fail" },
            "count": layers.len(),
            "violations": layers.iter().map(|(ctx, svc, dep)| json!({
                "context": ctx, "service": svc, "dependency": dep,
            })).collect::<Vec<_>>(),
        },
        "aggregate_quality": {
            "status": if agg_quality.is_empty() { "pass" } else { "fail" },
            "count": agg_quality.len(),
            "roots_without_invariants": agg_quality.iter().map(|(ctx, ent)| json!({
                "context": ctx, "entity": ent,
            })).collect::<Vec<_>>(),
        },
        "policy_violations": policy.as_ref().map(|p| json!({
            "status": p["status"],
            "count": p["count"],
            "violations": p["violations"],
        })),
    });

    // ── 4. Actual vs desired drift ─────────────────────────────────────
    let drift = store.diff_graph(workspace_path).ok().map(|diff| {
        let changes = diff["pending_changes"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        json!({
            "status": if changes.is_empty() { "in_sync" } else { "drifted" },
            "pending_change_count": changes.len(),
            "pending_changes": changes,
        })
    });

    // ── 5. AST edge statistics ─────────────────────────────────────────
    let ast_stats = store.load_actual(workspace_path).ok().flatten().map(|m| {
        let total = m.ast_edges.len();
        let mut by_type: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
        for edge in &m.ast_edges {
            *by_type.entry(edge.edge_type.as_str()).or_default() += 1;
        }
        let breakdown: serde_json::Map<String, Value> = by_type
            .into_iter()
            .map(|(k, v)| (k.to_string(), json!(v)))
            .collect();
        json!({
            "total": total,
            "by_type": breakdown,
        })
    });

    // ── 6. Compute prioritized next actions ────────────────────────────
    let mut next_actions: Vec<Value> = Vec::new();
    let mut priority = 0u32;

    if !has_actual {
        next_actions.push(json!({
            "priority": priority,
            "tool": "sync",
            "reason": "No current model exists. Scan the workspace to extract architecture from source code.",
        }));
        priority += 1;
    }

    // Critical: cycles must be broken first
    if !circular.is_empty() {
        next_actions.push(json!({
            "priority": priority,
            "tool": "define",
            "reason": format!(
                "{} circular dependency cycle(s) detected. Break cycles by extracting shared concepts or using events.",
                circular.len()
            ),
            "evidence": circular.iter().map(|(a, b)| format!("{a} ⇄ {b}")).collect::<Vec<_>>(),
        }));
        priority += 1;
    }

    // Critical: layer violations
    if !layers.is_empty() {
        next_actions.push(json!({
            "priority": priority,
            "tool": "define",
            "reason": format!(
                "{} layer violation(s). Domain services depend on infrastructure directly. Invert via ports/adapters.",
                layers.len()
            ),
            "evidence": layers.iter().map(|(ctx, svc, dep)| format!("{ctx}.{svc} → {dep}")).collect::<Vec<_>>(),
        }));
        priority += 1;
    }

    // Warning: policy violations
    if let Some(ref p) = policy {
        let pcount = p["count"].as_u64().unwrap_or(0);
        if pcount > 0 {
            next_actions.push(json!({
                "priority": priority,
                "tool": "constrain",
                "action": "evaluate",
                "reason": format!("{pcount} policy violation(s). Declared constraints are not met."),
            }));
            priority += 1;
        }
    }

    // Warning: aggregate quality
    if !agg_quality.is_empty() {
        next_actions.push(json!({
            "priority": priority,
            "tool": "define",
            "reason": format!(
                "{} aggregate root(s) without invariants. Add business rules to protect consistency.",
                agg_quality.len()
            ),
            "evidence": agg_quality.iter().map(|(ctx, ent)| format!("{ctx}.{ent}")).collect::<Vec<_>>(),
        }));
        priority += 1;
    }

    // Warning: unsourced events
    if let Some(ref h) = health {
        if !h.unsourced_events.is_empty() {
            next_actions.push(json!({
                "priority": priority,
                "tool": "define",
                "reason": format!(
                    "{} event(s) without a source entity. Link them to their originating aggregate.",
                    h.unsourced_events.len()
                ),
                "evidence": h.unsourced_events.iter().map(|[ctx, evt]| format!("{ctx}.{evt}")).collect::<Vec<_>>(),
            }));
            priority += 1;
        }
    }

    // Info: orphan contexts
    if let Some(ref h) = health {
        if !h.orphan_contexts.is_empty() {
            next_actions.push(json!({
                "priority": priority,
                "tool": "define",
                "reason": format!(
                    "{} orphan context(s) with no dependencies. Add dependencies or verify they are intentionally standalone.",
                    h.orphan_contexts.len()
                ),
                "evidence": &h.orphan_contexts,
            }));
        }
    }

    // Info: drift between current and planned
    if let Some(ref d) = drift {
        let dcount = d["pending_change_count"].as_u64().unwrap_or(0);
        if dcount > 0 {
            next_actions.push(json!({
                "priority": priority,
                "tool": "refactor",
                "action": "plan",
                "reason": format!("{dcount} pending change(s) between planned and current. Run 'plan' for detailed refactoring steps."),
            }));
        }
    }

    // If nothing else needed, suggest re-scan to verify
    if next_actions.is_empty() && has_actual {
        next_actions.push(json!({
            "priority": 0,
            "tool": "sync",
            "reason": "Architecture is healthy (score 100). Re-scan periodically to verify after code changes.",
        }));
    }

    let score = health.as_ref().map(|h| h.score).unwrap_or(0);

    text_result(
        json!({
            "status": if score == 100 { "healthy" } else if score >= 70 { "needs_improvement" } else { "unhealthy" },
            "health_score": score,
            "health": health_json,
            "invariants": invariants,
            "drift": drift,
            "ast_edges": ast_stats,
            "has_actual_model": has_actual,
            "has_desired_model": has_desired,
            "next_actions": next_actions,
            "loop_hint": "After implementing fixes, call sync then diagnose again to verify improvement.",
        })
        .to_string(),
    )
}

fn enrich_plan(store: &Store, workspace_path: &str, changes: &[Value]) -> Value {
    // Build context → module_path lookup from desired state
    let module_paths: std::collections::HashMap<String, String> = store
        .run_datalog(
            "?[name, module_path] := *context{workspace: $ws, name, module_path, state: 'desired' @ 'NOW'}",
            workspace_path,
        )
        .unwrap_or_default()
        .into_iter()
        .map(|row| (row[0].clone(), row[1].clone()))
        .collect();

    // Enrich each change
    let mut enriched: Vec<Value> = changes
        .iter()
        .map(|change| {
            let kind = change["kind"].as_str().unwrap_or("");
            let action = change["action"].as_str().unwrap_or("");
            let name = change["name"].as_str().unwrap_or("");
            let ctx = change.get("context").and_then(|v| v.as_str()).unwrap_or("");

            let mp = if kind == "context" {
                module_paths.get(name).cloned().unwrap_or_default()
            } else {
                module_paths.get(ctx).cloned().unwrap_or_default()
            };

            let suggested_file = suggest_file(&mp, kind, name);
            let priority = kind_priority(kind);

            let rationale = match (action, kind) {
                ("add", "context") => format!("Create bounded context '{name}' module structure"),
                ("remove", "context") => format!("Remove bounded context '{name}' and its module"),
                ("add", "entity") => format!("Add entity '{name}' to context '{ctx}'"),
                ("remove", "entity") => format!("Remove entity '{name}' from context '{ctx}'"),
                ("add", "service") => format!("Implement service '{name}' in context '{ctx}'"),
                ("remove", "service") => format!("Remove service '{name}' from context '{ctx}'"),
                ("add", k) => format!("Add {k} '{name}' in context '{ctx}'"),
                ("remove", k) => format!("Remove {k} '{name}' from context '{ctx}'"),
                _ => String::new(),
            };

            let mut entry = change.clone();
            entry["priority"] = json!(priority);
            entry["suggested_file"] = json!(suggested_file);
            entry["rationale"] = json!(rationale);
            entry
        })
        .collect();

    // Sort by priority (structural changes first)
    enriched.sort_by_key(|e| e["priority"].as_u64().unwrap_or(99));

    // Include model health score
    let health_score = store
        .model_health(workspace_path)
        .map(|h| h.score)
        .unwrap_or(0);

    json!({
        "status": "pending_changes",
        "pending_changes": enriched,
        "change_count": enriched.len(),
        "health_score": health_score,
        "migration_notes": [
            "Apply changes in priority order (0 = highest).",
            "Context-level changes should be done before entity/service changes.",
            "Run `sync` after implementing to update the current model.",
            "Run `refactor accept` when implementation matches the planned architecture."
        ]
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;
    use std::env::temp_dir;

    fn test_store() -> Store {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = temp_dir().join(format!(
            "dendrites_wt_test_{}_{}.db",
            std::process::id(),
            id
        ));
        Store::open(&path).unwrap()
    }

    fn test_model() -> DomainModel {
        DomainModel {
            name: "TestProject".into(),
            description: "Test".into(),
            bounded_contexts: vec![BoundedContext {
                name: "Identity".into(),
                description: "Auth context".into(),
                module_path: "src/identity".into(),
                ownership: Ownership::default(),
                aggregates: vec![],
                policies: vec![],
                read_models: vec![],
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
                    file_path: None,
                    start_line: None,
                    end_line: None,
                }],
                value_objects: vec![],
                services: vec![],
                api_endpoints: vec![],
                repositories: vec![],
                events: vec![],
                modules: vec![],
                dependencies: vec![],
            }],
            external_systems: vec![],
            architectural_decisions: vec![],
            ownership: Ownership::default(),
            rules: vec![],
            tech_stack: TechStack::default(),
            conventions: Conventions::default(),
            ast_edges: vec![],
            source_files: vec![],
            symbols: vec![],
            import_edges: vec![],
            call_edges: vec![],
        }
    }

    /// Save initial model and return (store, workspace).
    fn setup(ws: &str) -> Store {
        let store = test_store();
        store.save_desired(ws, &test_model()).unwrap();
        store
    }

    #[test]
    fn test_list_write_tools_count() {
        assert_eq!(list_write_tools().len(), 4);
    }

    #[test]
    fn test_update_entity_add_field() {
        let ws = "/tmp/test-add-field";
        let store = setup(ws);
        let result = call_write_tool(
            ws,
            &store,
            "define",
            &json!({
                "kind": "entity", "context": "Identity", "name": "User",
                "fields": [{"name": "email", "type": "String", "required": true}]
            }),
        );
        assert!(result.is_error.is_none());
        let loaded = store.load_desired(ws).unwrap().unwrap();
        let identity = loaded
            .bounded_contexts
            .iter()
            .find(|c| c.name == "Identity")
            .unwrap();
        let user = identity.entities.iter().find(|e| e.name == "User").unwrap();
        assert_eq!(user.fields.len(), 2);
        assert!(user.fields.iter().any(|f| f.name == "email"));
    }

    #[test]
    fn test_update_entity_merge_existing_field() {
        let ws = "/tmp/test-merge-field";
        let store = setup(ws);
        let result = call_write_tool(
            ws,
            &store,
            "define",
            &json!({
                "kind": "entity", "context": "Identity", "name": "User",
                "fields": [{"name": "id", "type": "Uuid"}]
            }),
        );
        assert!(result.is_error.is_none());
        let loaded = store.load_desired(ws).unwrap().unwrap();
        let identity = loaded
            .bounded_contexts
            .iter()
            .find(|c| c.name == "Identity")
            .unwrap();
        let user = identity.entities.iter().find(|e| e.name == "User").unwrap();
        assert_eq!(user.fields.len(), 1);
        assert_eq!(user.fields[0].field_type, "Uuid");
    }

    #[test]
    fn test_create_new_entity() {
        let ws = "/tmp/test-new-entity";
        let store = setup(ws);
        let result = call_write_tool(
            ws,
            &store,
            "define",
            &json!({
                "kind": "entity", "context": "Identity", "name": "Role",
                "description": "A role assignment", "aggregate_root": false,
                "fields": [{"name": "name", "type": "String"}]
            }),
        );
        assert!(result.is_error.is_none());
        let loaded = store.load_desired(ws).unwrap().unwrap();
        let identity = loaded
            .bounded_contexts
            .iter()
            .find(|c| c.name == "Identity")
            .unwrap();
        assert_eq!(identity.entities.len(), 2);
        assert!(identity.entities.iter().any(|e| e.name == "Role"));
    }

    #[test]
    fn test_update_entity_context_not_found() {
        let ws = "/tmp/test-ctx-notfound";
        let store = setup(ws);
        let result = call_write_tool(
            ws,
            &store,
            "define",
            &json!({"kind": "entity", "context": "Nonexistent", "name": "Foo"}),
        );
        assert_eq!(result.is_error, Some(true));
    }

    #[test]
    fn test_create_bounded_context() {
        let ws = "/tmp/test-create-bc";
        let store = setup(ws);
        let result = call_write_tool(
            ws,
            &store,
            "define",
            &json!({
                "kind": "bounded_context", "name": "Billing",
                "description": "Billing context", "module_path": "src/billing",
                "dependencies": ["Identity"]
            }),
        );
        assert!(result.is_error.is_none());
        let loaded = store.load_desired(ws).unwrap().unwrap();
        assert_eq!(loaded.bounded_contexts.len(), 2);
        let billing = loaded
            .bounded_contexts
            .iter()
            .find(|c| c.name == "Billing")
            .unwrap();
        assert_eq!(billing.dependencies, vec!["Identity"]);
    }

    #[test]
    fn test_update_existing_bounded_context() {
        let ws = "/tmp/test-update-bc";
        let store = setup(ws);
        let result = call_write_tool(
            ws,
            &store,
            "define",
            &json!({
                "kind": "bounded_context", "name": "Identity",
                "description": "Updated description"
            }),
        );
        assert!(result.is_error.is_none());
        let loaded = store.load_desired(ws).unwrap().unwrap();
        let identity = loaded
            .bounded_contexts
            .iter()
            .find(|c| c.name == "Identity")
            .unwrap();
        assert_eq!(identity.description, "Updated description");
    }

    #[test]
    fn test_remove_entity() {
        let ws = "/tmp/test-rm-entity";
        let store = setup(ws);
        let result = call_write_tool(
            ws,
            &store,
            "define",
            &json!({"kind": "entity", "action": "remove", "context": "Identity", "name": "User"}),
        );
        assert!(result.is_error.is_none());
        let loaded = store.load_desired(ws).unwrap().unwrap();
        let identity = loaded
            .bounded_contexts
            .iter()
            .find(|c| c.name == "Identity")
            .unwrap();
        assert_eq!(identity.entities.len(), 0);
    }

    #[test]
    fn test_remove_entity_not_found() {
        let ws = "/tmp/test-rm-entity-nf";
        let store = setup(ws);
        let result = call_write_tool(
            ws,
            &store,
            "define",
            &json!({"kind": "entity", "action": "remove", "context": "Identity", "name": "NotHere"}),
        );
        assert_eq!(result.is_error, Some(true));
    }

    #[test]
    fn test_update_service() {
        let ws = "/tmp/test-upd-svc";
        let store = setup(ws);
        let result = call_write_tool(
            ws,
            &store,
            "define",
            &json!({
                "kind": "service", "context": "Identity", "name": "AuthService",
                "service_kind": "application", "description": "Handles authentication"
            }),
        );
        assert!(result.is_error.is_none());
        let loaded = store.load_desired(ws).unwrap().unwrap();
        let identity = loaded
            .bounded_contexts
            .iter()
            .find(|c| c.name == "Identity")
            .unwrap();
        assert_eq!(identity.services.len(), 1);
        assert_eq!(identity.services[0].description, "Handles authentication");
    }

    #[test]
    fn test_update_event() {
        let ws = "/tmp/test-upd-evt";
        let store = setup(ws);
        let result = call_write_tool(
            ws,
            &store,
            "define",
            &json!({
                "kind": "event", "context": "Identity", "name": "UserRegistered",
                "source": "User", "fields": [{"name": "user_id", "type": "UserId"}]
            }),
        );
        assert!(result.is_error.is_none());
        let loaded = store.load_desired(ws).unwrap().unwrap();
        let identity = loaded
            .bounded_contexts
            .iter()
            .find(|c| c.name == "Identity")
            .unwrap();
        assert_eq!(identity.events.len(), 1);
        assert_eq!(identity.events[0].name, "UserRegistered");
    }

    #[test]
    fn test_upsert_aggregate_persists_members_and_ownership() {
        let ws = "/tmp/test-upsert-aggregate";
        let store = setup(ws);

        let result = call_write_tool(
            ws,
            &store,
            "define",
            &json!({
                "kind": "aggregate",
                "context": "Identity",
                "name": "UserAggregate",
                "description": "User consistency boundary",
                "root_entity": "User",
                "entities": ["User"],
                "value_objects": ["EmailAddress"],
                "ownership": {
                    "team": "Identity Team",
                    "owners": ["alice"],
                    "rationale": "Owns authentication"
                }
            }),
        );

        assert!(result.is_error.is_none());
        let loaded = store.load_desired(ws).unwrap().unwrap();
        let identity = loaded
            .bounded_contexts
            .iter()
            .find(|c| c.name == "Identity")
            .unwrap();
        let aggregate = identity
            .aggregates
            .iter()
            .find(|a| a.name == "UserAggregate")
            .unwrap();
        assert_eq!(aggregate.root_entity, "User");
        assert_eq!(aggregate.entities, vec!["User"]);
        assert_eq!(aggregate.value_objects, vec!["EmailAddress"]);
        assert_eq!(aggregate.ownership.team, "Identity Team");
    }

    #[test]
    fn test_upsert_policy_merges_links() {
        let ws = "/tmp/test-upsert-policy";
        let store = setup(ws);

        call_write_tool(
            ws,
            &store,
            "define",
            &json!({
                "kind": "policy",
                "context": "Identity",
                "name": "WelcomePolicy",
                "policy_kind": "process_manager",
                "triggers": ["UserRegistered"],
                "commands": ["SendWelcomeEmail"]
            }),
        );

        let result = call_write_tool(
            ws,
            &store,
            "define",
            &json!({
                "kind": "policy",
                "context": "Identity",
                "name": "WelcomePolicy",
                "commands": ["SendWelcomeEmail", "CreateAuditEntry"]
            }),
        );

        assert!(result.is_error.is_none());
        let loaded = store.load_desired(ws).unwrap().unwrap();
        let identity = loaded
            .bounded_contexts
            .iter()
            .find(|c| c.name == "Identity")
            .unwrap();
        let policy = identity
            .policies
            .iter()
            .find(|p| p.name == "WelcomePolicy")
            .unwrap();
        assert!(matches!(policy.kind, PolicyKind::ProcessManager));
        assert_eq!(policy.triggers, vec!["UserRegistered"]);
        assert_eq!(
            policy.commands,
            vec!["SendWelcomeEmail", "CreateAuditEntry"]
        );
    }

    #[test]
    fn test_upsert_read_model_merges_fields() {
        let ws = "/tmp/test-upsert-read-model";
        let store = setup(ws);

        call_write_tool(
            ws,
            &store,
            "define",
            &json!({
                "kind": "read_model",
                "context": "Identity",
                "name": "UserProfileView",
                "source": "User",
                "fields": [{"name": "id", "type": "UserId", "required": true}]
            }),
        );

        let result = call_write_tool(
            ws,
            &store,
            "define",
            &json!({
                "kind": "read_model",
                "context": "Identity",
                "name": "UserProfileView",
                "fields": [{"name": "email", "type": "String", "required": true}]
            }),
        );

        assert!(result.is_error.is_none());
        let loaded = store.load_desired(ws).unwrap().unwrap();
        let identity = loaded
            .bounded_contexts
            .iter()
            .find(|c| c.name == "Identity")
            .unwrap();
        let read_model = identity
            .read_models
            .iter()
            .find(|rm| rm.name == "UserProfileView")
            .unwrap();
        assert_eq!(read_model.fields.len(), 2);
        assert!(read_model.fields.iter().any(|f| f.name == "id"));
        assert!(read_model.fields.iter().any(|f| f.name == "email"));
    }

    #[test]
    fn test_upsert_external_system_and_decision() {
        let ws = "/tmp/test-upsert-boundaries";
        let store = setup(ws);

        let system_result = call_write_tool(
            ws,
            &store,
            "define",
            &json!({
                "kind": "external_system",
                "name": "Stripe",
                "description": "Payment processor",
                "kind_label": "saas",
                "consumed_by_contexts": ["Identity"],
                "rationale": "Delegates payments"
            }),
        );
        assert!(system_result.is_error.is_none());

        let decision_result = call_write_tool(
            ws,
            &store,
            "define",
            &json!({
                "kind": "architectural_decision",
                "name": "ADR-001",
                "title": "Use Stripe for payments",
                "status": "accepted",
                "scope": "payments",
                "date": "2026-03-06",
                "rationale": "Reduce PCI burden",
                "contexts": ["Identity"],
                "consequences": ["External dependency introduced"]
            }),
        );
        assert!(decision_result.is_error.is_none());

        let loaded = store.load_desired(ws).unwrap().unwrap();
        assert_eq!(loaded.external_systems.len(), 1);
        assert_eq!(loaded.external_systems[0].name, "Stripe");
        assert_eq!(
            loaded.external_systems[0].consumed_by_contexts,
            vec!["Identity"]
        );
        assert_eq!(loaded.architectural_decisions.len(), 1);
        assert!(matches!(
            loaded.architectural_decisions[0].status,
            DecisionStatus::Accepted
        ));
        assert_eq!(loaded.architectural_decisions[0].contexts, vec!["Identity"]);
    }

    #[test]
    fn test_remove_expressive_elements() {
        let ws = "/tmp/test-remove-expressive";
        let store = setup(ws);

        call_write_tool(
            ws,
            &store,
            "define",
            &json!({
                "kind": "aggregate", "context": "Identity", "name": "UserAggregate", "root_entity": "User"
            }),
        );
        call_write_tool(
            ws,
            &store,
            "define",
            &json!({
                "kind": "external_system", "name": "Stripe"
            }),
        );

        let rm_aggregate = call_write_tool(
            ws,
            &store,
            "define",
            &json!({
                "kind": "aggregate", "action": "remove", "context": "Identity", "name": "UserAggregate"
            }),
        );
        let rm_system = call_write_tool(
            ws,
            &store,
            "define",
            &json!({
                "kind": "external_system", "action": "remove", "name": "Stripe"
            }),
        );

        assert!(rm_aggregate.is_error.is_none());
        assert!(rm_system.is_error.is_none());

        let loaded = store.load_desired(ws).unwrap().unwrap();
        let identity = loaded
            .bounded_contexts
            .iter()
            .find(|c| c.name == "Identity")
            .unwrap();
        assert!(identity.aggregates.is_empty());
        assert!(loaded.external_systems.is_empty());
    }

    #[test]
    fn test_unknown_write_tool() {
        let store = test_store();
        let result = call_write_tool("/tmp/test-ws", &store, "nonexistent", &json!({}));
        assert_eq!(result.is_error, Some(true));
    }

    #[test]
    fn test_auto_save_on_mutation() {
        let ws = "/tmp/test-autosave";
        let store = setup(ws);
        call_write_tool(
            ws,
            &store,
            "define",
            &json!({"kind": "bounded_context", "name": "Billing", "description": "Billing context"}),
        );
        let loaded = store.load_desired(ws).unwrap().unwrap();
        assert_eq!(loaded.bounded_contexts.len(), 2);
    }

    #[test]
    fn test_auto_save_not_on_error() {
        let store = test_store();
        let ws = "/tmp/test-autosave-err";
        let result = call_write_tool(
            ws,
            &store,
            "define",
            &json!({"kind": "entity", "context": "Nonexistent", "name": "Foo"}),
        );
        assert_eq!(result.is_error, Some(true));
        assert!(store.load_desired(ws).unwrap().is_none());
    }

    #[test]
    fn test_draft_refactoring_plan_uses_baseline() {
        let ws = "/tmp/test-baseline";
        let store = setup(ws);
        call_write_tool(
            ws,
            &store,
            "define",
            &json!({"kind": "bounded_context", "name": "Identity", "description": "Updated"}),
        );
        let result = call_write_tool(ws, &store, "refactor", &json!({}));
        let text = match &result.content[0] {
            ContentBlock::Text { text } => text,
        };
        assert!(text.contains("pending_changes"));
    }

    #[test]
    fn test_draft_plan_does_not_auto_advance() {
        let ws = "/tmp/test-no-advance";
        let store = setup(ws);
        call_write_tool(
            ws,
            &store,
            "define",
            &json!({"kind": "bounded_context", "name": "Identity", "description": "Updated"}),
        );
        let result = call_write_tool(ws, &store, "refactor", &json!({}));
        let text = match &result.content[0] {
            ContentBlock::Text { text } => text,
        };
        assert!(text.contains("pending_changes"));
        let result = call_write_tool(ws, &store, "refactor", &json!({}));
        let text = match &result.content[0] {
            ContentBlock::Text { text } => text,
        };
        assert!(text.contains("pending_changes"));
    }

    #[test]
    fn test_accept_then_plan_shows_in_sync() {
        let ws = "/tmp/test-accept-sync";
        let store = setup(ws);
        call_write_tool(
            ws,
            &store,
            "define",
            &json!({"kind": "bounded_context", "name": "Identity", "description": "Updated"}),
        );
        let result = call_write_tool(ws, &store, "refactor", &json!({"action": "accept"}));
        let text = match &result.content[0] {
            ContentBlock::Text { text } => text,
        };
        assert!(text.contains("accepted"));
        let result = call_write_tool(ws, &store, "refactor", &json!({}));
        let text = match &result.content[0] {
            ContentBlock::Text { text } => text,
        };
        assert!(text.contains("in_sync"));
    }

    #[test]
    fn test_reset_reverts_desired() {
        let ws = "/tmp/test-reset-wt";
        let store = setup(ws);
        call_write_tool(
            ws,
            &store,
            "define",
            &json!({"kind": "bounded_context", "name": "Identity", "description": "Original"}),
        );
        call_write_tool(ws, &store, "refactor", &json!({"action": "accept"}));
        call_write_tool(
            ws,
            &store,
            "define",
            &json!({"kind": "bounded_context", "name": "Billing", "description": "New context"}),
        );
        let desired = store.load_desired(ws).unwrap().unwrap();
        assert_eq!(desired.bounded_contexts.len(), 2);
        let result = call_write_tool(ws, &store, "refactor", &json!({"action": "reset"}));
        let text = match &result.content[0] {
            ContentBlock::Text { text } => text,
        };
        assert!(text.contains("reset"));
        let reset = store.load_desired(ws).unwrap().unwrap();
        assert_eq!(reset.bounded_contexts.len(), 1);
    }

    #[test]
    fn test_update_service_merges_methods() {
        let ws = "/tmp/test-merge-methods";
        let store = setup(ws);
        call_write_tool(
            ws,
            &store,
            "define",
            &json!({
                "kind": "service", "context": "Identity", "name": "AuthService",
                "service_kind": "application",
                "methods": [{"name": "login", "return_type": "Token"}]
            }),
        );
        call_write_tool(
            ws,
            &store,
            "define",
            &json!({
                "kind": "service", "context": "Identity", "name": "AuthService",
                "methods": [{"name": "logout", "return_type": "void"}]
            }),
        );
        let loaded = store.load_desired(ws).unwrap().unwrap();
        let identity = loaded
            .bounded_contexts
            .iter()
            .find(|c| c.name == "Identity")
            .unwrap();
        let svc = identity
            .services
            .iter()
            .find(|s| s.name == "AuthService")
            .unwrap();
        assert_eq!(svc.methods.len(), 2);
    }

    #[test]
    fn test_remove_bounded_context() {
        let ws = "/tmp/test-rm-bc";
        let store = setup(ws);
        let result = call_write_tool(
            ws,
            &store,
            "define",
            &json!({"kind": "bounded_context", "action": "remove", "name": "Identity"}),
        );
        assert!(result.is_error.is_none());
        // After removing the only context, the model exists but has 0 contexts
        let loaded = store.load_desired(ws).unwrap().unwrap();
        assert_eq!(loaded.bounded_contexts.len(), 0);
    }

    #[test]
    fn test_missing_kind() {
        let store = test_store();
        let result = call_write_tool("/tmp/test-ws", &store, "define", &json!({"name": "Foo"}));
        assert_eq!(result.is_error, Some(true));
    }

    #[test]
    fn test_diagnose_returns_structured_report() {
        let ws = "/tmp/test-diagnose";
        let store = setup(ws);

        // diagnose on a model with data
        let result = call_write_tool(ws, &store, "refactor", &json!({"action": "diagnose"}));
        assert!(result.is_error.is_none() || result.is_error == Some(false));

        let text = match &result.content[0] {
            crate::mcp::protocol::ContentBlock::Text { text } => text.clone(),
        };
        let report: serde_json::Value =
            serde_json::from_str(&text).expect("diagnose must return valid JSON");

        // Must have required top-level fields
        assert!(
            report.get("health_score").is_some(),
            "must have health_score"
        );
        assert!(report.get("invariants").is_some(), "must have invariants");
        assert!(
            report.get("next_actions").is_some(),
            "must have next_actions"
        );
        assert!(
            report.get("has_desired_model").is_some(),
            "must have has_desired_model"
        );
        assert!(report.get("loop_hint").is_some(), "must have loop_hint");

        // Invariants must have all 4 checks
        let inv = &report["invariants"];
        assert!(inv.get("circular_deps").is_some());
        assert!(inv.get("layer_violations").is_some());
        assert!(inv.get("aggregate_quality").is_some());
        assert!(inv.get("policy_violations").is_some());

        // next_actions must be an array with at least one action
        let actions = report["next_actions"]
            .as_array()
            .expect("next_actions must be array");
        assert!(
            !actions.is_empty(),
            "diagnose must suggest at least one next action"
        );

        // Each action must have priority, tool, and reason
        for action in actions {
            assert!(
                action.get("priority").is_some(),
                "action must have priority"
            );
            assert!(action.get("tool").is_some(), "action must have tool");
            assert!(action.get("reason").is_some(), "action must have reason");
        }
    }

    #[test]
    fn test_diagnose_on_empty_store() {
        let store = test_store();
        let result = call_write_tool(
            "/tmp/test-diagnose-empty",
            &store,
            "refactor",
            &json!({"action": "diagnose"}),
        );
        assert!(result.is_error.is_none() || result.is_error == Some(false));

        let text = match &result.content[0] {
            crate::mcp::protocol::ContentBlock::Text { text } => text.clone(),
        };
        let report: serde_json::Value =
            serde_json::from_str(&text).expect("diagnose must return valid JSON");

        // No current model → should suggest sync
        assert_eq!(report["has_actual_model"], false);
        let actions = report["next_actions"].as_array().unwrap();
        let first = &actions[0];
        assert_eq!(
            first["tool"], "sync",
            "first action on empty store should be sync"
        );
    }
}
