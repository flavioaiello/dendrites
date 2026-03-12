with open("src/store/cozo.rs", "r") as f:
    text = f.read()

# Add provenance schemas
if "source_span { workspace" not in text:
    text = text.replace(
        ":create event { workspace, context, name, state => description, source }\n        \",",
        ":create event { workspace, context, name, state => description, source }\n        \",\n        \"\n            :create source_span { workspace, context, element_kind, element_name, state => file_path, start_line, end_line }\n        \",\n        \"\n            :create calls { workspace, context, caller, callee, state => }\n        \","
    )

with open("src/store/cozo.rs", "w") as f:
    f.write(text)

