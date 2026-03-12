import re

with open('src/store/cozo.rs', 'r') as f:
    text = f.read()

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

text = text.replace("let events: Vec<DomainEvent> = evts", reconstruct_injection + "\n            let events: Vec<DomainEvent> = evts")

with open('src/store/cozo.rs', 'w') as f:
    f.write(text)

with open('src/domain/analyze.rs', 'r') as f:
    text2 = f.read()

# remove double api_endpoints
text2 = re.sub(r'api_endpoints: vec!\[\],\s+api_endpoints:', 'api_endpoints:', text2)
text2 = text2.replace("api_endpoints: Default::default(),\n                api_endpoints:", "api_endpoints:")

with open('src/domain/analyze.rs', 'w') as f:
    f.write(text2)

print("fixed again")
