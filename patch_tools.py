with open("src/mcp/tools.rs", "r") as f:
    content = f.read()

new_tools = """
        ToolDefinition {
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
     with open("src/mcp/tools.rs", "    content = f.read()

new_tools = """,

new_tools = """
    olD        ToolDe              name: "querte            description: "Return one or more promi            input_schema: json!({
                "type": "object",
                "properties": {
                    "source": {"type": "string"},
                      "type": "object"                  "properties": {
                      "source":                       "target": {"type": "string"}                  },
                "required": sy                "              }),
        },
        ToolDefiniti          },
   xp        Tat     into(),
                        description: "Comp ID or normalized            input_schema: json!({
                "type": "object",
                "properties": {
                    "entity_id": {"type": "string"}
                      "type": "object"                  "properties": {
                      "entity_idty     with open("src/mcp/tools.rs", "    content = "r
new_tools = """,

new_tools = """
    olD        ToolDe 

p
new_tools = ""spl    olD       
#                "type": "object",
                "properties": {
                    "source": {"type": "string"},
              c/                "properties": {
wi                    "source": ")                      "type": "object"          on                      "source":            1)
if parts[0] is not None:
                 "required": sy                "              }),
        },
        ToolDefiniti     s         },
        ToolDefiniti      rint("Patched.")
