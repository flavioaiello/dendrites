import re

with open("src/store/cozo.rs", "r") as f:
    content = f.read()

# removing everything after first 'upsert_api_endpoint' up to the next block? Just delete the 2nd and 3rd instances
parts = content.split('    pub fn upsert_api_endpoint(')
if len(parts) > 2:
    # keep only the first split and second (which contains the first definition)
    # wait there might be other stuff after the last `query_api_endpoint`
    pass
