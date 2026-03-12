import re

with open('src/store/cozo.rs', 'r') as f:
    text = f.read()

# remove all block matches
block_regex = r"let api_endpoints_rows = self\.db\.run_script\([\s\S]*?let api_endpoints: Vec<APIEndpoint> = .*?\.collect\(\);"
text = re.sub(block_regex, "", text)

# Insert it strictly ONCE right before `// Value objects`
reconstruct_injection = """
            let api_endpoints_rows = self.db.run_script(
                "?[id, service_id, method, route_pattern, description] := *api_endpoint{workspace: $ws, context: $ctx, id, state: $st, service_id, method, route_pattern, description}",
                params_map(&[("ws", &ws), ("ctx", &ctx_name), ("st", state)]),
                ScriptMutability::Immutable,
            ).map(|r| r.rows).unwrap_or_default();
            let api_endpoints: Vec<APIEndpoint> = api_endpoints_rows.iter().map(|r| {
                APIEndpoint {
                    id: dv_str(&r[0]),
                    service_id: dv_str(&r[1]),
                    method: dv_str(&r[2]),
                    route_pattern: dv_str(&r[3]),
                    description: dv_str(&r[4]),
                }
            }).collect();
"""

text = text.replace("// Value objects", reconstruct_injection + "\n            // Value objects")

# Then verify BoundedContext
text = text.replace("services,\n                repositories,", "services,\n                api_endpoints,\n                repositories,")

# Oh wait, `api_endpoints` may have already been placed in BoundedContext. Let's fix just in case it's missing or duplicated
text = re.sub(r"api_endpoints,\s+api_endpoints,", "api_endpoints,", text)

if "api_endpoints,\n                repositories," not in text:
    text = text.replace("services,\n                repositories,", "services,\n                api_endpoints,\n                repositories,")

with open('src/store/cozo.rs', 'w') as f:
    f.write(text)

print("done cleaning")
