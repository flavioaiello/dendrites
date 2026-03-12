import sys
import subprocess

with open("src/mcp/tools.rs", "r") as f:
    orig = f.read()

subprocess.run(["git", "restore", "src/mcp/tools.rs"], check=False)

with open("src/mcp/tools.rs", "r") as f:
    content = f.read()

new_tools = """        ToolDefinition {
            name: "query_dependency_path".into(),
            description: "Return one or more proof paths between two architectural entities. Outputs explicit path sequences and supporting edges.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "source": {"type": "string"},
                    "target": {"type": "string"}
                },
                "required": ["source", "target"]
            }),
        },
        ToolDefinition {
            name: "query_blast_radius".into(),
            description: "Compute downstream impact of changing or deleting an entity. Return impacted entities, traversal mode, and path witnesses.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "entity_id": {"type": "string"}
                },
                "required": ["entity_id"]
            }),
        },
        ToolDefinition {
            name: "can_delete_symbol".into(),
            description: "Determine whether a function, method, or type can be safely deleted under defined scope. Evaluates inbound reference count and safe-to-delete proofs.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "symbol_id": {"type": "string"}
                },
                "required": ["symbol_id"]
            }),
        },
        ToolDefinition {
            name: "explain_violation".into(),
            description: "Take a violation ID or normalized proposition result and explain it using proof evidence directly mapped to Datalog rules and path witnesses.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "violation_id": {"type": "string"}
                },
                "required": ["violation_id"]
            }),
        },
        ToolDefinition {
            name: "diff_architecture_snapshots".into(),
            description: "Compare two revisions and return added/removed entities, changed dependencies, and newly introduced/resolved violations.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "revision_a": {"type": "string"},
                    "revision_b": {"type": "string"}
                },
                "required": ["revision_a", "revision_b"]
            }),
        },
"""

parts = content.split("vec![", 1)
new_content = parts[0] + "vec![\n" + new_tools + parts[1]

with open("src/mcp/tools.rs", "w") as f:
    f.write(new_content)

print("done patching tools.rs")
