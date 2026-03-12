import re

with open('src/store/cozo.rs', 'r') as f:
    text = f.read()

# Only replace in the tests mod
test_start = text.find('mod tests {')
if test_start != -1:
    before = text[:test_start]
    tests_part = text[test_start:]
    tests_part = re.sub(r'(events:\s*vec\!\[[^\]]*\],)', r'\1 api_endpoints: vec![],', tests_part)
    text = before + tests_part

with open('src/store/cozo.rs', 'w') as f:
    f.write(text)
