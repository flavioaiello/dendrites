import re

with open("src/store/cozo.rs", "r") as f:
    text = f.read()

text = re.sub(r"(pub fn upsert_service)", r"###UPSERT_SERVICE###", text, count=1)

methods_injection = """
    pub fn upsert_api_endpoint(&self, workspace_path: &str, ctx: &str, ep: &APIEndpoint) -> Result<()> {
        let ws = canonicalize_path(workspace_path);
        self.ensure_project(workspace_path)?;
        let params = params_map(&[
            ("ws", &ws), ("ctx", ctx), ("id", &ep.id), ("svc", &ep.service_id),
            ("met", &ep.method), ("path", &ep.route_pattern), ("desc", &ep.description)
        ]);
        self.db.run_script(
            "?[workspace, context, id, state, service_id, method, route_pattern, description] <- \\
             [[$ws, $ctx, $id, \desired',
