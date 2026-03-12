import re

with open('src/store/cozo.rs', 'r') as f:
    text = f.read()

# 1. Inject into reconstruct_model
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

# Wait, the `BoundedContext` struct in `src/domain/model.rs` was updated to include:
# ...
#     pub services: Vec<Service>,
#     #[serde(default)]
#     pub api_endpoints: Vec<APIEndpoint>,
# ...
# We need to make sure we inject `api_endpoints` where the BoundedContext is instantiated.

text = text.replace("services,\n                repositories,", "services,\n                api_endpoints,\n                repositories,")

# 2. Inject into save_state
save_state_injection = """
            for ep in &bc.api_endpoints {
                let params = params_map(&[
                    ("ws", workspace),
                    ("ctx", &bc.name),
                    ("id", &ep.id),
                    ("st", state),
                    ("svc", &ep.service_id),
                    ("met", &ep.method),
                    ("path", &ep.route_pattern),
                    ("desc", &ep.description),
                ]);
                self.db.run_script(
                    "?[workspace, context, id, state, service_id, method, route_pattern, description] <- \
                     [[$ws, $ctx, $id, $st, $svc, $met, $path, $desc]] \
                     :put api_endpoint { workspace, context, id, state => service_id, method, route_pattern, description }",
                    params,
                    ScriptMutability::Mutable,
                ).map_err(|e| anyhow::anyhow!("save api_endpoint: {:?}", e))?;
            }
"""

text = text.replace("for svc in &bc.services {", save_state_injection + "\n            for svc in &bc.services {")

# 3. Add upsert/remove/query methods
methods_injection = """
    pub fn upsert_api_endpoint(&self, workspace_path: &str, ctx: &str, ep: &APIEndpoint) -> Result<()> {
        let ws = canonicalize_path(workspace_path);
        self.ensure_project(workspace_path)?;
        let params = params_map(&[
            ("ws", &ws), ("ctx", ctx), ("id", &ep.id), ("svc", &ep.service_id),
            ("met", &ep.method), ("path", &ep.route_pattern), ("desc", &ep.description)
        ]);
        self.db.run_script(
            "?[workspace, context, id, state, service_id, method, route_pattern, description] <- \
             [[$ws, $ctx, $id, 'desired', $svc, $met, $path, $desc]] :put api_endpoint { workspace, context, id, state => service_id, method, route_pattern, description }",
            params, ScriptMutability::Mutable
        ).map_err(|e| anyhow::anyhow!("upsert_api_endpoint: {:?}", e))?;
        Ok(())
    }

    pub fn remove_api_endpoint(&self, workspace_path: &str, ctx: &str, id: &str) -> Result<bool> {
        let ws = canonicalize_path(workspace_path);
        let params = params_map(&[("ws", &ws), ("ctx", ctx), ("id", id)]);
        let _ = self.db.run_script(
            "?[workspace, context, id, state] := *api_endpoint{workspace, context, id, state}, workspace = $ws, context = $ctx, id = $id, state = 'desired' :rm api_endpoint { workspace, context, id, state }",
            params, ScriptMutability::Mutable
        ).map_err(|e| anyhow::anyhow!("remove_api_endpoint: {:?}", e))?;
        Ok(true)
    }

    pub fn query_api_endpoint(&self, ws: &str, ctx: &str, id: &str) -> Option<APIEndpoint> {
        let ws = canonicalize_path(ws);
        let rows = self.db.run_script(
            "?[service_id, method, route_pattern, description] := *api_endpoint{workspace: $ws, context: $ctx, id: $id, state: 'desired', service_id, method, route_pattern, description}",
            params_map(&[("ws", &ws), ("ctx", ctx), ("id", id)]),
            ScriptMutability::Immutable
        ).ok()?.rows;
        let row = rows.first()?;
        Some(APIEndpoint {
            id: id.to_string(),
            service_id: dv_str(&row[0]),
            method: dv_str(&row[1]),
            route_pattern: dv_str(&row[2]),
            description: dv_str(&row[3]),
        })
    }
"""

text = text.replace("pub fn upsert_service", methods_injection + "\n    pub fn upsert_service")

# Note: also handle `clear_state`!
clear_state_inject = """        let _ = self.db.run_script(
            "?[workspace, context, id, state] := *api_endpoint{workspace, context, id, state}, workspace = $ws, state = $st :rm api_endpoint { workspace, context, id, state }",
            params.clone(),
            ScriptMutability::Mutable,
        );"""
text = text.replace('let _ = self.db.run_script(\n            "?[workspace, context, name, state] := *service{', clear_state_inject + '\n        let _ = self.db.run_script(\n            "?[workspace, context, name, state] := *service{')

# Also handle copy_state
copy_state_inject = """        let _ = self.db.run_script(
            "?[workspace, context, id, state, service_id, method, route_pattern, description] <- \
             *api_endpoint{workspace: $ws, context, id, state: $s_st, service_id, method, route_pattern, description}, \
             state = $t_st \
             :put api_endpoint { workspace, context, id, state => service_id, method, route_pattern, description }",
            params.clone(),
            ScriptMutability::Mutable,
        )?;"""
text = text.replace('let _ = self.db.run_script(\n            "?[workspace, context, name, state, description, kind] <-\n             *service', copy_state_inject + '\n        let _ = self.db.run_script(\n            "?[workspace, context, name, state, description, kind] <-\n             *service')


with open('src/store/cozo.rs', 'w') as f:
    f.write(text)

print("done")
