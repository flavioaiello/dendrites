import sys

with open('src/store/cozo.rs', 'r') as f:
    text = f.read()

schema_str = '            ":create external_system_context { workspace: String, system: String, context: String, idx: Int, state: String }",'

new_schemas = """            ":create external_system_context { workspace: String, system: String, context: String, idx: Int, state: String }",
            ":create api_endpoint { workspace: String, context: String, id: String, state: String => service_id: String default '', method: String default '', route_pattern: String default '', description: String default '' }",
            ":create invokes_endpoint { workspace: String, caller_context: String, caller_method: String, endpoint_id: String, state: String }",
            ":create calls_external_system { workspace: String, caller_context: String, caller_method: String, ext_id: String, state: String }","""

if 'api_endpoint {' not in text:
    text = text.replace(schema_str, new_schemas)
    
with open('src/store/cozo.rs', 'w') as f:
    f.write(text)
print("done")
