use serde_json::{Value, json};

use crate::mcp::protocol::*;
use crate::store::Store;

/// Returns the list of tools the Dendrites server exposes.
pub fn list_tools() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "architecture".into(),
            description: "Show the complete architecture of this codebase: modules, components, \
                          services, events, and their relationships. Includes a health score \
                          (0-100) with specific issues like circular dependencies, layer violations, \
                          and complexity hotspots. \
                          Call this first to understand any codebase before making changes."
                .into(),
            input_schema: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
        ToolDefinition {
            name: "impact".into(),
            description: "Analyze the downstream impact of changing or deleting a component.\n\
                          Supports: transitive_deps, circular_deps, layer_violations, impact_analysis, \n\
                          aggregate_quality, dependency_graph, field_usage, method_search, shared_fields, \n\
                          pagerank, community_detection, betweenness_centrality, degree_centrality, \n\
                          topological_order, call_graph_callers, call_graph_callees, \n\
                          call_graph_reachability, call_graph_stats."
                .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "analysis": {
                        "type": "string",
                        "enum": ["transitive_deps", "circular_deps", "layer_violations", "impact_analysis",
                                 "aggregate_quality", "dependency_graph", "field_usage", "method_search",
                                 "shared_fields", "pagerank", "community_detection", "betweenness_centrality",
                                 "degree_centrality", "topological_order",
                                 "call_graph_callers", "call_graph_callees", "call_graph_reachability", "call_graph_stats"],
                        "description": "The specific analysis to run"
                    },
                    "context": { "type": "string", "description": "Module or component name (required for transitive_deps, impact_analysis)" },
                    "entity": { "type": "string", "description": "Entity name (required for impact_analysis)" },
                    "symbol": { "type": "string", "description": "Symbol name (required for call_graph_callers, call_graph_callees, call_graph_reachability)" },
                    "field_type": { "type": "string", "description": "Field type to search (required for field_usage)" },
                    "method_name": { "type": "string", "description": "Method name to search (required for method_search)" }
                },
                "required": ["analysis"]
            }),
        },
        ToolDefinition {
            name: "safe_to_delete".into(),
            description: "Check whether a function, struct, or component can be safely deleted. \
                          Evaluates all inbound references — callers, dependents, event consumers — \
                          and returns a clear yes/no with evidence."
                .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "context": { "type": "string", "description": "Module or component name" },
                    "entity": { "type": "string", "description": "Entity or symbol name to check" }
                },
                "required": ["context", "entity"]
            }),
        },
        ToolDefinition {
            name: "check".into(),
            description: "Check for architectural problems: circular dependencies, layer violations, \
                          missing business rules on core entities, isolated modules, or policy violations. \
                          Run without parameters to check everything at once."
                .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "check_name": {
                        "type": "string",
                        "enum": ["layer_violations", "circular_deps", "aggregate_quality", "orphan_contexts", "policy_violations", "drift"],
                        "description": "Specific check to run (default: runs all checks)"
                    }
                },
                "required": []
            }),
        },
        ToolDefinition {
            name: "how_connected".into(),
            description: "Show how two modules or components are connected. Returns the dependency \
                          path(s) between them, proving whether and how they relate."
                .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "from": { "type": "string", "description": "Source module or component" },
                    "to": { "type": "string", "description": "Target module or component" }
                },
                "required": ["from", "to"]
            }),
        },
        ToolDefinition {
            name: "why".into(),
            description: "Explain why something is flagged as a problem. Returns evidence-backed \
                          explanations with specific references and remediation suggestions."
                .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "violation_type": {
                        "type": "string",
                        "enum": ["layer_violations", "circular_deps", "policy_violations", "aggregate_quality", "orphan_contexts"],
                        "description": "The type of problem to explain"
                    }
                },
                "required": ["violation_type"]
            }),
        },
        ToolDefinition {
            name: "drift".into(),
            description: "Compare planned architecture vs current implementation. Shows what's been \
                          added, removed, or changed — and what still needs to be implemented."
                .into(),
            input_schema: json!({
                "type": "object",
                "properties": {},
            }),
        },
        ToolDefinition {
            name: "history".into(),
            description: "View architecture change history. Without timestamps, lists available \
                          snapshots. With timestamps, compares two points in time to show \
                          what changed between them."
                .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "state": {
                        "type": "string",
                        "enum": ["planned", "current"],
                        "description": "Which model state to query (default: planned)"
                    },
                    "ts_old": {
                        "type": "integer",
                        "description": "Older snapshot timestamp (microseconds). Required for comparison."
                    },
                    "ts_new": {
                        "type": "integer",
                        "description": "Newer snapshot timestamp (microseconds). Omit for latest."
                    }
                },
                "required": []
            }),
        },
        ToolDefinition {
            name: "search".into(),
            description: "Search the architecture by keyword. Finds matching modules, components, \
                          services, events, and decisions across the entire codebase."
                .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search keywords" },
                    "limit": { "type": "integer", "description": "Max results (default: 20)", "default": 20 }
                },
                "required": ["query"]
            }),
        },
    ]
}

/// Dispatches a tool call and returns the result.
pub fn call_tool(store: &Store, workspace_path: &str, name: &str, args: &Value) -> ToolCallResult {
    match name {
        "architecture" => {
            let canonical = crate::store::cozo::canonicalize_path(workspace_path);

            let planned = build_model_overview(store, &canonical, "desired");
            let current = build_model_overview(store, &canonical, "actual");

            let has_current = current
                .get("bounded_contexts")
                .and_then(|v| v.as_array())
                .is_some_and(|a| !a.is_empty());

            let (status, pending_count) = if has_current {
                let changes = store
                    .diff_graph(workspace_path)
                    .ok()
                    .and_then(|v| v.get("pending_changes").cloned())
                    .and_then(|v| v.as_array().cloned())
                    .unwrap_or_default();
                if changes.is_empty() {
                    ("in_sync", 0)
                } else {
                    ("pending_changes", changes.len())
                }
            } else {
                ("no_current", 0)
            };

            let mut overview = json!({
                "planned": planned,
                "current": if has_current { current } else { json!(null) },
                "status": status,
                "pending_change_count": pending_count,
            });

            // Merge health data into architecture response
            if let Ok(health) = store.model_health(workspace_path) {
                overview["health"] = serde_json::to_value(&health).unwrap_or(json!(null));
            }

            text_result(serde_json::to_string(&overview).unwrap())
        }

        "impact" => {
            let analysis = args["analysis"].as_str().unwrap_or("");
            let canonical = crate::store::cozo::canonicalize_path(workspace_path);

            match analysis {
                "transitive_deps" => {
                    let context = match args["context"].as_str() {
                        Some(c) => c,
                        None => {
                            return error_result(
                                "'context' parameter is required for transitive_deps".into(),
                            );
                        }
                    };
                    match store.transitive_deps(&canonical, context) {
                        Ok(deps) => text_result(
                            json!({
                                "analysis": "transitive_deps",
                                "context": context,
                                "dependencies": deps,
                                "count": deps.len(),
                            })
                            .to_string(),
                        ),
                        Err(e) => error_result(format!("Transitive deps query failed: {}", e)),
                    }
                }
                "circular_deps" => match store.circular_deps(&canonical) {
                    Ok(cycles) => {
                        let cycle_pairs: Vec<_> = cycles
                            .iter()
                            .map(|(a, b)| json!({"from": a, "to": b}))
                            .collect();
                        text_result(
                            json!({
                                "analysis": "circular_deps",
                                "cycles": cycle_pairs,
                                "has_cycles": !cycles.is_empty(),
                            })
                            .to_string(),
                        )
                    }
                    Err(e) => error_result(format!("Circular deps query failed: {}", e)),
                },
                "layer_violations" => match store.layer_violations(&canonical) {
                    Ok(violations) => {
                        let items: Vec<_> = violations
                            .iter()
                            .map(|(ctx, svc, dep)| {
                                json!({
                                    "context": ctx,
                                    "domain_service": svc,
                                    "infrastructure_dependency": dep,
                                })
                            })
                            .collect();
                        text_result(
                            json!({
                                "analysis": "layer_violations",
                                "violations": items,
                                "count": violations.len(),
                            })
                            .to_string(),
                        )
                    }
                    Err(e) => error_result(format!("Layer violations query failed: {}", e)),
                },
                "impact_analysis" => {
                    let context = match args["context"].as_str() {
                        Some(c) => c,
                        None => {
                            return error_result(
                                "'context' parameter is required for impact_analysis".into(),
                            );
                        }
                    };
                    let entity = match args["entity"].as_str() {
                        Some(e) => e,
                        None => {
                            return error_result(
                                "'entity' parameter is required for impact_analysis".into(),
                            );
                        }
                    };
                    match store.impact_analysis(&canonical, context, entity) {
                        Ok(result) => text_result(
                            json!({
                                "analysis": "impact_analysis",
                                "result": result,
                            })
                            .to_string(),
                        ),
                        Err(e) => error_result(format!("Impact analysis query failed: {}", e)),
                    }
                }
                "aggregate_quality" => match store.aggregate_roots_without_invariants(&canonical) {
                    Ok(roots) => {
                        let items: Vec<_> = roots
                            .iter()
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
                },
                "dependency_graph" => match store.dependency_graph(&canonical) {
                    Ok(graph) => text_result(
                        json!({
                            "analysis": "dependency_graph",
                            "graph": graph,
                        })
                        .to_string(),
                    ),
                    Err(e) => error_result(format!("Dependency graph query failed: {}", e)),
                },
                "field_usage" => {
                    let field_type = match args["field_type"].as_str() {
                        Some(t) => t,
                        None => {
                            return error_result(
                                "'field_type' parameter is required for field_usage".into(),
                            );
                        }
                    };
                    match store.run_datalog(
                        &format!(
                            "?[ctx, owner_kind, owner, field_name] := \
                                *field{{workspace: $ws, context: ctx, owner_kind, owner, \
                                       name: field_name, field_type: '{}', state: 'desired' @ 'NOW'}}",
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
                        None => {
                            return error_result(
                                "'method_name' parameter is required for method_search".into(),
                            );
                        }
                    };
                    match store.run_datalog(
                        &format!(
                            "?[ctx, owner_kind, owner, return_type] := \
                                *method{{workspace: $ws, context: ctx, owner_kind, owner, \
                                        name: '{}', state: 'desired' @ 'NOW', return_type}}",
                            method_name.replace('\'', "''")
                        ),
                        &canonical,
                    ) {
                        Ok(rows) => {
                            let items: Vec<_> = rows
                                .iter()
                                .map(|r| {
                                    json!({
                                        "context": r[0], "owner_kind": r[1],
                                        "owner": r[2], "return_type": r[3],
                                    })
                                })
                                .collect();
                            text_result(
                                json!({
                                    "analysis": "method_search",
                                    "method_name": method_name,
                                    "matches": items,
                                    "count": items.len(),
                                })
                                .to_string(),
                            )
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
                                   owner, name, field_type: ft, state: 'desired' @ 'NOW'} \
                         event_field[ctx, owner, name, ft] := \
                            *field{workspace: $ws, context: ctx, owner_kind: 'event', \
                                   owner, name, field_type: ft, state: 'desired' @ 'NOW'} \
                         ?[ctx, entity, event, field_name, field_type] := \
                            entity_field[ctx, entity, field_name, field_type], \
                            event_field[ctx, event, field_name, field_type]",
                        &canonical,
                    ) {
                        Ok(rows) => {
                            let items: Vec<_> = rows
                                .iter()
                                .map(|r| {
                                    json!({
                                        "context": r[0], "entity": r[1],
                                        "event": r[2], "field": r[3], "type": r[4],
                                    })
                                })
                                .collect();
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
                "pagerank" => match store.pagerank(&canonical) {
                    Ok(ranked) => {
                        let items: Vec<_> = ranked
                            .iter()
                            .map(|(name, rank)| json!({"context": name, "rank": rank}))
                            .collect();
                        text_result(json!({
                                "analysis": "pagerank",
                                "ranking": items,
                                "count": ranked.len(),
                                "insight": "Higher PageRank indicates more architecturally important contexts (more dependencies flow through them).",
                            }).to_string())
                    }
                    Err(e) => error_result(format!("PageRank query failed: {e}")),
                },
                "community_detection" => match store.community_detection(&canonical) {
                    Ok(communities) => {
                        let items: Vec<_> = communities
                            .iter()
                            .map(|(name, cid)| json!({"context": name, "community": cid}))
                            .collect();
                        text_result(json!({
                                "analysis": "community_detection",
                                "communities": items,
                                "count": communities.len(),
                                "insight": "Contexts in the same community are tightly coupled. Consider aligning bounded context boundaries with community boundaries.",
                            }).to_string())
                    }
                    Err(e) => error_result(format!("Community detection query failed: {e}")),
                },
                "betweenness_centrality" => match store.betweenness_centrality(&canonical) {
                    Ok(ranked) => {
                        let items: Vec<_> = ranked.iter()
                                .map(|(name, centrality)| json!({"context": name, "centrality": centrality}))
                                .collect();
                        text_result(json!({
                                "analysis": "betweenness_centrality",
                                "ranking": items,
                                "count": ranked.len(),
                                "insight": "High betweenness centrality indicates bottleneck contexts. Changes here have outsized downstream impact.",
                            }).to_string())
                    }
                    Err(e) => error_result(format!("Betweenness centrality query failed: {e}")),
                },
                "degree_centrality" => match store.degree_centrality(&canonical) {
                    Ok(degrees) => {
                        let items: Vec<_> = degrees.iter()
                                .map(|(name, in_d, out_d)| json!({"context": name, "in_degree": in_d, "out_degree": out_d}))
                                .collect();
                        text_result(
                            json!({
                                "analysis": "degree_centrality",
                                "degrees": items,
                                "count": degrees.len(),
                            })
                            .to_string(),
                        )
                    }
                    Err(e) => error_result(format!("Degree centrality query failed: {e}")),
                },
                "topological_order" => match store.topological_order(&canonical) {
                    Ok(result) => text_result(
                        json!({
                            "analysis": "topological_order",
                            "result": result,
                        })
                        .to_string(),
                    ),
                    Err(e) => error_result(format!("Topological order query failed: {e}")),
                },
                "call_graph_callers" => {
                    let symbol = match args["symbol"].as_str() {
                        Some(s) => s,
                        None => {
                            return error_result(
                                "'symbol' parameter is required for call_graph_callers".into(),
                            );
                        }
                    };
                    match store.call_graph_callers(&canonical, symbol) {
                        Ok(result) => text_result(
                            json!({
                                "analysis": "call_graph_callers",
                                "result": result,
                            })
                            .to_string(),
                        ),
                        Err(e) => error_result(format!("Call graph callers query failed: {e}")),
                    }
                }
                "call_graph_callees" => {
                    let symbol = match args["symbol"].as_str() {
                        Some(s) => s,
                        None => {
                            return error_result(
                                "'symbol' parameter is required for call_graph_callees".into(),
                            );
                        }
                    };
                    match store.call_graph_callees(&canonical, symbol) {
                        Ok(result) => text_result(
                            json!({
                                "analysis": "call_graph_callees",
                                "result": result,
                            })
                            .to_string(),
                        ),
                        Err(e) => error_result(format!("Call graph callees query failed: {e}")),
                    }
                }
                "call_graph_reachability" => {
                    let symbol = match args["symbol"].as_str() {
                        Some(s) => s,
                        None => {
                            return error_result(
                                "'symbol' parameter is required for call_graph_reachability".into(),
                            );
                        }
                    };
                    match store.call_graph_reachability(&canonical, symbol) {
                        Ok(result) => text_result(
                            json!({
                                "analysis": "call_graph_reachability",
                                "result": result,
                            })
                            .to_string(),
                        ),
                        Err(e) => {
                            error_result(format!("Call graph reachability query failed: {e}"))
                        }
                    }
                }
                "call_graph_stats" => match store.call_graph_stats(&canonical) {
                    Ok(result) => text_result(
                        json!({
                            "analysis": "call_graph_stats",
                            "result": result,
                        })
                        .to_string(),
                    ),
                    Err(e) => error_result(format!("Call graph stats query failed: {e}")),
                },
                _ => error_result(format!(
                    "Unknown analysis type: '{}'. Valid types: transitive_deps, circular_deps, layer_violations, impact_analysis, aggregate_quality, dependency_graph, field_usage, method_search, shared_fields, pagerank, community_detection, betweenness_centrality, degree_centrality, topological_order, call_graph_callers, call_graph_callees, call_graph_reachability, call_graph_stats",
                    analysis
                )),
            }
        }

        "safe_to_delete" => {
            let canonical = crate::store::cozo::canonicalize_path(workspace_path);
            let context = match args["context"].as_str() {
                Some(c) => c,
                None => return error_result("'context' parameter is required".into()),
            };
            let entity = match args["entity"].as_str() {
                Some(e) => e,
                None => return error_result("'entity' parameter is required".into()),
            };
            match store.can_delete_symbol(&canonical, context, entity) {
                Ok(result) => text_result(json!({
                    "status": if result["can_delete"].as_bool().unwrap_or(false) { "true" } else { "false" },
                    "entity": entity,
                    "context": context,
                    "result": result,
                    "proof": {
                        "rule": "entity deletable IFF no inbound references in scope",
                        "derived_from": ["service_dep", "context_dep", "event", "repository"],
                    },
                    "provenance": { "source": "datalog", "state": "desired" },
                }).to_string()),
                Err(e) => error_result(format!("can_delete_symbol failed: {e}")),
            }
        }

        "check" => {
            let canonical = crate::store::cozo::canonicalize_path(workspace_path);
            let invariant = args["check_name"].as_str()
                .or_else(|| args["invariant"].as_str())
                .unwrap_or("all");
            match invariant {
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
                                "invariant": "layer_violations",
                                "status": if violations.is_empty() { "true" } else { "false" },
                                "violations": items,
                                "count": violations.len(),
                                "proof": {
                                    "rule": "domain_service MUST_NOT depend_on infrastructure_service",
                                    "derived_from": ["service", "service_dep"],
                                    "witness_count": violations.len(),
                                },
                                "provenance": { "source": "datalog", "state": "desired" },
                            }).to_string())
                        }
                        Err(e) => error_result(format!("layer_violations check failed: {e}")),
                    }
                }
                "circular_deps" => {
                    match store.circular_deps(&canonical) {
                        Ok(cycles) => {
                            let pairs: Vec<_> = cycles.iter()
                                .map(|(a, b)| json!({"from": a, "to": b}))
                                .collect();
                            text_result(json!({
                                "invariant": "circular_deps",
                                "status": if cycles.is_empty() { "true" } else { "false" },
                                "cycles": pairs,
                                "count": cycles.len(),
                                "proof": {
                                    "rule": "context_dep graph MUST be acyclic",
                                    "derived_from": ["context_dep"],
                                    "witness_count": cycles.len(),
                                },
                                "provenance": { "source": "datalog", "state": "desired" },
                            }).to_string())
                        }
                        Err(e) => error_result(format!("circular_deps check failed: {e}")),
                    }
                }
                "aggregate_quality" => {
                    match store.aggregate_roots_without_invariants(&canonical) {
                        Ok(roots) => {
                            let items: Vec<_> = roots.iter()
                                .map(|(ctx, ent)| json!({"context": ctx, "entity": ent}))
                                .collect();
                            text_result(json!({
                                "invariant": "aggregate_quality",
                                "status": if roots.is_empty() { "true" } else { "false" },
                                "roots_without_invariants": items,
                                "count": roots.len(),
                                "proof": {
                                    "rule": "aggregate_root MUST have at_least_one invariant",
                                    "derived_from": ["entity", "invariant"],
                                    "witness_count": roots.len(),
                                },
                                "provenance": { "source": "datalog", "state": "desired" },
                            }).to_string())
                        }
                        Err(e) => error_result(format!("aggregate_quality check failed: {e}")),
                    }
                }
                "orphan_contexts" => {
                    // orphan_contexts is private, use model_health and extract
                    match store.model_health(&canonical) {
                        Ok(health) => {
                            text_result(json!({
                                "invariant": "orphan_contexts",
                                "status": if health.orphan_contexts.is_empty() { "true" } else { "false" },
                                "orphans": health.orphan_contexts,
                                "count": health.orphan_contexts.len(),
                                "proof": {
                                    "rule": "context SHOULD participate_in dependency_graph",
                                    "derived_from": ["context", "context_dep"],
                                    "witness_count": health.orphan_contexts.len(),
                                },
                                "provenance": { "source": "datalog", "state": "desired" },
                            }).to_string())
                        }
                        Err(e) => error_result(format!("orphan_contexts check failed: {e}")),
                    }
                }
                "policy_violations" => {
                    match store.evaluate_policy_violations(&canonical) {
                        Ok(result) => text_result(json!({
                            "invariant": "policy_violations",
                            "status": result["status"],
                            "violations": result["violations"],
                            "count": result["count"],
                            "proof": {
                                "rule": "dependency MUST_NOT violate declared constraint",
                                "derived_from": ["context_dep", "layer_assignment", "dependency_constraint"],
                                "witness_count": result["count"],
                            },
                            "provenance": { "source": "datalog", "state": "desired" },
                        }).to_string()),
                        Err(e) => error_result(format!("policy_violations check failed: {e}")),
                    }
                }
                "drift" => {
                    match store.diff_graph(&canonical) {
                        Ok(diff) => {
                            let changes = diff["pending_changes"]
                                .as_array()
                                .cloned()
                                .unwrap_or_default();
                            text_result(json!({
                                "check_name": "drift",
                                "status": if changes.is_empty() { "true" } else { "false" },
                                "pending_changes": changes.len(),
                            }).to_string())
                        }
                        Err(e) => error_result(format!("drift check failed: {e}")),
                    }
                }
                "all" | "" => {
                    // Run all checks and return combined result
                    let mut results = json!({});
                    let mut all_pass = true;

                    if let Ok(violations) = store.layer_violations(&canonical) {
                        if !violations.is_empty() { all_pass = false; }
                        results["layer_violations"] = json!({
                            "status": violations.is_empty(),
                            "count": violations.len(),
                        });
                    }
                    if let Ok(cycles) = store.circular_deps(&canonical) {
                        if !cycles.is_empty() { all_pass = false; }
                        results["circular_deps"] = json!({
                            "status": cycles.is_empty(),
                            "count": cycles.len(),
                        });
                    }
                    if let Ok(roots) = store.aggregate_roots_without_invariants(&canonical) {
                        if !roots.is_empty() { all_pass = false; }
                        results["aggregate_quality"] = json!({
                            "status": roots.is_empty(),
                            "count": roots.len(),
                        });
                    }
                    if let Ok(health) = store.model_health(&canonical) {
                        if !health.orphan_contexts.is_empty() { all_pass = false; }
                        results["orphan_contexts"] = json!({
                            "status": health.orphan_contexts.is_empty(),
                            "count": health.orphan_contexts.len(),
                        });
                    }
                    if let Ok(result) = store.evaluate_policy_violations(&canonical) {
                        let count = result["count"].as_u64().unwrap_or(0);
                        if count > 0 { all_pass = false; }
                        results["policy_violations"] = json!({
                            "status": count == 0,
                            "count": count,
                        });
                    }
                    if let Ok(diff) = store.diff_graph(&canonical) {
                        let changes = diff["pending_changes"]
                            .as_array()
                            .map(|a| a.len())
                            .unwrap_or(0);
                        if changes > 0 { all_pass = false; }
                        results["drift"] = json!({
                            "status": changes == 0,
                            "count": changes,
                        });
                    }

                    text_result(json!({
                        "check_name": "all",
                        "status": if all_pass { "pass" } else { "issues_found" },
                        "checks": results,
                    }).to_string())
                }
                _ => error_result(format!("Unknown check: '{}'. Valid: layer_violations, circular_deps, aggregate_quality, orphan_contexts, policy_violations, drift, all", invariant)),
            }
        }

        "how_connected" => {
            let canonical = crate::store::cozo::canonicalize_path(workspace_path);
            let from = args["from"].as_str()
                .or_else(|| args["from_context"].as_str());
            let from = match from {
                Some(f) => f,
                None => return error_result("'from' parameter is required".into()),
            };
            let to = args["to"].as_str()
                .or_else(|| args["to_context"].as_str());
            let to = match to {
                Some(t) => t,
                None => return error_result("'to' parameter is required".into()),
            };
            match store.query_dependency_path(&canonical, from, to) {
                Ok(paths) => text_result(
                    json!({
                        "from": from,
                        "to": to,
                        "paths": paths,
                        "reachable": !paths.is_empty(),
                        "hop_count": paths.len(),
                        "proof": {
                            "rule": "transitive reachability via context_dep",
                            "derived_from": ["context_dep"],
                            "witness_paths": paths,
                        },
                        "provenance": { "source": "datalog", "state": "desired" },
                    })
                    .to_string(),
                ),
                Err(e) => error_result(format!("query_dependency_path failed: {e}")),
            }
        }

        "why" => {
            let canonical = crate::store::cozo::canonicalize_path(workspace_path);
            let violation_type = match args["violation_type"].as_str() {
                Some(v) => v,
                None => return error_result("'violation_type' parameter is required".into()),
            };

            match violation_type {
                "layer_violations" => {
                    match store.layer_violations(&canonical) {
                        Ok(violations) if violations.is_empty() => {
                            text_result(json!({
                                "violation_type": "layer_violations",
                                "status": "true",
                                "explanation": "No layer violations detected. All domain services depend only on domain-level abstractions.",
                                "evidence": [],
                            }).to_string())
                        }
                        Ok(violations) => {
                            let evidence: Vec<_> = violations.iter().map(|(ctx, svc, dep)| {
                                json!({
                                    "context": ctx,
                                    "domain_service": svc,
                                    "infrastructure_dependency": dep,
                                    "explanation": format!(
                                        "Service '{svc}' in context '{ctx}' depends on '{dep}', \
                                         which is an infrastructure-layer dependency. Domain services \
                                         must not depend on infrastructure directly."
                                    ),
                                    "rule": "domain_service MUST NOT depend_on infrastructure_dependency",
                                })
                            }).collect();
                            text_result(json!({
                                "violation_type": "layer_violations",
                                "status": "false",
                                "explanation": format!(
                                    "{} layer violation(s) found. Domain services reference infrastructure dependencies directly, \
                                     violating the dependency inversion principle.",
                                    violations.len()
                                ),
                                "evidence": evidence,
                                "remediation": "Introduce abstractions (traits/interfaces) in the domain layer and inject infrastructure implementations.",
                            }).to_string())
                        }
                        Err(e) => error_result(format!("layer_violations check failed: {e}")),
                    }
                }
                "circular_deps" => {
                    match store.circular_deps(&canonical) {
                        Ok(cycles) if cycles.is_empty() => {
                            text_result(json!({
                                "violation_type": "circular_deps",
                                "status": "true",
                                "explanation": "No circular dependencies detected. Context dependency graph is acyclic.",
                                "evidence": [],
                            }).to_string())
                        }
                        Ok(cycles) => {
                            let evidence: Vec<_> = cycles.iter().map(|(a, b)| {
                                json!({
                                    "from": a,
                                    "to": b,
                                    "explanation": format!(
                                        "Context '{a}' depends on '{b}' and '{b}' depends on '{a}', \
                                         forming a circular dependency cycle."
                                    ),
                                    "rule": "context dependency graph MUST be acyclic",
                                })
                            }).collect();
                            text_result(json!({
                                "violation_type": "circular_deps",
                                "status": "false",
                                "explanation": format!(
                                    "{} circular dependency pair(s) found. Cycles prevent clean module boundaries.",
                                    cycles.len()
                                ),
                                "evidence": evidence,
                                "remediation": "Break cycles by extracting shared concepts into a new context or using events for decoupling.",
                            }).to_string())
                        }
                        Err(e) => error_result(format!("circular_deps check failed: {e}")),
                    }
                }
                "policy_violations" => {
                    match store.evaluate_policy_violations(&canonical) {
                        Ok(result) => {
                            let violations = result["violations"].as_array().cloned().unwrap_or_default();
                            let evidence: Vec<_> = violations.iter().map(|v| {
                                let kind = v["kind"].as_str().unwrap_or("?");
                                json!({
                                    "kind": kind,
                                    "from_context": v["from_context"],
                                    "to_context": v["to_context"],
                                    "from_layer": v["from_layer"],
                                    "to_layer": v["to_layer"],
                                    "rule": v["rule"],
                                    "explanation": if kind == "layer" {
                                        format!(
                                            "Context '{}' (layer: {}) depends on '{}' (layer: {}), \
                                             violating forbidden layer dependency.",
                                            v["from_context"].as_str().unwrap_or("?"),
                                            v["from_layer"].as_str().unwrap_or("?"),
                                            v["to_context"].as_str().unwrap_or("?"),
                                            v["to_layer"].as_str().unwrap_or("?"),
                                        )
                                    } else {
                                        format!(
                                            "Context '{}' depends on '{}', violating forbidden context dependency.",
                                            v["from_context"].as_str().unwrap_or("?"),
                                            v["to_context"].as_str().unwrap_or("?"),
                                        )
                                    },
                                })
                            }).collect();
                            text_result(json!({
                                "violation_type": "policy_violations",
                                "status": result["status"],
                                "explanation": if violations.is_empty() {
                                    "No policy violations detected. All dependencies conform to declared constraints.".to_string()
                                } else {
                                    format!("{} policy violation(s) found. Dependencies violate declared architectural constraints.", violations.len())
                                },
                                "evidence": evidence,
                                "remediation": if violations.is_empty() { Value::Null } else {
                                    json!("Review forbidden dependencies and refactor to respect layer boundaries.")
                                },
                            }).to_string())
                        }
                        Err(e) => error_result(format!("policy_violations check failed: {e}")),
                    }
                }
                "aggregate_quality" => {
                    match store.aggregate_roots_without_invariants(&canonical) {
                        Ok(roots) if roots.is_empty() => {
                            text_result(json!({
                                "violation_type": "aggregate_quality",
                                "status": "true",
                                "explanation": "All aggregate root entities have at least one invariant defined.",
                                "evidence": [],
                            }).to_string())
                        }
                        Ok(roots) => {
                            let evidence: Vec<_> = roots.iter().map(|(ctx, ent)| {
                                json!({
                                    "context": ctx,
                                    "entity": ent,
                                    "explanation": format!(
                                        "Entity '{ent}' in context '{ctx}' is marked as aggregate root \
                                         but has no invariants defined. Aggregate roots should enforce \
                                         domain invariants."
                                    ),
                                    "rule": "aggregate_root MUST have at_least_one invariant",
                                })
                            }).collect();
                            text_result(json!({
                                "violation_type": "aggregate_quality",
                                "status": "false",
                                "explanation": format!(
                                    "{} aggregate root(s) without invariants. Domain integrity may be at risk.",
                                    roots.len()
                                ),
                                "evidence": evidence,
                                "remediation": "Add invariants to aggregate roots to express domain rules explicitly.",
                            }).to_string())
                        }
                        Err(e) => error_result(format!("aggregate_quality check failed: {e}")),
                    }
                }
                "orphan_contexts" => {
                    match store.model_health(&canonical) {
                        Ok(health) if health.orphan_contexts.is_empty() => {
                            text_result(json!({
                                "violation_type": "orphan_contexts",
                                "status": "true",
                                "explanation": "No orphan contexts. All contexts participate in the dependency graph.",
                                "evidence": [],
                            }).to_string())
                        }
                        Ok(health) => {
                            let evidence: Vec<_> = health.orphan_contexts.iter().map(|ctx| {
                                json!({
                                    "context": ctx,
                                    "explanation": format!(
                                        "Context '{ctx}' has no dependencies to or from other contexts. \
                                         It may be unused or missing declared relationships."
                                    ),
                                    "rule": "context SHOULD participate_in dependency_graph",
                                })
                            }).collect();
                            text_result(json!({
                                "violation_type": "orphan_contexts",
                                "status": "false",
                                "explanation": format!(
                                    "{} orphan context(s) found. These contexts are isolated from the dependency graph.",
                                    health.orphan_contexts.len()
                                ),
                                "evidence": evidence,
                                "remediation": "Add dependencies or remove unused contexts.",
                            }).to_string())
                        }
                        Err(e) => error_result(format!("orphan_contexts check failed: {e}")),
                    }
                }
                _ => error_result(format!(
                    "Unknown violation_type: '{}'. Valid: layer_violations, circular_deps, policy_violations, aggregate_quality, orphan_contexts",
                    violation_type
                )),
            }
        }

        "drift" => {
            let canonical = crate::store::cozo::canonicalize_path(workspace_path);
            match store.diff_graph(&canonical) {
                Ok(diff) => {
                    let changes = diff["pending_changes"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default();

                    let added: Vec<_> = changes
                        .iter()
                        .filter(|c| c["action"].as_str() == Some("add"))
                        .cloned()
                        .collect();
                    let removed: Vec<_> = changes
                        .iter()
                        .filter(|c| c["action"].as_str() == Some("remove"))
                        .cloned()
                        .collect();

                    // Include persisted drift data
                    let drift_entries: Vec<Value> = store
                        .load_drift(workspace_path)
                        .unwrap_or_default()
                        .into_iter()
                        .map(|(cat, ctx, name, ct)| {
                            let mut e = json!({"category": cat, "name": name, "change_type": ct});
                            if !ctx.is_empty() {
                                e["context"] = json!(ctx);
                            }
                            e
                        })
                        .collect();

                    text_result(json!({
                        "status": if changes.is_empty() { "in_sync" } else { "pending_changes" },
                        "summary": {
                            "total_changes": changes.len(),
                            "additions": added.len(),
                            "removals": removed.len(),
                            "drift_entries": drift_entries.len(),
                        },
                        "added": added,
                        "removed": removed,
                        "drift": drift_entries,
                    }).to_string())
                }
                Err(e) => error_result(format!("diff_models failed: {e}")),
            }
        }

        "history" => {
            // Map user-facing state names to internal values
            let raw_state = args["state"].as_str().unwrap_or("planned");
            let state = match raw_state {
                "planned" => "desired",
                "current" => "actual",
                other => other,
            };

            let has_timestamps = args["ts_old"].is_number() || args["ts_new"].is_number();

            if has_timestamps {
                // Compare two snapshots
                let ts_old = args["ts_old"].as_i64().unwrap_or(0);
                let ts_new = args["ts_new"].as_i64().unwrap_or_else(|| {
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_micros() as i64)
                        .unwrap_or(0)
                });
                match store.diff_snapshots(workspace_path, state, ts_old, ts_new) {
                    Ok(result) => text_result(result.to_string()),
                    Err(e) => error_result(format!("history comparison failed: {e}")),
                }
            } else {
                // List available snapshots
                match store.list_snapshots(workspace_path, state) {
                    Ok(timestamps) => text_result(
                        json!({
                            "state": raw_state,
                            "snapshots": timestamps,
                            "count": timestamps.len(),
                        })
                        .to_string(),
                    ),
                    Err(e) => error_result(format!("history listing failed: {e}")),
                }
            }
        }

        "search" => {
            let query = match args["query"].as_str() {
                Some(q) => q,
                None => return error_result("'query' parameter is required".into()),
            };
            let limit = args["limit"].as_u64().unwrap_or(20) as usize;
            match store.search_text(workspace_path, query, limit) {
                Ok(results) => text_result(results.to_string()),
                Err(e) => error_result(format!("search_architecture failed: {e}")),
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
    let contexts = store
        .run_datalog(
            &format!(
                "?[name, description, module_path] := \
            *context{{workspace: $ws, name, description, module_path, state: '{state}' @ 'NOW'}}"
            ),
            workspace,
        )
        .unwrap_or_default();

    let context_deps = store
        .run_datalog(
            &format!(
                "?[from_ctx, to_ctx] := \
            *context_dep{{workspace: $ws, from_ctx, to_ctx, state: '{state}' @ 'NOW'}}"
            ),
            workspace,
        )
        .unwrap_or_default();

    let entities = store.run_datalog(
        &format!("?[ctx, name, description, aggregate_root] := \
            *entity{{workspace: $ws, context: ctx, name, description, aggregate_root, state: '{state}' @ 'NOW'}}"),
        workspace,
    ).unwrap_or_default();

    let services = store.run_datalog(
        &format!("?[ctx, name, description, kind] := \
            *service{{workspace: $ws, context: ctx, name, description, kind, state: '{state}' @ 'NOW'}}"),
        workspace,
    ).unwrap_or_default();

    let events = store.run_datalog(
        &format!("?[ctx, name, description, source] := \
            *event{{workspace: $ws, context: ctx, name, description, source, state: '{state}' @ 'NOW'}}"),
        workspace,
    ).unwrap_or_default();

    let value_objects = store.run_datalog(
        &format!("?[ctx, name, description] := \
            *value_object{{workspace: $ws, context: ctx, name, description, state: '{state}' @ 'NOW'}}"),
        workspace,
    ).unwrap_or_default();

    let repositories = store
        .run_datalog(
            &format!(
                "?[ctx, name, aggregate] := \
            *repository{{workspace: $ws, context: ctx, name, aggregate, state: '{state}' @ 'NOW'}}"
            ),
            workspace,
        )
        .unwrap_or_default();

    let fields = store.run_datalog(
        &format!("?[ctx, owner_kind, owner, name, field_type, required] := \
            *field{{workspace: $ws, context: ctx, owner_kind, owner, name, field_type, required, state: '{state}' @ 'NOW'}}"),
        workspace,
    ).unwrap_or_default();

    let methods = store.run_datalog(
        &format!("?[ctx, owner_kind, owner, name, description, return_type] := \
            *method{{workspace: $ws, context: ctx, owner_kind, owner, name, description, return_type, state: '{state}' @ 'NOW'}}"),
        workspace,
    ).unwrap_or_default();

    let method_params = store.run_datalog(
        &format!("?[ctx, owner_kind, owner, method, name, param_type, required] := \
            *method_param{{workspace: $ws, context: ctx, owner_kind, owner, method, name, param_type, required, state: '{state}' @ 'NOW'}}"),
        workspace,
    ).unwrap_or_default();

    let invariants = store
        .run_datalog(
            &format!(
                "?[ctx, entity, text] := \
            *invariant{{workspace: $ws, context: ctx, entity, text, state: '{state}' @ 'NOW'}}"
            ),
            workspace,
        )
        .unwrap_or_default();

    let vo_rules = store.run_datalog(
        &format!("?[ctx, vo, text] := \
            *vo_rule{{workspace: $ws, context: ctx, value_object: vo, text, state: '{state}' @ 'NOW'}}"),
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
        let path = temp_dir().join(format!(
            "dendrites_tools_test_{}_{}.db",
            std::process::id(),
            id
        ));
        Store::open(&path).unwrap()
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
        assert_eq!(tools.len(), 9);
    }

    #[test]
    fn test_safe_to_delete_dispatch() {
        let store = test_store();
        let ws = format!("/tmp/test-can-del-{}", std::process::id());
        store
            .save_desired(
                &ws,
                &DomainModel {
                    name: "P".into(),
                    description: "".into(),
                    bounded_contexts: vec![BoundedContext {
                        name: "Sales".into(),
                        description: "".into(),
                        module_path: "src/sales".into(),
                        ownership: Ownership::default(),
                        aggregates: vec![],
                        policies: vec![],
                        read_models: vec![],
                        entities: vec![Entity {
                            name: "Order".into(),
                            description: "".into(),
                            aggregate_root: true,
                            fields: vec![],
                            methods: vec![],
                            invariants: vec![],
                            file_path: None,
                            start_line: None,
                            end_line: None,
                        }],
                        value_objects: vec![],
                        services: vec![],
                        repositories: vec![],
                        events: vec![],
                        modules: vec![],
                        dependencies: vec![],
                        api_endpoints: vec![],
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
                },
            )
            .unwrap();

        let result = call_tool(
            &store,
            &ws,
            "safe_to_delete",
            &json!({
                "context": "Sales",
                "entity": "Order"
            }),
        );
        assert_eq!(result.is_error, None);
        let ContentBlock::Text { text } = &result.content[0];
        let parsed: serde_json::Value = serde_json::from_str(text).unwrap();
        // Order has no references, so it should be deletable
        assert_eq!(parsed["status"], "true");
    }

    #[test]
    fn test_check_dispatch() {
        let store = test_store();
        let ws = format!("/tmp/test-invariant-{}", std::process::id());
        // No data = no violations
        let result = call_tool(
            &store,
            &ws,
            "check",
            &json!({
                "check_name": "circular_deps"
            }),
        );
        assert_eq!(result.is_error, None);
        let ContentBlock::Text { text } = &result.content[0];
        let parsed: serde_json::Value = serde_json::from_str(text).unwrap();
        assert_eq!(parsed["status"], "true");
        assert_eq!(parsed["count"], 0);
    }

    #[test]
    fn test_check_unknown() {
        let store = test_store();
        let result = call_tool(
            &store,
            "/tmp/test",
            "check",
            &json!({
                "check_name": "nonexistent"
            }),
        );
        assert_eq!(result.is_error, Some(true));
    }

    #[test]
    fn test_how_connected_dispatch() {
        let store = test_store();
        let ws = format!("/tmp/test-deppath-{}", std::process::id());
        store
            .save_desired(
                &ws,
                &DomainModel {
                    name: "P".into(),
                    description: "".into(),
                    bounded_contexts: vec![
                        BoundedContext {
                            name: "A".into(),
                            description: "".into(),
                            module_path: "src/a".into(),
                            ownership: Ownership::default(),
                            aggregates: vec![],
                            policies: vec![],
                            read_models: vec![],
                            entities: vec![],
                            value_objects: vec![],
                            services: vec![],
                            repositories: vec![],
                            events: vec![],
                            modules: vec![],
                            dependencies: vec!["B".into()],
                            api_endpoints: vec![],
                        },
                        BoundedContext {
                            name: "B".into(),
                            description: "".into(),
                            module_path: "src/b".into(),
                            ownership: Ownership::default(),
                            aggregates: vec![],
                            policies: vec![],
                            read_models: vec![],
                            entities: vec![],
                            value_objects: vec![],
                            services: vec![],
                            repositories: vec![],
                            events: vec![],
                            modules: vec![],
                            dependencies: vec![],
                            api_endpoints: vec![],
                        },
                    ],
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
                },
            )
            .unwrap();

        let result = call_tool(
            &store,
            &ws,
            "how_connected",
            &json!({
                "from": "A",
                "to": "B"
            }),
        );
        assert_eq!(result.is_error, None);
        let ContentBlock::Text { text } = &result.content[0];
        let parsed: serde_json::Value = serde_json::from_str(text).unwrap();
        assert_eq!(parsed["reachable"], true);
    }
}
