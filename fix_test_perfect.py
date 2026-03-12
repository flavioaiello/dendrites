import re

with open('src/store/cozo.rs', 'r') as f:
    text = f.read()

test_mod = text.find('mod tests {')
if test_mod != -1:
    before = text[:test_mod]
    tests = text[test_mod:]
    
    # regex match BoundedContext { and we inject api_endpoints: vec![],
    tests = re.sub(r'(BoundedContext\s*\{)', r'\1\n                    api_endpoints: vec![],', tests)
    
    text = before + tests

with open('src/store/cozo.rs', 'w') as f:
    f.write(text)

