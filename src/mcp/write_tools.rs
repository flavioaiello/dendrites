use serde_json::{json, Value};

use crate::domain::model::*;
use crate::domain::to_snake;
use crate::mcp::protocol::*;
use crate::store::Store;

/// Returns the list of write tools the Dendrites server exposes.
pub fn list_write_tools() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "set_model".into(),
            description: "Create, update, or remove elements in the desired domain model. \
                          As you process code or conversation, call this tool whenever you \
                          discover domain relationships (entities, aggregates, services, events, \
                          value objects, repositories). \
                          These updates build the desired model iteratively. \
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
            name: "refactor".into(),
            description: "Manage the refactoring lifecycle between actual and desired domain models. \
                          Actions: \
                          'plan' (default) — diff actual vs desired and produce a full refactoring \
                          plan with code actions, file paths, priorities, and migration notes. \
                          'accept' — after implementing the refactoring, promote desired → actual. \
                          'reset' — discard desired changes, revert desired → actual. \
                          'scan' — scan the workspace source code (AST extraction) and populate \
                          the actual model from what is really implemented. Guided by the desired \
                          model's bounded contexts and module_path mappings."
                .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["plan", "accept", "reset", "scan"],
                        "description": "Refactoring lifecycle action (default: plan)"
                    }
                },
                "required": []
            }),
        },
    ]
}

/// List of tool names that mutate the model and should trigger auto-save.
const MUTATION_TOOLS: &[&str] = &["set_model"];

/// Dispatches a write tool call. Mutations are auto-saved to the store.
pub fn call_write_tool(
    workspace_path: &str,
    store: &Store,
    name: &str,
    args: &Value,
) -> ToolCallResult {
    let mut model = store.load_desired(workspace_path).ok().flatten()
        .unwrap_or_else(|| DomainModel::empty(workspace_path));

    let result = dispatch_write_tool(&mut model, workspace_path, store, name, args);

    // Auto-save after successful mutations
    if result.is_error.is_none() && MUTATION_TOOLS.contains(&name)
        && let Err(e) = store.save_desired(workspace_path, &model)
    {
        return error_result(format!("Mutation succeeded but save failed: {e}"));
    }

    result
}

fn dispatch_write_tool(
    model: &mut DomainModel,
    workspace_path: &str,
    store: &Store,
    name: &str,
    args: &Value,
) -> ToolCallResult {
    match name {
        "set_model" => {
            let kind = arg_str(args, "kind");
            let action = args
                .get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("upsert");

            match (kind.as_str(), action) {
                ("bounded_context", "upsert") => {
                    upsert_bounded_context(model, args)
                },
                ("bounded_context", "remove") => remove_bounded_context(model, args),
                ("entity", "upsert") => {
                    upsert_entity(model, args)
                },
                ("entity", "remove") => remove_entity(model, args),
                ("service", "upsert") => {
                    upsert_service(model, args)
                },
                ("service", "remove") => remove_service(model, args),
                ("event", "upsert") => {
                    upsert_event(model, args)
                },
                ("event", "remove") => remove_event(model, args),
                ("value_object", "upsert") => {
                    upsert_value_object(model, args)
                },
                ("value_object", "remove") => remove_value_object(model, args),
                ("repository", "upsert") => {
                    upsert_repository(model, args)
                },
                ("repository", "remove") => remove_repository(model, args),
                    ("aggregate", "upsert") => upsert_aggregate(model, args),
                    ("aggregate", "remove") => remove_aggregate(model, args),
                    ("policy", "upsert") => upsert_policy(model, args),
                    ("policy", "remove") => remove_policy(model, args),
                    ("read_model", "upsert") => upsert_read_model(model, args),
                    ("read_model", "remove") => remove_read_model(model, args),
                    ("external_system", "upsert") => upsert_external_system(model, args),
                    ("external_system", "remove") => remove_external_system(model, args),
                    ("architectural_decision", "upsert") => upsert_architectural_decision(model, args),
                    ("architectural_decision", "remove") => remove_architectural_decision(model, args),
                ("", _) => error_result("'kind' is required"),
                (_, action) => error_result(format!("Unknown action '{action}' for kind '{kind}'")),
            }
        }

        "refactor" => {
            let action = args
                .get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("plan");

            match action {
                "plan" => {
                    // PHASE 3 GRAPH MIGRATION: Delegate diffing into Datalog
                    match store.diff_graph(workspace_path) {
                        Ok(diff_data) => {
                            let changes = diff_data["pending_changes"].as_array().unwrap();
                            if changes.is_empty() {
                                text_result(
                                    json!({
                                        "status": "in_sync",
                                        "message": "Actual and desired models are in sync. Nothing to refactor."
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
                        Ok(()) => text_result(
                            json!({
                                "status": "accepted",
                                "message": "Desired model promoted to actual. Models are now in sync."
                            })
                            .to_string(),
                        ),
                        Err(e) => error_result(format!("Failed to accept: {e}")),
                    }
                }

                "reset" => {
                    match store.reset(workspace_path) {
                        Ok(Some(_)) => text_result(
                            json!({
                                "status": "reset",
                                "message": "Desired model reverted to actual. All pending changes discarded."
                            })
                            .to_string(),
                        ),
                        Ok(None) => error_result("No actual model to reset to"),
                        Err(e) => error_result(format!("Failed to reset: {e}")),
                    }
                }

                "scan" => {
                    use crate::domain::analyze::scan_actual_model;

                    let workspace_root = std::path::Path::new(workspace_path);
                    let desired = store.load_desired(workspace_path).ok().flatten()
                        .unwrap_or_else(|| DomainModel::empty(workspace_path));

                    if desired.bounded_contexts.is_empty() {
                        return error_result(
                            "No bounded contexts in the desired model. \
                             Seed the model first with set_model before scanning."
                        );
                    }

                    match scan_actual_model(workspace_root, &desired) {
                        Ok(actual) => {
                            let entity_count: usize = actual.bounded_contexts.iter().map(|bc| bc.entities.len()).sum();
                            let vo_count: usize = actual.bounded_contexts.iter().map(|bc| bc.value_objects.len()).sum();
                            let svc_count: usize = actual.bounded_contexts.iter().map(|bc| bc.services.len()).sum();
                            let repo_count: usize = actual.bounded_contexts.iter().map(|bc| bc.repositories.len()).sum();
                            let event_count: usize = actual.bounded_contexts.iter().map(|bc| bc.events.len()).sum();

                            match store.save_actual(workspace_path, &actual) {
                                Ok(()) => text_result(
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
                                ),
                                Err(e) => error_result(format!("Scan succeeded but save failed: {e}")),
                            }
                        }
                        Err(e) => error_result(format!("Scan failed: {e}")),
                    }
                }

                _ => error_result(format!("Unknown action '{action}'. Use 'plan', 'accept', 'reset', or 'scan'.")),
            }
        }

        _ => error_result(format!("Unknown write tool: {name}")),
    }
}

// ─── Kind handlers ─────────────────────────────────────────────────────────

fn upsert_bounded_context(model: &mut DomainModel, args: &Value) -> ToolCallResult {
    let ctx_name = arg_str(args, "name");
    if ctx_name.is_empty() {
        return error_result("'name' is required");
    }

    let existing = model
        .bounded_contexts
        .iter_mut()
        .find(|bc| bc.name.eq_ignore_ascii_case(&ctx_name));

    let action = match existing {
        Some(bc) => {
            if let Some(desc) = args.get("description").and_then(|v| v.as_str()) {
                bc.description = desc.to_string();
            }
            if let Some(mp) = args.get("module_path").and_then(|v| v.as_str()) {
                bc.module_path = mp.to_string();
            }
            if args.get("ownership").is_some() {
                bc.ownership = parse_ownership(args.get("ownership"));
            }
            if let Some(deps) = args.get("dependencies").and_then(|v| v.as_array()) {
                bc.dependencies = deps
                    .iter()
                    .filter_map(|d| d.as_str().map(String::from))
                    .collect();
            }
            "updated"
        }
        None => {
            model.bounded_contexts.push(BoundedContext {
                name: ctx_name.clone(),
                description: arg_str(args, "description"),
                module_path: arg_str(args, "module_path"),
                ownership: parse_ownership(args.get("ownership")),
                aggregates: vec![],
                policies: vec![],
                read_models: vec![],
                entities: vec![],
                value_objects: vec![],
                services: vec![],
                repositories: vec![],
                events: vec![],
                dependencies: args
                    .get("dependencies")
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|d| d.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default(),
            });
            "created"
        }
    };

    // Inline dependency validation: check if declared deps reference existing contexts
    let bc = model.bounded_contexts.iter().find(|bc| bc.name.eq_ignore_ascii_case(&ctx_name)).unwrap();
    let all_ctx_names: Vec<&str> = model.bounded_contexts.iter().map(|c| c.name.as_str()).collect();
    let unknown_deps: Vec<&str> = bc.dependencies.iter()
        .filter(|d| !all_ctx_names.iter().any(|c| c.eq_ignore_ascii_case(d)))
        .map(|d| d.as_str())
        .collect();

    let mut result = json!({
        "message": format!("{} bounded context '{}'", if action == "created" { "Created" } else { "Updated" }, ctx_name),
    });

    if !unknown_deps.is_empty() {
        result["dependency_warnings"] = json!(unknown_deps.iter()
            .map(|d| format!("Dependency '{}' references an undefined bounded context", d))
            .collect::<Vec<_>>());
    }

    text_result(result.to_string())
}

fn remove_bounded_context(model: &mut DomainModel, args: &Value) -> ToolCallResult {
    let ctx_name = arg_str(args, "name");
    let before = model.bounded_contexts.len();
    model
        .bounded_contexts
        .retain(|bc| !bc.name.eq_ignore_ascii_case(&ctx_name));
    if model.bounded_contexts.len() < before {
        text_result(format!("Removed bounded context '{ctx_name}'"))
    } else {
        error_result(format!("Bounded context '{ctx_name}' not found"))
    }
}

fn upsert_entity(model: &mut DomainModel, args: &Value) -> ToolCallResult {
    let ctx_name = arg_str(args, "context");
    let entity_name = arg_str(args, "name");

    let bc = match model
        .bounded_contexts
        .iter_mut()
        .find(|bc| bc.name.eq_ignore_ascii_case(&ctx_name))
    {
        Some(bc) => bc,
        None => return error_result(format!("Bounded context '{ctx_name}' not found")),
    };

    let existing = bc
        .entities
        .iter_mut()
        .find(|e| e.name.eq_ignore_ascii_case(&entity_name));

    match existing {
        Some(entity) => {
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
            text_result(json!({
                "message": format!("Updated entity '{}' in '{}'", entity_name, ctx_name),
                "suggested_path": suggest_path(&model.conventions.file_structure.pattern, &ctx_name, "entity", &entity_name)
            }).to_string())
        }
        None => {
            let entity = Entity {
                name: entity_name.clone(),
                description: arg_str(args, "description"),
                aggregate_root: args
                    .get("aggregate_root")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                fields: parse_fields(args.get("fields")),
                methods: parse_methods(args.get("methods")),
                invariants: args
                    .get("invariants")
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|i| i.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default(),
            };
            bc.entities.push(entity);
            text_result(json!({
                "message": format!("Created entity '{}' in '{}'", entity_name, ctx_name),
                "suggested_path": suggest_path(&model.conventions.file_structure.pattern, &ctx_name, "entity", &entity_name)
            }).to_string())
        }
    }
}

fn remove_entity(model: &mut DomainModel, args: &Value) -> ToolCallResult {
    let ctx_name = arg_str(args, "context");
    let entity_name = arg_str(args, "name");
    let bc = match model
        .bounded_contexts
        .iter_mut()
        .find(|bc| bc.name.eq_ignore_ascii_case(&ctx_name))
    {
        Some(bc) => bc,
        None => return error_result(format!("Bounded context '{ctx_name}' not found")),
    };
    let before = bc.entities.len();
    bc.entities
        .retain(|e| !e.name.eq_ignore_ascii_case(&entity_name));
    if bc.entities.len() < before {
        text_result(format!("Removed entity '{entity_name}' from '{ctx_name}'"))
    } else {
        error_result(format!("Entity '{entity_name}' not found in '{ctx_name}'"))
    }
}

fn upsert_service(model: &mut DomainModel, args: &Value) -> ToolCallResult {
    let ctx_name = arg_str(args, "context");
    let svc_name = arg_str(args, "name");

    let bc = match model
        .bounded_contexts
        .iter_mut()
        .find(|bc| bc.name.eq_ignore_ascii_case(&ctx_name))
    {
        Some(bc) => bc,
        None => return error_result(format!("Bounded context '{ctx_name}' not found")),
    };

    let kind = match args
        .get("service_kind")
        .and_then(|v| v.as_str())
        .unwrap_or("domain")
    {
        "application" => ServiceKind::Application,
        "infrastructure" => ServiceKind::Infrastructure,
        _ => ServiceKind::Domain,
    };

    let existing = bc
        .services
        .iter_mut()
        .find(|s| s.name.eq_ignore_ascii_case(&svc_name));

    match existing {
        Some(svc) => {
            if let Some(desc) = args.get("description").and_then(|v| v.as_str()) {
                svc.description = desc.to_string();
            }
            svc.kind = kind;
            if let Some(deps) = args.get("dependencies").and_then(|v| v.as_array()) {
                svc.dependencies = deps
                    .iter()
                    .filter_map(|d| d.as_str().map(String::from))
                    .collect();
            }
            if let Some(methods) = args.get("methods").and_then(|v| v.as_array()) {
                merge_methods(&mut svc.methods, methods);
            }
            text_result(json!({
                "message": format!("Updated service '{}' in '{}'", svc_name, ctx_name),
                "suggested_path": suggest_path(&model.conventions.file_structure.pattern, &ctx_name, "service", &svc_name)
            }).to_string())
        }
        None => {
            bc.services.push(Service {
                name: svc_name.clone(),
                description: arg_str(args, "description"),
                kind,
                methods: parse_methods(args.get("methods")),
                dependencies: args
                    .get("dependencies")
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|d| d.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default(),
            });
            text_result(json!({
                "message": format!("Created service '{}' in '{}'", svc_name, ctx_name),
                "suggested_path": suggest_path(&model.conventions.file_structure.pattern, &ctx_name, "service", &svc_name)
            }).to_string())
        }
    }
}

fn remove_service(model: &mut DomainModel, args: &Value) -> ToolCallResult {
    let ctx_name = arg_str(args, "context");
    let svc_name = arg_str(args, "name");
    let bc = match model
        .bounded_contexts
        .iter_mut()
        .find(|bc| bc.name.eq_ignore_ascii_case(&ctx_name))
    {
        Some(bc) => bc,
        None => return error_result(format!("Bounded context '{ctx_name}' not found")),
    };
    let before = bc.services.len();
    bc.services
        .retain(|s| !s.name.eq_ignore_ascii_case(&svc_name));
    if bc.services.len() < before {
        text_result(format!("Removed service '{svc_name}' from '{ctx_name}'"))
    } else {
        error_result(format!("Service '{svc_name}' not found in '{ctx_name}'"))
    }
}

fn upsert_event(model: &mut DomainModel, args: &Value) -> ToolCallResult {
    let ctx_name = arg_str(args, "context");
    let event_name = arg_str(args, "name");

    let bc = match model
        .bounded_contexts
        .iter_mut()
        .find(|bc| bc.name.eq_ignore_ascii_case(&ctx_name))
    {
        Some(bc) => bc,
        None => return error_result(format!("Bounded context '{ctx_name}' not found")),
    };

    let existing = bc
        .events
        .iter_mut()
        .find(|e| e.name.eq_ignore_ascii_case(&event_name));

    match existing {
        Some(evt) => {
            if let Some(desc) = args.get("description").and_then(|v| v.as_str()) {
                evt.description = desc.to_string();
            }
            if let Some(src) = args.get("source").and_then(|v| v.as_str()) {
                evt.source = src.to_string();
            }
            if let Some(fields) = args.get("fields").and_then(|v| v.as_array()) {
                merge_fields(&mut evt.fields, fields);
            }
            text_result(json!({
                "message": format!("Updated event '{}' in '{}'", event_name, ctx_name),
                "suggested_path": suggest_path(&model.conventions.file_structure.pattern, &ctx_name, "event", &event_name)
            }).to_string())
        }
        None => {
            bc.events.push(DomainEvent {
                name: event_name.clone(),
                description: arg_str(args, "description"),
                fields: parse_fields(args.get("fields")),
                source: arg_str(args, "source"),
            });
            text_result(json!({
                "message": format!("Created event '{}' in '{}'", event_name, ctx_name),
                "suggested_path": suggest_path(&model.conventions.file_structure.pattern, &ctx_name, "event", &event_name)
            }).to_string())
        }
    }
}

fn remove_event(model: &mut DomainModel, args: &Value) -> ToolCallResult {
    let ctx_name = arg_str(args, "context");
    let event_name = arg_str(args, "name");
    let bc = match model
        .bounded_contexts
        .iter_mut()
        .find(|bc| bc.name.eq_ignore_ascii_case(&ctx_name))
    {
        Some(bc) => bc,
        None => return error_result(format!("Bounded context '{ctx_name}' not found")),
    };
    let before = bc.events.len();
    bc.events
        .retain(|e| !e.name.eq_ignore_ascii_case(&event_name));
    if bc.events.len() < before {
        text_result(format!("Removed event '{event_name}' from '{ctx_name}'"))
    } else {
        error_result(format!("Event '{event_name}' not found in '{ctx_name}'"))
    }
}

fn upsert_value_object(model: &mut DomainModel, args: &Value) -> ToolCallResult {
    let ctx_name = arg_str(args, "context");
    let vo_name = arg_str(args, "name");

    let bc = match model
        .bounded_contexts
        .iter_mut()
        .find(|bc| bc.name.eq_ignore_ascii_case(&ctx_name))
    {
        Some(bc) => bc,
        None => return error_result(format!("Bounded context '{ctx_name}' not found")),
    };

    let existing = bc
        .value_objects
        .iter_mut()
        .find(|v| v.name.eq_ignore_ascii_case(&vo_name));

    match existing {
        Some(vo) => {
            if let Some(desc) = args.get("description").and_then(|v| v.as_str()) {
                vo.description = desc.to_string();
            }
            if let Some(fields) = args.get("fields").and_then(|v| v.as_array()) {
                merge_fields(&mut vo.fields, fields);
            }
            if let Some(rules) = args.get("validation_rules").and_then(|v| v.as_array()) {
                for rule in rules {
                    if let Some(s) = rule.as_str()
                        && !vo.validation_rules.iter().any(|r| r == s)
                    {
                        vo.validation_rules.push(s.to_string());
                    }
                }
            }
            text_result(json!({
                "message": format!("Updated value object '{}' in '{}'", vo_name, ctx_name),
                "suggested_path": suggest_path(&model.conventions.file_structure.pattern, &ctx_name, "value_object", &vo_name)
            }).to_string())
        }
        None => {
            bc.value_objects.push(ValueObject {
                name: vo_name.clone(),
                description: arg_str(args, "description"),
                fields: parse_fields(args.get("fields")),
                validation_rules: args
                    .get("validation_rules")
                    .and_then(|v| v.as_array())
                    .map(|a| a.iter().filter_map(|r| r.as_str().map(String::from)).collect())
                    .unwrap_or_default(),
            });
            text_result(json!({
                "message": format!("Created value object '{}' in '{}'", vo_name, ctx_name),
                "suggested_path": suggest_path(&model.conventions.file_structure.pattern, &ctx_name, "value_object", &vo_name)
            }).to_string())
        }
    }
}

fn remove_value_object(model: &mut DomainModel, args: &Value) -> ToolCallResult {
    let ctx_name = arg_str(args, "context");
    let vo_name = arg_str(args, "name");
    let bc = match model
        .bounded_contexts
        .iter_mut()
        .find(|bc| bc.name.eq_ignore_ascii_case(&ctx_name))
    {
        Some(bc) => bc,
        None => return error_result(format!("Bounded context '{ctx_name}' not found")),
    };
    let before = bc.value_objects.len();
    bc.value_objects.retain(|v| !v.name.eq_ignore_ascii_case(&vo_name));
    if bc.value_objects.len() < before {
        text_result(format!("Removed value object '{vo_name}' from '{ctx_name}'"))
    } else {
        error_result(format!("Value object '{vo_name}' not found in '{ctx_name}'"))
    }
}

fn upsert_repository(model: &mut DomainModel, args: &Value) -> ToolCallResult {
    let ctx_name = arg_str(args, "context");
    let repo_name = arg_str(args, "name");

    let bc = match model
        .bounded_contexts
        .iter_mut()
        .find(|bc| bc.name.eq_ignore_ascii_case(&ctx_name))
    {
        Some(bc) => bc,
        None => return error_result(format!("Bounded context '{ctx_name}' not found")),
    };

    let existing = bc
        .repositories
        .iter_mut()
        .find(|r| r.name.eq_ignore_ascii_case(&repo_name));

    match existing {
        Some(repo) => {
            if let Some(agg) = args.get("aggregate").and_then(|v| v.as_str()) {
                repo.aggregate = agg.to_string();
            }
            if let Some(methods) = args.get("methods").and_then(|v| v.as_array()) {
                merge_methods(&mut repo.methods, methods);
            }
            text_result(json!({
                "message": format!("Updated repository '{}' in '{}'", repo_name, ctx_name),
                "suggested_path": suggest_path(&model.conventions.file_structure.pattern, &ctx_name, "repository", &repo_name)
            }).to_string())
        }
        None => {
            bc.repositories.push(Repository {
                name: repo_name.clone(),
                aggregate: arg_str(args, "aggregate"),
                methods: parse_methods(args.get("methods")),
            });
            text_result(json!({
                "message": format!("Created repository '{}' in '{}'", repo_name, ctx_name),
                "suggested_path": suggest_path(&model.conventions.file_structure.pattern, &ctx_name, "repository", &repo_name)
            }).to_string())
        }
    }
}

fn remove_repository(model: &mut DomainModel, args: &Value) -> ToolCallResult {
    let ctx_name = arg_str(args, "context");
    let repo_name = arg_str(args, "name");
    let bc = match model
        .bounded_contexts
        .iter_mut()
        .find(|bc| bc.name.eq_ignore_ascii_case(&ctx_name))
    {
        Some(bc) => bc,
        None => return error_result(format!("Bounded context '{ctx_name}' not found")),
    };
    let before = bc.repositories.len();
    bc.repositories.retain(|r| !r.name.eq_ignore_ascii_case(&repo_name));
    if bc.repositories.len() < before {
        text_result(format!("Removed repository '{repo_name}' from '{ctx_name}'"))
    } else {
        error_result(format!("Repository '{repo_name}' not found in '{ctx_name}'"))
    }
}

fn upsert_aggregate(model: &mut DomainModel, args: &Value) -> ToolCallResult {
    let ctx_name = arg_str(args, "context");
    let aggregate_name = arg_str(args, "name");

    let bc = match model
        .bounded_contexts
        .iter_mut()
        .find(|bc| bc.name.eq_ignore_ascii_case(&ctx_name))
    {
        Some(bc) => bc,
        None => return error_result(format!("Bounded context '{ctx_name}' not found")),
    };

    let entities = parse_string_array(args.get("entities"));
    let value_objects = parse_string_array(args.get("value_objects"));
    let ownership = parse_ownership(args.get("ownership"));

    match bc
        .aggregates
        .iter_mut()
        .find(|a| a.name.eq_ignore_ascii_case(&aggregate_name))
    {
        Some(aggregate) => {
            if let Some(description) = args.get("description").and_then(|v| v.as_str()) {
                aggregate.description = description.to_string();
            }
            if let Some(root_entity) = args.get("root_entity").and_then(|v| v.as_str()) {
                aggregate.root_entity = root_entity.to_string();
            }
            if args.get("entities").is_some() {
                aggregate.entities = entities;
            }
            if args.get("value_objects").is_some() {
                aggregate.value_objects = value_objects;
            }
            if args.get("ownership").is_some() {
                aggregate.ownership = ownership;
            }
            text_result(json!({
                "message": format!("Updated aggregate '{}' in '{}'", aggregate_name, ctx_name)
            }).to_string())
        }
        None => {
            bc.aggregates.push(Aggregate {
                name: aggregate_name.clone(),
                description: arg_str(args, "description"),
                root_entity: arg_str(args, "root_entity"),
                entities,
                value_objects,
                ownership,
            });
            text_result(json!({
                "message": format!("Created aggregate '{}' in '{}'", aggregate_name, ctx_name)
            }).to_string())
        }
    }
}

fn remove_aggregate(model: &mut DomainModel, args: &Value) -> ToolCallResult {
    let ctx_name = arg_str(args, "context");
    let aggregate_name = arg_str(args, "name");
    let bc = match model
        .bounded_contexts
        .iter_mut()
        .find(|bc| bc.name.eq_ignore_ascii_case(&ctx_name))
    {
        Some(bc) => bc,
        None => return error_result(format!("Bounded context '{ctx_name}' not found")),
    };
    let before = bc.aggregates.len();
    bc.aggregates
        .retain(|a| !a.name.eq_ignore_ascii_case(&aggregate_name));
    if bc.aggregates.len() < before {
        text_result(format!("Removed aggregate '{aggregate_name}' from '{ctx_name}'"))
    } else {
        error_result(format!("Aggregate '{aggregate_name}' not found in '{ctx_name}'"))
    }
}

fn upsert_policy(model: &mut DomainModel, args: &Value) -> ToolCallResult {
    let ctx_name = arg_str(args, "context");
    let policy_name = arg_str(args, "name");

    let bc = match model
        .bounded_contexts
        .iter_mut()
        .find(|bc| bc.name.eq_ignore_ascii_case(&ctx_name))
    {
        Some(bc) => bc,
        None => return error_result(format!("Bounded context '{ctx_name}' not found")),
    };

    let triggers = parse_string_array(args.get("triggers"));
    let commands = parse_string_array(args.get("commands"));
    let ownership = parse_ownership(args.get("ownership"));

    match bc
        .policies
        .iter_mut()
        .find(|p| p.name.eq_ignore_ascii_case(&policy_name))
    {
        Some(policy) => {
            if let Some(description) = args.get("description").and_then(|v| v.as_str()) {
                policy.description = description.to_string();
            }
            if let Some(kind) = args.get("policy_kind").and_then(|v| v.as_str()) {
                policy.kind = parse_policy_kind(kind);
            }
            if args.get("triggers").is_some() {
                policy.triggers = triggers;
            }
            if args.get("commands").is_some() {
                policy.commands = commands;
            }
            if args.get("ownership").is_some() {
                policy.ownership = ownership;
            }
            text_result(json!({
                "message": format!("Updated policy '{}' in '{}'", policy_name, ctx_name)
            }).to_string())
        }
        None => {
            bc.policies.push(Policy {
                name: policy_name.clone(),
                description: arg_str(args, "description"),
                kind: parse_policy_kind(&arg_str(args, "policy_kind")),
                triggers,
                commands,
                ownership,
            });
            text_result(json!({
                "message": format!("Created policy '{}' in '{}'", policy_name, ctx_name)
            }).to_string())
        }
    }
}

fn remove_policy(model: &mut DomainModel, args: &Value) -> ToolCallResult {
    let ctx_name = arg_str(args, "context");
    let policy_name = arg_str(args, "name");
    let bc = match model
        .bounded_contexts
        .iter_mut()
        .find(|bc| bc.name.eq_ignore_ascii_case(&ctx_name))
    {
        Some(bc) => bc,
        None => return error_result(format!("Bounded context '{ctx_name}' not found")),
    };
    let before = bc.policies.len();
    bc.policies
        .retain(|p| !p.name.eq_ignore_ascii_case(&policy_name));
    if bc.policies.len() < before {
        text_result(format!("Removed policy '{policy_name}' from '{ctx_name}'"))
    } else {
        error_result(format!("Policy '{policy_name}' not found in '{ctx_name}'"))
    }
}

fn upsert_read_model(model: &mut DomainModel, args: &Value) -> ToolCallResult {
    let ctx_name = arg_str(args, "context");
    let read_model_name = arg_str(args, "name");

    let bc = match model
        .bounded_contexts
        .iter_mut()
        .find(|bc| bc.name.eq_ignore_ascii_case(&ctx_name))
    {
        Some(bc) => bc,
        None => return error_result(format!("Bounded context '{ctx_name}' not found")),
    };

    let ownership = parse_ownership(args.get("ownership"));
    match bc
        .read_models
        .iter_mut()
        .find(|rm| rm.name.eq_ignore_ascii_case(&read_model_name))
    {
        Some(read_model) => {
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
                read_model.ownership = ownership;
            }
            text_result(json!({
                "message": format!("Updated read model '{}' in '{}'", read_model_name, ctx_name)
            }).to_string())
        }
        None => {
            bc.read_models.push(ReadModel {
                name: read_model_name.clone(),
                description: arg_str(args, "description"),
                source: arg_str(args, "source"),
                fields: parse_fields(args.get("fields")),
                ownership,
            });
            text_result(json!({
                "message": format!("Created read model '{}' in '{}'", read_model_name, ctx_name)
            }).to_string())
        }
    }
}

fn remove_read_model(model: &mut DomainModel, args: &Value) -> ToolCallResult {
    let ctx_name = arg_str(args, "context");
    let read_model_name = arg_str(args, "name");
    let bc = match model
        .bounded_contexts
        .iter_mut()
        .find(|bc| bc.name.eq_ignore_ascii_case(&ctx_name))
    {
        Some(bc) => bc,
        None => return error_result(format!("Bounded context '{ctx_name}' not found")),
    };
    let before = bc.read_models.len();
    bc.read_models
        .retain(|rm| !rm.name.eq_ignore_ascii_case(&read_model_name));
    if bc.read_models.len() < before {
        text_result(format!("Removed read model '{read_model_name}' from '{ctx_name}'"))
    } else {
        error_result(format!("Read model '{read_model_name}' not found in '{ctx_name}'"))
    }
}

fn upsert_external_system(model: &mut DomainModel, args: &Value) -> ToolCallResult {
    let system_name = arg_str(args, "name");
    let ownership = parse_ownership(args.get("ownership"));
    let consumed_by_contexts = parse_string_array(args.get("consumed_by_contexts"));

    match model
        .external_systems
        .iter_mut()
        .find(|s| s.name.eq_ignore_ascii_case(&system_name))
    {
        Some(system) => {
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
                system.consumed_by_contexts = consumed_by_contexts;
            }
            if args.get("ownership").is_some() {
                system.ownership = ownership;
            }
            text_result(json!({
                "message": format!("Updated external system '{}'", system_name)
            }).to_string())
        }
        None => {
            model.external_systems.push(ExternalSystem {
                name: system_name.clone(),
                description: arg_str(args, "description"),
                kind: arg_str(args, "kind_label"),
                consumed_by_contexts,
                rationale: arg_str(args, "rationale"),
                ownership,
            });
            text_result(json!({
                "message": format!("Created external system '{}'", system_name)
            }).to_string())
        }
    }
}

fn remove_external_system(model: &mut DomainModel, args: &Value) -> ToolCallResult {
    let system_name = arg_str(args, "name");
    let before = model.external_systems.len();
    model.external_systems
        .retain(|s| !s.name.eq_ignore_ascii_case(&system_name));
    if model.external_systems.len() < before {
        text_result(format!("Removed external system '{system_name}'"))
    } else {
        error_result(format!("External system '{system_name}' not found"))
    }
}

fn upsert_architectural_decision(model: &mut DomainModel, args: &Value) -> ToolCallResult {
    let decision_id = arg_str(args, "name");
    let ownership = parse_ownership(args.get("ownership"));
    let contexts = parse_string_array(args.get("contexts"));
    let consequences = parse_string_array(args.get("consequences"));

    match model
        .architectural_decisions
        .iter_mut()
        .find(|d| d.id.eq_ignore_ascii_case(&decision_id))
    {
        Some(decision) => {
            if let Some(title) = args.get("title").and_then(|v| v.as_str()) {
                decision.title = title.to_string();
            }
            if let Some(status) = args.get("status").and_then(|v| v.as_str()) {
                decision.status = parse_decision_status(status);
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
                decision.contexts = contexts;
            }
            if args.get("consequences").is_some() {
                decision.consequences = consequences;
            }
            if args.get("ownership").is_some() {
                decision.ownership = ownership;
            }
            text_result(json!({
                "message": format!("Updated architectural decision '{}'", decision_id)
            }).to_string())
        }
        None => {
            model.architectural_decisions.push(ArchitecturalDecision {
                id: decision_id.clone(),
                title: arg_str(args, "title"),
                status: parse_decision_status(&arg_str(args, "status")),
                scope: arg_str(args, "scope"),
                date: arg_str(args, "date"),
                rationale: arg_str(args, "rationale"),
                consequences,
                contexts,
                ownership,
            });
            text_result(json!({
                "message": format!("Created architectural decision '{}'", decision_id)
            }).to_string())
        }
    }
}

fn remove_architectural_decision(model: &mut DomainModel, args: &Value) -> ToolCallResult {
    let decision_id = arg_str(args, "name");
    let before = model.architectural_decisions.len();
    model.architectural_decisions
        .retain(|d| !d.id.eq_ignore_ascii_case(&decision_id));
    if model.architectural_decisions.len() < before {
        text_result(format!("Removed architectural decision '{decision_id}'"))
    } else {
        error_result(format!("Architectural decision '{decision_id}' not found"))
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
        team: obj.get("team").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        owners: obj
            .get("owners")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default(),
        rationale: obj.get("rationale").and_then(|v| v.as_str()).unwrap_or("").to_string(),
    }
}

fn parse_string_array(val: Option<&Value>) -> Vec<String> {
    val.and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default()
}

fn parse_policy_kind(kind: &str) -> PolicyKind {
    match kind {
        "process_manager" => PolicyKind::ProcessManager,
        "integration" => PolicyKind::Integration,
        _ => PolicyKind::Domain,
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

/// Compute the suggested file path for a domain artifact, using project conventions.
/// This replaces the standalone `suggest_file_path` tool — now implicit in every
/// `set_model` response for artifacts that live in files (entity, service, event).
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
                        required: f
                            .get("required")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false),
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
fn enrich_plan(store: &Store, workspace_path: &str, changes: &[Value]) -> Value {
    // Build context → module_path lookup from desired state
    let module_paths: std::collections::HashMap<String, String> = store
        .run_datalog(
            "?[name, module_path] := *context{workspace: $ws, name, module_path, state: 'desired'}",
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
            "Run `refactor scan` after implementing to update the actual model.",
            "Run `refactor accept` when implementation matches the desired model."
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
        let path = temp_dir().join(format!("dendrites_wt_test_{}_{}.db", std::process::id(), id));
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
                }],
                value_objects: vec![],
                services: vec![],
                repositories: vec![],
                events: vec![],
                dependencies: vec![],
            }],
            external_systems: vec![],
            architectural_decisions: vec![],
            ownership: Ownership::default(),
            rules: vec![],
            tech_stack: TechStack::default(),
            conventions: Conventions::default(),
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
        assert_eq!(list_write_tools().len(), 2);
    }

    #[test]
    fn test_update_entity_add_field() {
        let ws = "/tmp/test-add-field";
        let store = setup(ws);
        let result = call_write_tool(ws, &store, "set_model", &json!({
            "kind": "entity", "context": "Identity", "name": "User",
            "fields": [{"name": "email", "type": "String", "required": true}]
        }));
        assert!(result.is_error.is_none());
        let loaded = store.load_desired(ws).unwrap().unwrap();
        let identity = loaded.bounded_contexts.iter().find(|c| c.name == "Identity").unwrap();
        let user = identity.entities.iter().find(|e| e.name == "User").unwrap();
        assert_eq!(user.fields.len(), 2);
        assert!(user.fields.iter().any(|f| f.name == "email"));
    }

    #[test]
    fn test_update_entity_merge_existing_field() {
        let ws = "/tmp/test-merge-field";
        let store = setup(ws);
        let result = call_write_tool(ws, &store, "set_model", &json!({
            "kind": "entity", "context": "Identity", "name": "User",
            "fields": [{"name": "id", "type": "Uuid"}]
        }));
        assert!(result.is_error.is_none());
        let loaded = store.load_desired(ws).unwrap().unwrap();
        let identity = loaded.bounded_contexts.iter().find(|c| c.name == "Identity").unwrap();
        let user = identity.entities.iter().find(|e| e.name == "User").unwrap();
        assert_eq!(user.fields.len(), 1);
        assert_eq!(user.fields[0].field_type, "Uuid");
    }

    #[test]
    fn test_create_new_entity() {
        let ws = "/tmp/test-new-entity";
        let store = setup(ws);
        let result = call_write_tool(ws, &store, "set_model", &json!({
            "kind": "entity", "context": "Identity", "name": "Role",
            "description": "A role assignment", "aggregate_root": false,
            "fields": [{"name": "name", "type": "String"}]
        }));
        assert!(result.is_error.is_none());
        let loaded = store.load_desired(ws).unwrap().unwrap();
        let identity = loaded.bounded_contexts.iter().find(|c| c.name == "Identity").unwrap();
        assert_eq!(identity.entities.len(), 2);
        assert!(identity.entities.iter().any(|e| e.name == "Role"));
    }

    #[test]
    fn test_update_entity_context_not_found() {
        let ws = "/tmp/test-ctx-notfound";
        let store = setup(ws);
        let result = call_write_tool(ws, &store, "set_model",
            &json!({"kind": "entity", "context": "Nonexistent", "name": "Foo"}),
        );
        assert_eq!(result.is_error, Some(true));
    }

    #[test]
    fn test_create_bounded_context() {
        let ws = "/tmp/test-create-bc";
        let store = setup(ws);
        let result = call_write_tool(ws, &store, "set_model", &json!({
            "kind": "bounded_context", "name": "Billing",
            "description": "Billing context", "module_path": "src/billing",
            "dependencies": ["Identity"]
        }));
        assert!(result.is_error.is_none());
        let loaded = store.load_desired(ws).unwrap().unwrap();
        assert_eq!(loaded.bounded_contexts.len(), 2);
        let billing = loaded.bounded_contexts.iter().find(|c| c.name == "Billing").unwrap();
        assert_eq!(billing.dependencies, vec!["Identity"]);
    }

    #[test]
    fn test_update_existing_bounded_context() {
        let ws = "/tmp/test-update-bc";
        let store = setup(ws);
        let result = call_write_tool(ws, &store, "set_model", &json!({
            "kind": "bounded_context", "name": "Identity",
            "description": "Updated description"
        }));
        assert!(result.is_error.is_none());
        let loaded = store.load_desired(ws).unwrap().unwrap();
        let identity = loaded.bounded_contexts.iter().find(|c| c.name == "Identity").unwrap();
        assert_eq!(identity.description, "Updated description");
    }

    #[test]
    fn test_remove_entity() {
        let ws = "/tmp/test-rm-entity";
        let store = setup(ws);
        let result = call_write_tool(ws, &store, "set_model",
            &json!({"kind": "entity", "action": "remove", "context": "Identity", "name": "User"}),
        );
        assert!(result.is_error.is_none());
        let loaded = store.load_desired(ws).unwrap().unwrap();
        let identity = loaded.bounded_contexts.iter().find(|c| c.name == "Identity").unwrap();
        assert_eq!(identity.entities.len(), 0);
    }

    #[test]
    fn test_remove_entity_not_found() {
        let ws = "/tmp/test-rm-entity-nf";
        let store = setup(ws);
        let result = call_write_tool(ws, &store, "set_model",
            &json!({"kind": "entity", "action": "remove", "context": "Identity", "name": "NotHere"}),
        );
        assert_eq!(result.is_error, Some(true));
    }

    #[test]
    fn test_update_service() {
        let ws = "/tmp/test-upd-svc";
        let store = setup(ws);
        let result = call_write_tool(ws, &store, "set_model", &json!({
            "kind": "service", "context": "Identity", "name": "AuthService",
            "service_kind": "application", "description": "Handles authentication"
        }));
        assert!(result.is_error.is_none());
        let loaded = store.load_desired(ws).unwrap().unwrap();
        let identity = loaded.bounded_contexts.iter().find(|c| c.name == "Identity").unwrap();
        assert_eq!(identity.services.len(), 1);
        assert_eq!(identity.services[0].description, "Handles authentication");
    }

    #[test]
    fn test_update_event() {
        let ws = "/tmp/test-upd-evt";
        let store = setup(ws);
        let result = call_write_tool(ws, &store, "set_model", &json!({
            "kind": "event", "context": "Identity", "name": "UserRegistered",
            "source": "User", "fields": [{"name": "user_id", "type": "UserId"}]
        }));
        assert!(result.is_error.is_none());
        let loaded = store.load_desired(ws).unwrap().unwrap();
        let identity = loaded.bounded_contexts.iter().find(|c| c.name == "Identity").unwrap();
        assert_eq!(identity.events.len(), 1);
        assert_eq!(identity.events[0].name, "UserRegistered");
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
        call_write_tool(ws, &store, "set_model",
            &json!({"kind": "bounded_context", "name": "Billing", "description": "Billing context"}),
        );
        let loaded = store.load_desired(ws).unwrap().unwrap();
        assert_eq!(loaded.bounded_contexts.len(), 2);
    }

    #[test]
    fn test_auto_save_not_on_error() {
        let store = test_store();
        let ws = "/tmp/test-autosave-err";
        let result = call_write_tool(ws, &store, "set_model",
            &json!({"kind": "entity", "context": "Nonexistent", "name": "Foo"}),
        );
        assert_eq!(result.is_error, Some(true));
        assert!(store.load_desired(ws).unwrap().is_none());
    }

    #[test]
    fn test_draft_refactoring_plan_uses_baseline() {
        let ws = "/tmp/test-baseline";
        let store = setup(ws);
        call_write_tool(ws, &store, "set_model",
            &json!({"kind": "bounded_context", "name": "Identity", "description": "Updated"}),
        );
        let result = call_write_tool(ws, &store, "refactor", &json!({}));
        let text = match &result.content[0] { ContentBlock::Text { text } => text };
        assert!(text.contains("pending_changes"));
    }

    #[test]
    fn test_draft_plan_does_not_auto_advance() {
        let ws = "/tmp/test-no-advance";
        let store = setup(ws);
        call_write_tool(ws, &store, "set_model",
            &json!({"kind": "bounded_context", "name": "Identity", "description": "Updated"}),
        );
        let result = call_write_tool(ws, &store, "refactor", &json!({}));
        let text = match &result.content[0] { ContentBlock::Text { text } => text };
        assert!(text.contains("pending_changes"));
        let result = call_write_tool(ws, &store, "refactor", &json!({}));
        let text = match &result.content[0] { ContentBlock::Text { text } => text };
        assert!(text.contains("pending_changes"));
    }

    #[test]
    fn test_accept_then_plan_shows_in_sync() {
        let ws = "/tmp/test-accept-sync";
        let store = setup(ws);
        call_write_tool(ws, &store, "set_model",
            &json!({"kind": "bounded_context", "name": "Identity", "description": "Updated"}),
        );
        let result = call_write_tool(ws, &store, "refactor", &json!({"action": "accept"}));
        let text = match &result.content[0] { ContentBlock::Text { text } => text };
        assert!(text.contains("accepted"));
        let result = call_write_tool(ws, &store, "refactor", &json!({}));
        let text = match &result.content[0] { ContentBlock::Text { text } => text };
        assert!(text.contains("in_sync"));
    }

    #[test]
    fn test_reset_reverts_desired() {
        let ws = "/tmp/test-reset-wt";
        let store = setup(ws);
        call_write_tool(ws, &store, "set_model",
            &json!({"kind": "bounded_context", "name": "Identity", "description": "Original"}),
        );
        call_write_tool(ws, &store, "refactor", &json!({"action": "accept"}));
        call_write_tool(ws, &store, "set_model",
            &json!({"kind": "bounded_context", "name": "Billing", "description": "New context"}),
        );
        let desired = store.load_desired(ws).unwrap().unwrap();
        assert_eq!(desired.bounded_contexts.len(), 2);
        let result = call_write_tool(ws, &store, "refactor", &json!({"action": "reset"}));
        let text = match &result.content[0] { ContentBlock::Text { text } => text };
        assert!(text.contains("reset"));
        let reset = store.load_desired(ws).unwrap().unwrap();
        assert_eq!(reset.bounded_contexts.len(), 1);
    }

    #[test]
    fn test_update_service_merges_methods() {
        let ws = "/tmp/test-merge-methods";
        let store = setup(ws);
        call_write_tool(ws, &store, "set_model", &json!({
            "kind": "service", "context": "Identity", "name": "AuthService",
            "service_kind": "application",
            "methods": [{"name": "login", "return_type": "Token"}]
        }));
        call_write_tool(ws, &store, "set_model", &json!({
            "kind": "service", "context": "Identity", "name": "AuthService",
            "methods": [{"name": "logout", "return_type": "void"}]
        }));
        let loaded = store.load_desired(ws).unwrap().unwrap();
        let identity = loaded.bounded_contexts.iter().find(|c| c.name == "Identity").unwrap();
        let svc = identity.services.iter().find(|s| s.name == "AuthService").unwrap();
        assert_eq!(svc.methods.len(), 2);
    }

    #[test]
    fn test_remove_bounded_context() {
        let ws = "/tmp/test-rm-bc";
        let store = setup(ws);
        let result = call_write_tool(ws, &store, "set_model",
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
        let result = call_write_tool("/tmp/test-ws", &store, "set_model",
            &json!({"name": "Foo"}),
        );
        assert_eq!(result.is_error, Some(true));
    }
}