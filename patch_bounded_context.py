import re

with open('src/domain/model.rs', 'r') as f:
    text = f.read()

find_str = "pub services: Vec<Service>,"
add_str = "    #[serde(default)]\n    pub api_endpoints: Vec<APIEndpoint>,"

if 'pub api_endpoints' not in text:
    text = text.replace(find_str, find_str + "\n" + add_str)

with open('src/domain/model.rs', 'w') as f:
    f.write(text)

print("Patched BoundedContext")
