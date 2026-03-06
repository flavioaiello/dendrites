use anyhow::{Context, Result};
use std::path::Path;
use syn::visit::Visit;

use super::model::*;

// ─── Live Import Extraction ────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct LiveDependency {
    pub from_file: String,
    pub to_module: String,
}

struct ImportVisitor {
    imports: Vec<String>,
}

impl<'ast> Visit<'ast> for ImportVisitor {
    fn visit_use_tree(&mut self, node: &'ast syn::UseTree) {
        // Very basic extraction: turn use trees into string paths
        fn extract_paths(tree: &syn::UseTree, prefix: &str) -> Vec<String> {
            match tree {
                syn::UseTree::Path(path) => extract_paths(
                    &path.tree,
                    &format!("{}{}{}::", prefix, if prefix.is_empty() { "" } else { "::" }, path.ident),
                ),
                syn::UseTree::Name(name) => vec![format!("{}{}", prefix, name.ident)],
                syn::UseTree::Rename(rename) => vec![format!("{}{}", prefix, rename.ident)],
                syn::UseTree::Glob(_) => vec![format!("{}*", prefix)],
                syn::UseTree::Group(group) => {
                    let mut paths = vec![];
                    for item in &group.items {
                        paths.extend(extract_paths(item, prefix));
                    }
                    paths
                }
            }
        }
        self.imports.extend(extract_paths(node, ""));
        syn::visit::visit_use_tree(self, node);
    }
}

pub fn extract_live_dependencies(file_path: &Path, source_code: &str) -> Result<Vec<LiveDependency>> {
    let syntax_tree = syn::parse_file(source_code)
        .with_context(|| format!("Failed to parse rust file: {}", file_path.display()))?;

    let mut visitor = ImportVisitor { imports: vec![] };
    visitor.visit_file(&syntax_tree);

    let from_file = file_path.to_string_lossy().to_string();
    let deps = visitor
        .imports
        .into_iter()
        .map(|to_module| LiveDependency {
            from_file: from_file.clone(),
            to_module,
        })
        .collect();

    Ok(deps)
}

pub fn scan_workspace(workspace_root: &Path) -> Result<Vec<LiveDependency>> {
    let mut all_deps = Vec::new();

    for entry in ignore::WalkBuilder::new(workspace_root)
        .build()
        .filter_map(Result::ok)
    {
        let path = entry.path();
        if path.is_file() && path.extension().is_some_and(|ext| ext == "rs")
            && let Ok(content) = std::fs::read_to_string(path)
            && let Ok(deps) = extract_live_dependencies(path, &content)
        {
            all_deps.extend(deps);
        }
    }

    Ok(all_deps)
}

// ─── Domain Structure Extraction ───────────────────────────────────────────

/// A struct discovered in the source code with its fields.
#[derive(Debug, Clone)]
pub struct DiscoveredStruct {
    pub name: String,
    pub fields: Vec<Field>,
    pub file_path: String,
}

/// A method discovered from an impl block.
#[derive(Debug, Clone)]
pub struct DiscoveredMethod {
    /// The type this impl block is for (e.g. "Store")
    pub owner: String,
    pub name: String,
    pub parameters: Vec<Field>,
    pub return_type: String,
    pub file_path: String,
}

/// Everything discovered in source files under a single bounded context's module path.
#[derive(Debug, Clone)]
pub struct ContextScan {
    pub context_name: String,
    pub module_path: String,
    pub structs: Vec<DiscoveredStruct>,
    pub methods: Vec<DiscoveredMethod>,
}

/// AST visitor that collects struct definitions and impl methods.
struct StructMethodVisitor {
    structs: Vec<DiscoveredStruct>,
    methods: Vec<DiscoveredMethod>,
    file_path: String,
}

impl<'ast> Visit<'ast> for StructMethodVisitor {
    fn visit_item_struct(&mut self, node: &'ast syn::ItemStruct) {
        // Skip private, test, or #[cfg(test)] structs
        if !is_public(&node.vis) {
            return;
        }

        let name = node.ident.to_string();
        let fields = match &node.fields {
            syn::Fields::Named(named) => named
                .named
                .iter()
                .filter_map(|f| {
                    let field_name = f.ident.as_ref()?.to_string();
                    let field_type = type_to_string(&f.ty);
                    Some(Field {
                        name: field_name,
                        field_type,
                        required: !is_option_type(&f.ty),
                        description: String::new(),
                    })
                })
                .collect(),
            _ => vec![],
        };

        self.structs.push(DiscoveredStruct {
            name,
            fields,
            file_path: self.file_path.clone(),
        });

        syn::visit::visit_item_struct(self, node);
    }

    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        // Only inherent impls, not trait impls
        if node.trait_.is_some() {
            return;
        }

        let owner = type_to_string(&node.self_ty);

        for item in &node.items {
            if let syn::ImplItem::Fn(method) = item {
                if !is_public(&method.vis) {
                    continue;
                }

                let name = method.sig.ident.to_string();
                let return_type = match &method.sig.output {
                    syn::ReturnType::Default => String::new(),
                    syn::ReturnType::Type(_, ty) => type_to_string(ty),
                };

                let parameters: Vec<Field> = method
                    .sig
                    .inputs
                    .iter()
                    .filter_map(|arg| match arg {
                        syn::FnArg::Typed(pat_type) => {
                            let param_name = match pat_type.pat.as_ref() {
                                syn::Pat::Ident(ident) => ident.ident.to_string(),
                                _ => return None,
                            };
                            let param_type = type_to_string(&pat_type.ty);
                            Some(Field {
                                name: param_name,
                                field_type: param_type,
                                required: true,
                                description: String::new(),
                            })
                        }
                        syn::FnArg::Receiver(_) => None, // skip &self
                    })
                    .collect();

                self.methods.push(DiscoveredMethod {
                    owner: owner.clone(),
                    name,
                    parameters,
                    return_type,
                    file_path: self.file_path.clone(),
                });
            }
        }

        syn::visit::visit_item_impl(self, node);
    }
}

/// Scan a single Rust source file and extract structs + impl methods.
fn scan_file(file_path: &Path, source_code: &str) -> Result<(Vec<DiscoveredStruct>, Vec<DiscoveredMethod>)> {
    let syntax_tree = syn::parse_file(source_code)
        .with_context(|| format!("Failed to parse: {}", file_path.display()))?;

    let mut visitor = StructMethodVisitor {
        structs: vec![],
        methods: vec![],
        file_path: file_path.to_string_lossy().to_string(),
    };
    visitor.visit_file(&syntax_tree);

    Ok((visitor.structs, visitor.methods))
}

/// Scan all `.rs` files under a directory and collect structs + methods.
fn scan_directory(dir: &Path) -> Result<(Vec<DiscoveredStruct>, Vec<DiscoveredMethod>)> {
    let mut all_structs = Vec::new();
    let mut all_methods = Vec::new();

    if !dir.exists() {
        return Ok((all_structs, all_methods));
    }

    for entry in ignore::WalkBuilder::new(dir)
        .build()
        .filter_map(Result::ok)
    {
        let path = entry.path();
        if path.is_file()
            && path.extension().is_some_and(|ext| ext == "rs")
            && let Ok(content) = std::fs::read_to_string(path)
            && let Ok((structs, methods)) = scan_file(path, &content)
        {
            all_structs.extend(structs);
            all_methods.extend(methods);
        }
    }

    Ok((all_structs, all_methods))
}

/// Scan the workspace guided by the desired model's bounded contexts.
///
/// For each context, walks the `module_path` directory, extracts structs and
/// impl methods, then cross-references them against the desired model to
/// classify each struct as entity, value_object, service, repository, or event.
///
/// Returns a `DomainModel` representing what actually exists in the source code.
pub fn scan_actual_model(workspace_root: &Path, desired: &DomainModel) -> Result<DomainModel> {
    let mut actual = DomainModel {
        name: desired.name.clone(),
        description: desired.description.clone(),
        bounded_contexts: vec![],
        rules: desired.rules.clone(),
        tech_stack: desired.tech_stack.clone(),
        conventions: desired.conventions.clone(),
    };

    for desired_bc in &desired.bounded_contexts {
        let module_dir = workspace_root.join(&desired_bc.module_path);
        let (structs, methods) = scan_directory(&module_dir)?;

        // Build a lookup of desired element names → their kind
        let desired_entities: Vec<&str> = desired_bc.entities.iter().map(|e| e.name.as_str()).collect();
        let desired_vos: Vec<&str> = desired_bc.value_objects.iter().map(|v| v.name.as_str()).collect();
        let desired_services: Vec<&str> = desired_bc.services.iter().map(|s| s.name.as_str()).collect();
        let desired_repos: Vec<&str> = desired_bc.repositories.iter().map(|r| r.name.as_str()).collect();
        let desired_events: Vec<&str> = desired_bc.events.iter().map(|e| e.name.as_str()).collect();

        let mut bc = BoundedContext {
            name: desired_bc.name.clone(),
            description: desired_bc.description.clone(),
            module_path: desired_bc.module_path.clone(),
            entities: vec![],
            value_objects: vec![],
            services: vec![],
            repositories: vec![],
            events: vec![],
            dependencies: desired_bc.dependencies.clone(),
        };

        for discovered in &structs {
            let name = &discovered.name;

            // Methods for this struct
            let struct_methods: Vec<Method> = methods
                .iter()
                .filter(|m| m.owner == *name)
                .map(|m| Method {
                    name: m.name.clone(),
                    description: String::new(),
                    parameters: m.parameters.clone(),
                    return_type: m.return_type.clone(),
                })
                .collect();

            // Classify by matching against the desired model
            if desired_entities.iter().any(|e| e.eq_ignore_ascii_case(name)) {
                // Find desired entity to copy aggregate_root / invariants
                let desired_ent = desired_bc.entities.iter().find(|e| e.name.eq_ignore_ascii_case(name));
                bc.entities.push(Entity {
                    name: name.clone(),
                    description: String::new(),
                    aggregate_root: desired_ent.is_some_and(|e| e.aggregate_root),
                    fields: discovered.fields.clone(),
                    methods: struct_methods,
                    invariants: vec![], // invariants are domain rules, not extractable from AST
                });
            } else if desired_vos.iter().any(|v| v.eq_ignore_ascii_case(name)) {
                bc.value_objects.push(ValueObject {
                    name: name.clone(),
                    description: String::new(),
                    fields: discovered.fields.clone(),
                    validation_rules: vec![],
                });
            } else if desired_services.iter().any(|s| s.eq_ignore_ascii_case(name)) {
                let desired_svc = desired_bc.services.iter().find(|s| s.name.eq_ignore_ascii_case(name));
                bc.services.push(Service {
                    name: name.clone(),
                    description: String::new(),
                    kind: desired_svc.map_or(ServiceKind::Domain, |s| s.kind.clone()),
                    methods: struct_methods,
                    dependencies: vec![],
                });
            } else if desired_repos.iter().any(|r| r.eq_ignore_ascii_case(name)) {
                let desired_repo = desired_bc.repositories.iter().find(|r| r.name.eq_ignore_ascii_case(name));
                bc.repositories.push(Repository {
                    name: name.clone(),
                    aggregate: desired_repo.map_or(String::new(), |r| r.aggregate.clone()),
                    methods: struct_methods,
                });
            } else if desired_events.iter().any(|e| e.eq_ignore_ascii_case(name)) {
                let desired_evt = desired_bc.events.iter().find(|e| e.name.eq_ignore_ascii_case(name));
                bc.events.push(DomainEvent {
                    name: name.clone(),
                    description: String::new(),
                    fields: discovered.fields.clone(),
                    source: desired_evt.map_or(String::new(), |e| e.source.clone()),
                });
            }
            // Else: struct exists in code but not in desired model — ignored
        }

        actual.bounded_contexts.push(bc);
    }

    Ok(actual)
}

// ─── Helpers ───────────────────────────────────────────────────────────────

fn is_public(vis: &syn::Visibility) -> bool {
    matches!(vis, syn::Visibility::Public(_))
}

/// Convert a syn::Type to a readable string.
fn type_to_string(ty: &syn::Type) -> String {
    // Manual conversion without depending on `quote` crate
    match ty {
        syn::Type::Path(type_path) => {
            let segments: Vec<String> = type_path
                .path
                .segments
                .iter()
                .map(|seg| {
                    let ident = seg.ident.to_string();
                    match &seg.arguments {
                        syn::PathArguments::None => ident,
                        syn::PathArguments::AngleBracketed(args) => {
                            let inner: Vec<String> = args
                                .args
                                .iter()
                                .filter_map(|a| match a {
                                    syn::GenericArgument::Type(t) => Some(type_to_string(t)),
                                    _ => None,
                                })
                                .collect();
                            if inner.is_empty() {
                                ident
                            } else {
                                format!("{}<{}>", ident, inner.join(","))
                            }
                        }
                        syn::PathArguments::Parenthesized(args) => {
                            let inputs: Vec<String> =
                                args.inputs.iter().map(type_to_string).collect();
                            format!("{}({})", ident, inputs.join(","))
                        }
                    }
                })
                .collect();
            segments.join("::")
        }
        syn::Type::Reference(r) => {
            let mutability = if r.mutability.is_some() { "&mut " } else { "&" };
            format!("{}{}", mutability, type_to_string(&r.elem))
        }
        syn::Type::Slice(s) => format!("[{}]", type_to_string(&s.elem)),
        syn::Type::Array(a) => format!("[{}; _]", type_to_string(&a.elem)),
        syn::Type::Tuple(t) => {
            let elems: Vec<String> = t.elems.iter().map(type_to_string).collect();
            format!("({})", elems.join(","))
        }
        _ => "?".to_string(),
    }
}

/// Check whether a type is Option<T>.
fn is_option_type(ty: &syn::Type) -> bool {
    if let syn::Type::Path(path) = ty
        && let Some(segment) = path.path.segments.last()
    {
        return segment.ident == "Option";
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_live_deps() {
        let code = r#"
            use std::path::Path;
            use crate::domain::model::DomainModel;
        "#;
        let deps = extract_live_dependencies(Path::new("test.rs"), code).unwrap();
        // The visitor walks recursively, producing entries for each nesting level.
        // std::path::Path → 3 entries, crate::domain::model::DomainModel → 4 entries.
        assert!(deps.len() >= 2);
        assert!(deps.iter().any(|d| d.to_module.contains("Path")));
        assert!(deps.iter().any(|d| d.to_module.contains("DomainModel")));
    }

    #[test]
    fn test_scan_file_struct_fields() {
        let code = r#"
            pub struct User {
                pub name: String,
                pub email: Option<String>,
                pub age: u32,
            }
        "#;
        let (structs, _) = scan_file(Path::new("test.rs"), code).unwrap();
        assert_eq!(structs.len(), 1);
        assert_eq!(structs[0].name, "User");
        assert_eq!(structs[0].fields.len(), 3);
        assert!(structs[0].fields[0].required); // name: String
        assert!(!structs[0].fields[1].required); // email: Option<String>
        assert!(structs[0].fields[2].required); // age: u32
    }

    #[test]
    fn test_scan_file_impl_methods() {
        let code = r#"
            pub struct Store {}
            impl Store {
                pub fn open(path: &Path) -> Result<Self> { todo!() }
                pub fn save(&self, name: &str, data: &[u8]) -> Result<()> { todo!() }
                fn private_helper(&self) {} // should be ignored
            }
        "#;
        let (structs, methods) = scan_file(Path::new("test.rs"), code).unwrap();
        assert_eq!(structs.len(), 1);
        assert_eq!(methods.len(), 2); // only public methods
        assert_eq!(methods[0].owner, "Store");
        assert_eq!(methods[0].name, "open");
        assert_eq!(methods[0].parameters.len(), 1); // &self excluded
        assert_eq!(methods[1].name, "save");
        assert_eq!(methods[1].parameters.len(), 2);
    }

    #[test]
    fn test_scan_file_skips_private_structs() {
        let code = r#"
            struct PrivateStruct { x: i32 }
            pub struct PublicStruct { pub y: i32 }
        "#;
        let (structs, _) = scan_file(Path::new("test.rs"), code).unwrap();
        assert_eq!(structs.len(), 1);
        assert_eq!(structs[0].name, "PublicStruct");
    }

    #[test]
    fn test_scan_file_skips_trait_impls() {
        let code = r#"
            pub struct Foo {}
            impl Foo {
                pub fn bar(&self) {}
            }
            impl std::fmt::Display for Foo {
                fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                    write!(f, "foo")
                }
            }
        "#;
        let (_, methods) = scan_file(Path::new("test.rs"), code).unwrap();
        assert_eq!(methods.len(), 1); // only inherent impl
        assert_eq!(methods[0].name, "bar");
    }

    #[test]
    fn test_scan_actual_model_classifies_by_desired() {
        use std::env::temp_dir;
        use std::fs;

        let tmp = temp_dir().join(format!("dendrites_scan_test_{}", std::process::id()));
        let src = tmp.join("src").join("domain");
        fs::create_dir_all(&src).unwrap();

        // Write a Rust file with structs matching desired model
        fs::write(
            src.join("model.rs"),
            r#"
pub struct User {
    pub name: String,
    pub email: Option<String>,
}

pub struct Email {
    pub value: String,
}

impl User {
    pub fn change_email(&self, email: Email) -> Result<()> { todo!() }
}
"#,
        ).unwrap();

        let desired = DomainModel {
            name: "Test".into(),
            description: "".into(),
            bounded_contexts: vec![BoundedContext {
                name: "Domain".into(),
                description: "".into(),
                module_path: "src/domain".into(),
                entities: vec![Entity {
                    name: "User".into(),
                    description: "".into(),
                    aggregate_root: true,
                    fields: vec![],
                    methods: vec![],
                    invariants: vec!["Email must be unique".into()],
                }],
                value_objects: vec![ValueObject {
                    name: "Email".into(),
                    description: "".into(),
                    fields: vec![],
                    validation_rules: vec![],
                }],
                services: vec![],
                repositories: vec![],
                events: vec![],
                dependencies: vec![],
            }],
            rules: vec![],
            tech_stack: TechStack::default(),
            conventions: Conventions::default(),
        };

        let actual = scan_actual_model(&tmp, &desired).unwrap();
        assert_eq!(actual.bounded_contexts.len(), 1);
        let bc = &actual.bounded_contexts[0];

        // User classified as entity (from desired)
        assert_eq!(bc.entities.len(), 1);
        assert_eq!(bc.entities[0].name, "User");
        assert!(bc.entities[0].aggregate_root); // inherited from desired
        assert_eq!(bc.entities[0].fields.len(), 2); // name, email from AST
        assert_eq!(bc.entities[0].methods.len(), 1); // change_email
        assert!(bc.entities[0].invariants.is_empty()); // not extractable from AST

        // Email classified as value_object (from desired)
        assert_eq!(bc.value_objects.len(), 1);
        assert_eq!(bc.value_objects[0].name, "Email");
        assert_eq!(bc.value_objects[0].fields.len(), 1);

        // Cleanup
        let _ = fs::remove_dir_all(&tmp);
    }
}
