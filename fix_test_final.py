import re

with open('src/store/cozo.rs', 'r') as f:
    text = f.read()

text = re.sub(r'(policies: vec!\[\],)', r'\1 api_endpoints: vec![],', text)

with open('src/store/cozo.rs', 'w') as f:
    f.write(text)
