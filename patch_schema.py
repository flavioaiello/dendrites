with open("src/store/cozo.rs", "r") as f:
    text = f.read()

# Add imports schema
if "imports_module { workspace" not in text:
    text = text.replace(
        ":create service_dep { workspace, context, service, dependency, state => }\n        \",",
        ":create service_dep { workspace, context, service, dependency, state => }\n        \",\n        \"\n            :create imports_module { workspace, context, source_module, target_module, state => }\n        \","
    )

with open("src/store/cozo.rs", "w") as f:
    f.write(text)

