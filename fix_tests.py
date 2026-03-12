import os
import glob
import re

for filepath in glob.glob("**/*.rs", recursive=True):
    if "target" in filepath or "tests/snapshot_test" in filepath:
        continue
    
    with open(filepath, "r") as f:
        text = f.read()

    # In Rust test scripts, mock BoundedContext objects are usually created as:
    # BoundedContext {
    #     ...
    #     repositories: vec![],
    #     events: vec![],
    #     services: vec![],
    #     some_other_field: ...
    # }
    
    # Let's cleanly inject `api_endpoints: vec![],` after `services:` where it is used in initializers.
    # Look for: "services: [ANYTHING]," and replace with "services: \1,\n                api_endpoints: vec![],"
    # Or "events: [ANYTHING]," -> "events: \1,\n                api_endpoints: vec![],"

    # Some tests do `events: vec![],`
    # Let's replace only once per `events: vec![],` block that belongs to BoundedContext
    
    # It's safer to just do a regex replace over the whole file for the `services: vec![...],` or `repositories: vec![],`
    
    # We will search for `services:\s*vec!\[.*?\],` and insert `api_endpoints: vec![],`
    # But wait, services might have multiple lines. A safer anchor is just inserting a new line inside the `BoundedContext { ... }` blocks which lack `api_endpoints:`.
    
    # Let's perform string manipulation using simple search and replace on common patterns found in the project test code.
    out = text
    if "api_endpoints" not in out and "BoundedContext {" in out:
        # We know there's a BoundedContext initialization without api_endpoints.
        # Find all `events: vec![],` and replace with `events: vec![], api_endpoints: vec![],`
        out = re.sub(r'(events:\s*vec\!\[[^\]]*\],)', r'\1 api_endpoints: vec![],', out)

    with open(filepath, "w") as f:
        f.write(out)

print("done")
