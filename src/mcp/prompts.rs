use crate::domain::model::DomainModel;
use crate::mcp::protocol::*;
use crate::store::Store;

/// Returns the list of prompts the Dendrites server exposes.
pub fn list_prompts() -> Vec<PromptDefinition> {
    vec![PromptDefinition {
        name: "dendrites_guidelines".into(),
        description: "Architecture guidelines enriched by Datalog inference from the domain \
                      knowledge graph. Surfaces circular deps, layer violations, complexity \
                      hotspots, and model health — all computed live from CozoDB."
            .into(),
        arguments: vec![],
    }]
}

/// Resolve a prompt by name.
///
/// The `store` and `workspace_path` are required so that prompts can run
/// Datalog inference queries against the CozoDB knowledge graph, making
/// the domain model a **metalayer** that actively shapes every interaction.
pub fn get_prompt(
    model: &DomainModel,
    store: &Store,
    workspace_path: &str,
    name: &str,
) -> Option<PromptGetResult> {
    match name {
        "dendrites_guidelines" => Some(build_guidelines_prompt(model, store, workspace_path)),
        _ => None,
    }
}

fn build_guidelines_prompt(
    model: &DomainModel,
    store: &Store,
    workspace_path: &str,
) -> PromptGetResult {
    let project_name = &model.name;
    let is_empty = model.bounded_contexts.is_empty();

    let context_line = if is_empty {
        "No bounded contexts defined yet.".to_string()
    } else {
        let names: Vec<&str> = model
            .bounded_contexts
            .iter()
            .map(|bc| bc.name.as_str())
            .collect();
        format!("Bounded contexts: {}", names.join(", "))
    };

    let bootstrap = if is_empty {
        "\n**This project has no domain model yet.** \
         Analyze the codebase first: identify bounded contexts, entities, services, \
         and events using `set_model` (changes build the desired model).\n"
    } else {
        ""
    };

    let rules_section = if model.rules.is_empty() {
        String::new()
    } else {
        let rules: Vec<String> = model
            .rules
            .iter()
            .map(|r| {
                format!(
                    "- **{}** ({}): {}",
                    r.id,
                    format!("{:?}", r.severity).to_lowercase(),
                    r.description
                )
            })
            .collect();
        format!("\n### Rules\n\n{}\n", rules.join("\n"))
    };

    // ── Metalayer: Datalog-inferred health section ─────────────────────
    let health_section = build_health_section(store, workspace_path);

    let text = format!(
        r#"## Dendrites — {project_name}

{context_line}
{bootstrap}
### Workflow

Dendrites maintains two models: the **actual** model (reflects implemented code) and the **desired** model (the target state you are refining).

1. **Before writing code** → call `get_model` (shows actual + desired + pending changes)
2. **To update the desired model** → call `set_model` with the appropriate `kind` (bounded_context, entity, service, event) — auto-saved, returns file path suggestions and dependency validation
3. **To run Datalog analysis** → call `scrutinize` with analysis type (circular_deps, layer_violations, impact, aggregate_quality, dependency_graph, or custom Datalog)
4. **To review changes** → call `refactor` to diff actual vs desired → code actions, file paths, priorities, migration notes
5. **Iterate** steps 2–4 until the desired model is satisfactory
6. **After implementing** → call `refactor` with `action: "accept"` to promote desired → actual
7. **To discard changes** → call `refactor` with `action: "reset"` to revert desired → actual
{rules_section}
{health_section}"#
    );

    PromptGetResult {
        description: format!("Architecture guidelines for {project_name}"),
        messages: vec![PromptMessage {
            role: "user".into(),
            content: ContentBlock::Text { text },
        }],
    }
}

/// Build the Datalog-inferred health section by querying the CozoDB knowledge graph.
///
/// This is the core metalayer mechanism: every prompt is enriched with live
/// inference results from the domain model's relational decomposition. The
/// knowledge graph isn't just stored — it **thinks** and surfaces issues that
/// static JSON analysis can never detect (transitive closure, negation, recursion).
fn build_health_section(store: &Store, workspace_path: &str) -> String {
    let health = match store.model_health(workspace_path) {
        Ok(h) => h,
        Err(_) => return String::new(), // graceful degradation
    };

    let mut sections: Vec<String> = Vec::new();
    sections.push(format!(
        "### Domain Model Health — Score: {}/100\n\n\
         _Computed live from the CozoDB knowledge graph via Datalog inference._",
        health.score
    ));

    // ── Critical issues ────────────────────────────────────────────────
    if !health.circular_deps.is_empty() {
        let items: Vec<String> = health
            .circular_deps
            .iter()
            .map(|[a, b]| format!("- **{a}** ⇄ **{b}**"))
            .collect();
        sections.push(format!(
            "#### ⛔ Circular Dependencies (CRITICAL)\n\n\
             These bounded contexts form dependency cycles. Break them before adding code.\n\n{}",
            items.join("\n")
        ));
    }

    if !health.layer_violations.is_empty() {
        let items: Vec<String> = health
            .layer_violations
            .iter()
            .map(|v| {
                format!(
                    "- **{}.{}** → `{}` (infra)",
                    v.context, v.domain_service, v.infra_dependency
                )
            })
            .collect();
        sections.push(format!(
            "#### ⛔ Layer Violations (CRITICAL)\n\n\
             Domain services depending on infrastructure. Invert these via ports/adapters.\n\n{}",
            items.join("\n")
        ));
    }

    // ── Warnings ───────────────────────────────────────────────────────
    if !health.missing_invariants.is_empty() {
        let items: Vec<String> = health
            .missing_invariants
            .iter()
            .map(|[ctx, ent]| format!("- **{ctx}.{ent}** — aggregate root without invariants"))
            .collect();
        sections.push(format!(
            "#### ⚠️ Missing Invariants\n\n\
             Aggregate roots should enforce business rules. Define invariants for these.\n\n{}",
            items.join("\n")
        ));
    }

    if !health.god_contexts.is_empty() {
        sections.push(format!(
            "#### ⚠️ God Contexts (>10 entities+services)\n\n\
             Consider splitting: {}",
            health.god_contexts.join(", ")
        ));
    }

    if !health.unsourced_events.is_empty() {
        let items: Vec<String> = health
            .unsourced_events
            .iter()
            .map(|[ctx, evt]| format!("- **{ctx}.{evt}**"))
            .collect();
        sections.push(format!(
            "#### ⚠️ Events Without Source\n\n\
             These events have no source entity set. Link them to their originating aggregate.\n\n{}",
            items.join("\n")
        ));
    }

    // ── Info ────────────────────────────────────────────────────────────
    if !health.orphan_contexts.is_empty() {
        sections.push(format!(
            "#### ℹ️ Orphan Contexts\n\n\
             No dependencies to or from: {}. \
             This may be intentional (standalone modules) or indicate missing relationships.",
            health.orphan_contexts.join(", ")
        ));
    }

    // ── Complexity table ───────────────────────────────────────────────
    if !health.complexity.is_empty() {
        let mut table = String::from(
            "#### Complexity Distribution\n\n\
             | Context | Entities | Services | Events | Deps |\n\
             |---------|----------|----------|--------|------|\n",
        );
        for c in &health.complexity {
            table.push_str(&format!(
                "| {} | {} | {} | {} | {} |\n",
                c.context, c.entity_count, c.service_count, c.event_count, c.dep_count
            ));
        }
        sections.push(table);
    }

    // If the model is perfectly healthy
    if health.score == 100 {
        sections.push(
            "✅ **No issues detected.** The domain model is structurally sound.".into(),
        );
    }

    sections.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::*;

    fn test_model() -> DomainModel {
        DomainModel {
            name: "TestProject".into(),
            description: "Test".into(),
            bounded_contexts: vec![BoundedContext {
                name: "Identity".into(),
                description: "".into(),
                module_path: "".into(),
                entities: vec![],
                value_objects: vec![],
                services: vec![],
                repositories: vec![],
                events: vec![],
                dependencies: vec![],
            }],
            rules: vec![],
            tech_stack: TechStack::default(),
            conventions: Conventions::default(),
        }
    }

    fn test_store() -> Store {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "dendrites_prompt_test_{}_{}.db",
            std::process::id(),
            id
        ));
        Store::open(&path).unwrap()
    }

    #[test]
    fn test_list_prompts() {
        let prompts = list_prompts();
        assert_eq!(prompts.len(), 1);
        assert_eq!(prompts[0].name, "dendrites_guidelines");
    }

    #[test]
    fn test_get_prompt_found() {
        let model = test_model();
        let store = test_store();
        let result = get_prompt(&model, &store, "/tmp/test", "dendrites_guidelines");
        assert!(result.is_some());
        let prompt = result.unwrap();
        assert!(prompt.description.contains("TestProject"));
        assert_eq!(prompt.messages.len(), 1);
    }

    #[test]
    fn test_get_prompt_not_found() {
        let model = test_model();
        let store = test_store();
        assert!(get_prompt(&model, &store, "/tmp/test", "nonexistent").is_none());
    }

    #[test]
    fn test_prompt_includes_contexts() {
        let model = test_model();
        let store = test_store();
        let prompt = get_prompt(&model, &store, "/tmp/test", "dendrites_guidelines").unwrap();
        let text = match &prompt.messages[0].content {
            ContentBlock::Text { text } => text,
        };
        assert!(text.contains("Identity"));
    }

    #[test]
    fn test_prompt_includes_workflow() {
        let model = test_model();
        let store = test_store();
        let prompt = get_prompt(&model, &store, "/tmp/test", "dendrites_guidelines").unwrap();
        let text = match &prompt.messages[0].content {
            ContentBlock::Text { text } => text,
        };
        assert!(text.contains("`scrutinize`"));
        assert!(text.contains("Datalog"));
    }

    #[test]
    fn test_prompt_includes_health_section() {
        let model = test_model();
        let store = test_store();
        let prompt = get_prompt(&model, &store, "/tmp/test", "dendrites_guidelines").unwrap();
        let text = match &prompt.messages[0].content {
            ContentBlock::Text { text } => text,
        };
        assert!(text.contains("Domain Model Health"));
        assert!(text.contains("Score:"));
    }

    #[test]
    fn test_prompt_healthy_model_no_warnings() {
        // Empty store → score 100, no issues
        let model = test_model();
        let store = test_store();
        let prompt = get_prompt(&model, &store, "/tmp/test", "dendrites_guidelines").unwrap();
        let text = match &prompt.messages[0].content {
            ContentBlock::Text { text } => text,
        };
        assert!(text.contains("100/100"));
        assert!(text.contains("No issues detected"));
        assert!(!text.contains("CRITICAL"));
    }
}
