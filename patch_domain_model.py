import re

with open('src/domain/model.rs', 'r') as f:
    text = f.read()

endpoint_code = """
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct APIEndpoint {
    pub id: String,
    pub service_id: String,
    #[serde(default)]
    pub method: String,
    #[serde(default)]
    pub route_pattern: String,
    #[serde(default)]
    pub description: String,
}

"""

if 'pub struct APIEndpoint' not in text:
    text = text.replace('// ─── External Boundaries ──────────────────────────────────────────────────', '// ─── External Boundaries ──────────────────────────────────────────────────\n' + endpoint_code)

with open('src/domain/model.rs', 'w') as f:
    f.write(text)

