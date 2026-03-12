import re

with open('src/store/cozo.rs', 'r') as f:
    text = f.read()

# Fix duplicates: remove everything from the second 'pub fn upsert_api_endpoint' down to its query
first_idx = text.find('pub fn upsert_api_endpoint')
if first_idx != -1:
    second_idx = text.find('pub fn upsert_api_endpoint', first_idx + 1)
    if second_idx != -1:
        # find the end of `query_api_endpoint`
        end_idx = text.find('}\n', text.find('query_api_endpoint', second_idx)) + 2
        text = text[:second_idx] + text[end_idx:]

with open('src/store/cozo.rs', 'w') as f:
    f.write(text)

with open('src/domain/analyze.rs', 'r') as f:
    text = f.read()

text = text.replace("services: Default::default(),\n", "services: Default::default(),\n                api_endpoints: Default::default(),\n")

with open('src/domain/analyze.rs', 'w') as f:
    f.write(text)

print("done fixing compilation")
