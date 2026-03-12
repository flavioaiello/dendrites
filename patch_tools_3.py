import sys
import subprocess

with open("src/mcp/tools.rs", "r") as f:
    content = f.read()

new_tools = """        ToolDefinition {
            name: "ingest_ast_facts".into(),
            description: "Ingest normalized AST-derived facts. Normalizes names and IDs. Associates all observed facts with provenance.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "repository_identity": {"type": "string"},
                    "revision": {"type": "string"},
                    "extractor_version": {"type": "string"},
                    "language": {"type": "string"},
                    "entities": {"type": "array"},
                    "edges": {"type": "array"},
                    "source_spans": {"type": "array"},
                    "upsert_mode": {"type": "boolean"}
                },
                "required": ["repository_identity", "revision", "extractor_version", "language", "entities", "edges", "source_spans", "upsert_mode"]
            }),
        },
        ToolDefinition {
            name: "assert_architecture_policy".into(),
            description: "Persist intended architecture and constraints (e.g. module-to-layer assignments, allowed/forbidden layer dependencies).".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "policy_type": {"type": "string"},
                    "rules": {"type": "array"}
                },
                "required": ["policy_type", "rules"]
            }),
        },
        ToolDefinition {
            name: "check_architectural_invariant".into(),
            description: "Evaluate a curated invariant or restricted proposition. Evaluates safe DSL statements.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "invariant_name": {"type": "string"},
                    "proposition": {"type": "string"}
                }
            }),
        },
"""

parts = content.split("vec![", 1)
new_content = parts[0] + "vec![\n" + new_tools + parts[1]

with open("src/mcp/tools.rs", "w") as f:
    f.write(new_content)

print("done patching tools.rs 3")
