import re

with open('src/domain/polyglot.rs', 'r') as f:
    text = f.read()

text = text.replace("iface_body.child(i)", "iface_body.child(i as u32)")
text = text.replace("class_body.child(i)", "class_body.child(i as u32)")
text = text.replace("child.child(j)", "child.child(j as u32)")
text = text.replace("params_node.child(i)", "params_node.child(i as u32)")

with open('src/domain/polyglot.rs', 'w') as f:
    f.write(text)

print("done fixing polyglot")

with open('src/domain/analyze.rs', 'r') as f:
    text2 = f.read()

# in BoundedContext construction inside analyze.rs
text2 = text2.replace("services: vec![],\n                repositories: vec![],\n                events: vec![],", "services: vec![],\n                api_endpoints: vec![],\n                repositories: vec![],\n                events: vec![],")

with open('src/domain/analyze.rs', 'w') as f:
    f.write(text2)

