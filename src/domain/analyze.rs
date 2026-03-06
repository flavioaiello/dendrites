use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
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

// ─── Crate Discovery ──────────────────────────────────────────────────────

/// A discovered crate root in the workspace.
#[derive(Debug, Clone)]
struct CrateSource {
    /// Name of the crate (from directory name)
    name: String,
    /// Absolute path to the crate's src/ directory
    src_dir: PathBuf,
}

/// Discover all crate source directories in the workspace.
///
/// Walks the workspace looking for `Cargo.toml` files with adjacent `src/`
/// directories. Respects `.gitignore` (skips `target/`, hidden dirs, etc.).
fn discover_crate_sources(workspace_root: &Path) -> Vec<CrateSource> {
    let mut sources = Vec::new();

    // Check the workspace root itself
    let root_cargo = workspace_root.join("Cargo.toml");
    let root_src = workspace_root.join("src");
    if root_cargo.exists() && root_src.is_dir() {
        let name = workspace_root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "root".into());
        sources.push(CrateSource {
            name,
            src_dir: root_src.clone(),
        });
    }

    // Walk for workspace member crates (nested Cargo.toml files)
    for entry in ignore::WalkBuilder::new(workspace_root)
        .max_depth(Some(4))
        .build()
        .filter_map(Result::ok)
    {
        let path = entry.path();
        if path.file_name().is_some_and(|n| n == "Cargo.toml") && path != root_cargo {
            let crate_dir = match path.parent() {
                Some(d) => d,
                None => continue,
            };
            let src = crate_dir.join("src");
            if src.is_dir() {
                let name = crate_dir
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "unknown".into());
                sources.push(CrateSource {
                    name,
                    src_dir: src,
                });
            }
        }
    }

    // Fallback: if no Cargo.toml was found but src/ exists, still scan it.
    // Covers non-Cargo Rust projects and test scenarios.
    if sources.is_empty() {
        let fallback_src = workspace_root.join("src");
        if fallback_src.is_dir() {
            let name = workspace_root
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "root".into());
            sources.push(CrateSource {
                name,
                src_dir: fallback_src,
            });
        }
    }

    sources
}

// ─── Struct Classification ─────────────────────────────────────────────────

/// Classification of a discovered struct based on naming conventions and
/// structural heuristics. Used when no desired model is available or when a
/// struct is not declared in the desired model.
#[derive(Debug, Clone, PartialEq)]
enum StructKind {
    Entity,
    ValueObject,
    Service,
    Repository,
    Event,
}

/// Classify a struct via naming conventions first, then structural heuristics.
///
/// Priority order:
/// 1. Suffix-matched naming conventions (strongest signal)
/// 2. Structural shape (fields vs methods vs both)
fn classify_struct(
    name: &str,
    fields: &[Field],
    methods: &[DiscoveredMethod],
) -> StructKind {
    let upper = name.to_uppercase();

    // ── Naming conventions (suffix-based) ──
    if upper.ends_with("REPOSITORY") || upper.ends_with("REPO") {
        return StructKind::Repository;
    }
    if upper.ends_with("SERVICE")
        || upper.ends_with("HANDLER")
        || upper.ends_with("USECASE")
        || upper.ends_with("INTERACTOR")
    {
        return StructKind::Service;
    }
    if upper.ends_with("EVENT")
        || upper.ends_with("CREATED")
        || upper.ends_with("UPDATED")
        || upper.ends_with("DELETED")
        || upper.ends_with("CHANGED")
        || upper.ends_with("OCCURRED")
    {
        return StructKind::Event;
    }

    // ── Structural heuristics ──
    let struct_methods: Vec<_> = methods.iter().filter(|m| m.owner == name).collect();

    // Fields that carry domain data (not framework wiring)
    let has_data_fields = fields.iter().any(|f| {
        !f.field_type.starts_with("Arc<")
            && !f.field_type.starts_with("Box<dyn")
            && !f.field_type.starts_with("Rc<")
            && !f.field_type.starts_with("&")
    });

    if !has_data_fields && !struct_methods.is_empty() {
        return StructKind::Service;
    }
    if has_data_fields && !struct_methods.is_empty() {
        return StructKind::Entity;
    }

    // Has fields, no public methods → pure data → ValueObject
    StructKind::ValueObject
}

// ─── Full Workspace Scan ───────────────────────────────────────────────────

/// Scan the entire workspace bottom-up, discovering ALL public structs and
/// methods across every crate's `src/` directory.
///
/// Bounded contexts are derived from the top-level module directories under
/// each `src/`. For multi-crate workspaces every crate is scanned.
///
/// When a `desired` model is provided it is used as an enrichment overlay:
///   - Structs matching a desired element inherit its classification, metadata
///     (description, invariants, aggregate_root, etc.).
///   - Structs NOT in the desired model are still discovered and classified
///     via naming conventions and structural heuristics.
///   - Desired-only metadata (aggregates, policies, read_models, external
///     systems, architectural decisions, ownership, rules, etc.) is carried
///     forward into the actual model.
pub fn scan_actual_model(
    workspace_root: &Path,
    desired: Option<&DomainModel>,
) -> Result<DomainModel> {
    let project_name = desired
        .map(|d| d.name.clone())
        .unwrap_or_else(|| {
            workspace_root
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "Unnamed".into())
        });

    let mut actual = DomainModel {
        name: project_name,
        description: desired.map_or(String::new(), |d| d.description.clone()),
        bounded_contexts: vec![],
        external_systems: desired.map_or(vec![], |d| d.external_systems.clone()),
        architectural_decisions: desired.map_or(vec![], |d| d.architectural_decisions.clone()),
        ownership: desired.map_or(Ownership::default(), |d| d.ownership.clone()),
        rules: desired.map_or(vec![], |d| d.rules.clone()),
        tech_stack: desired.map_or(TechStack::default(), |d| d.tech_stack.clone()),
        conventions: desired.map_or(Conventions::default(), |d| d.conventions.clone()),
    };

    // 1. Discover all crate source directories
    let crate_sources = discover_crate_sources(workspace_root);
    let multi_crate = crate_sources.len() > 1;

    for crate_src in &crate_sources {
        // 2. Discover bounded contexts from top-level module directories
        let module_dirs: Vec<(String, PathBuf, String)> = {
            let mut contexts = Vec::new();

            if let Ok(entries) = std::fs::read_dir(&crate_src.src_dir) {
                for entry in entries.filter_map(Result::ok) {
                    let path = entry.path();
                    if path.is_dir() {
                        let dir_name = match path.file_name() {
                            Some(n) => n.to_string_lossy().to_string(),
                            None => continue,
                        };
                        if dir_name.starts_with('.') {
                            continue;
                        }
                        let module_path = path
                            .strip_prefix(workspace_root)
                            .unwrap_or(&path)
                            .to_string_lossy()
                            .to_string();
                        let ctx_name = if multi_crate {
                            format!("{}::{}", crate_src.name, dir_name)
                        } else {
                            dir_name
                        };
                        contexts.push((ctx_name, path, module_path));
                    }
                }
            }

            // If src/ has no subdirectories, treat it as a single context
            if contexts.is_empty() {
                let module_path = crate_src
                    .src_dir
                    .strip_prefix(workspace_root)
                    .unwrap_or(&crate_src.src_dir)
                    .to_string_lossy()
                    .to_string();
                contexts.push((
                    crate_src.name.clone(),
                    crate_src.src_dir.clone(),
                    module_path,
                ));
            }

            contexts
        };

        // 3. For each discovered context, scan and classify
        for (ctx_name, scan_dir, module_path) in &module_dirs {
            let (structs, methods) = scan_directory(scan_dir)?;

            // Resolve matching desired bounded context (by name or module_path)
            let desired_bc = desired.and_then(|d| {
                d.bounded_contexts
                    .iter()
                    .find(|bc| {
                        bc.name.eq_ignore_ascii_case(ctx_name)
                            || bc.module_path == *module_path
                    })
            });

            let mut bc = BoundedContext {
                name: ctx_name.to_string(),
                description: desired_bc.map_or(String::new(), |b| b.description.clone()),
                module_path: module_path.clone(),
                ownership: desired_bc.map_or(Ownership::default(), |b| b.ownership.clone()),
                aggregates: desired_bc.map_or(vec![], |b| b.aggregates.clone()),
                policies: desired_bc.map_or(vec![], |b| b.policies.clone()),
                read_models: desired_bc.map_or(vec![], |b| b.read_models.clone()),
                entities: vec![],
                value_objects: vec![],
                services: vec![],
                repositories: vec![],
                events: vec![],
                dependencies: desired_bc.map_or(vec![], |b| b.dependencies.clone()),
            };

            for discovered in &structs {
                let name = &discovered.name;

                // Collect public methods for this struct
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

                // Check if desired model provides an explicit classification
                let desired_kind = desired_bc.and_then(|dbc| {
                    if dbc.entities.iter().any(|e| e.name.eq_ignore_ascii_case(name)) {
                        Some(StructKind::Entity)
                    } else if dbc.value_objects.iter().any(|v| v.name.eq_ignore_ascii_case(name)) {
                        Some(StructKind::ValueObject)
                    } else if dbc.services.iter().any(|s| s.name.eq_ignore_ascii_case(name)) {
                        Some(StructKind::Service)
                    } else if dbc.repositories.iter().any(|r| r.name.eq_ignore_ascii_case(name)) {
                        Some(StructKind::Repository)
                    } else if dbc.events.iter().any(|e| e.name.eq_ignore_ascii_case(name)) {
                        Some(StructKind::Event)
                    } else {
                        None
                    }
                });

                // Use desired classification when available, otherwise heuristic
                let kind = desired_kind
                    .unwrap_or_else(|| classify_struct(name, &discovered.fields, &methods));

                match kind {
                    StructKind::Entity => {
                        let desired_ent = desired_bc
                            .and_then(|dbc| dbc.entities.iter().find(|e| e.name.eq_ignore_ascii_case(name)));
                        bc.entities.push(Entity {
                            name: name.clone(),
                            description: desired_ent.map_or(String::new(), |e| e.description.clone()),
                            aggregate_root: desired_ent.is_some_and(|e| e.aggregate_root),
                            fields: discovered.fields.clone(),
                            methods: struct_methods,
                            invariants: desired_ent.map_or(vec![], |e| e.invariants.clone()),
                        });
                    }
                    StructKind::ValueObject => {
                        let desired_vo = desired_bc
                            .and_then(|dbc| dbc.value_objects.iter().find(|v| v.name.eq_ignore_ascii_case(name)));
                        bc.value_objects.push(ValueObject {
                            name: name.clone(),
                            description: desired_vo.map_or(String::new(), |v| v.description.clone()),
                            fields: discovered.fields.clone(),
                            validation_rules: desired_vo.map_or(vec![], |v| v.validation_rules.clone()),
                        });
                    }
                    StructKind::Service => {
                        let desired_svc = desired_bc
                            .and_then(|dbc| dbc.services.iter().find(|s| s.name.eq_ignore_ascii_case(name)));
                        bc.services.push(Service {
                            name: name.clone(),
                            description: desired_svc.map_or(String::new(), |s| s.description.clone()),
                            kind: desired_svc.map_or(ServiceKind::Domain, |s| s.kind.clone()),
                            methods: struct_methods,
                            dependencies: desired_svc.map_or(vec![], |s| s.dependencies.clone()),
                        });
                    }
                    StructKind::Repository => {
                        let desired_repo = desired_bc
                            .and_then(|dbc| dbc.repositories.iter().find(|r| r.name.eq_ignore_ascii_case(name)));
                        bc.repositories.push(Repository {
                            name: name.clone(),
                            aggregate: desired_repo.map_or(String::new(), |r| r.aggregate.clone()),
                            methods: struct_methods,
                        });
                    }
                    StructKind::Event => {
                        let desired_evt = desired_bc
                            .and_then(|dbc| dbc.events.iter().find(|e| e.name.eq_ignore_ascii_case(name)));
                        bc.events.push(DomainEvent {
                            name: name.clone(),
                            description: desired_evt.map_or(String::new(), |e| e.description.clone()),
                            fields: discovered.fields.clone(),
                            source: desired_evt.map_or(String::new(), |e| e.source.clone()),
                        });
                    }
                }
            }

            actual.bounded_contexts.push(bc);
        }
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
    fn test_classify_struct_naming_conventions() {
        // Repository suffix
        assert_eq!(
            classify_struct("UserRepository", &[], &[]),
            StructKind::Repository,
        );
        assert_eq!(
            classify_struct("OrderRepo", &[], &[]),
            StructKind::Repository,
        );

        // Service suffix
        assert_eq!(
            classify_struct("PaymentService", &[], &[]),
            StructKind::Service,
        );
        assert_eq!(
            classify_struct("AuthHandler", &[], &[]),
            StructKind::Service,
        );

        // Event suffix
        assert_eq!(
            classify_struct("OrderCreated", &[], &[]),
            StructKind::Event,
        );
        assert_eq!(
            classify_struct("UserDeletedEvent", &[], &[]),
            StructKind::Event,
        );
    }

    #[test]
    fn test_classify_struct_structural_heuristics() {
        let data_fields = vec![Field {
            name: "name".into(),
            field_type: "String".into(),
            required: true,
            description: String::new(),
        }];
        let dep_fields = vec![Field {
            name: "store".into(),
            field_type: "Arc<Store>".into(),
            required: true,
            description: String::new(),
        }];
        let methods = vec![DiscoveredMethod {
            owner: "Foo".into(),
            name: "do_thing".into(),
            parameters: vec![],
            return_type: String::new(),
            file_path: String::new(),
        }];

        // Data fields + methods → Entity
        assert_eq!(
            classify_struct("Foo", &data_fields, &methods),
            StructKind::Entity,
        );

        // Data fields, no methods → ValueObject
        assert_eq!(
            classify_struct("Foo", &data_fields, &[]),
            StructKind::ValueObject,
        );

        // Only dependency fields + methods → Service
        assert_eq!(
            classify_struct("Foo", &dep_fields, &methods),
            StructKind::Service,
        );
    }

    #[test]
    fn test_scan_actual_model_discovers_without_desired() {
        use std::env::temp_dir;
        use std::fs;

        let tmp = temp_dir().join(format!("dendrites_nodesc_test_{}", std::process::id()));
        let src = tmp.join("src").join("billing");
        fs::create_dir_all(&src).unwrap();

        fs::write(
            src.join("types.rs"),
            r#"
pub struct Invoice {
    pub id: u64,
    pub total: f64,
}

pub struct Currency {
    pub code: String,
}

pub struct InvoiceRepository {}

impl Invoice {
    pub fn apply_discount(&self, pct: f64) -> f64 { todo!() }
}

impl InvoiceRepository {
    pub fn find_by_id(&self, id: u64) -> Option<Invoice> { todo!() }
}
"#,
        )
        .unwrap();

        // No desired model at all — pure heuristic discovery
        let actual = scan_actual_model(&tmp, None).unwrap();

        assert_eq!(actual.bounded_contexts.len(), 1);
        let bc = &actual.bounded_contexts[0];
        assert_eq!(bc.name, "billing");

        // Invoice: has data fields + methods → Entity (heuristic)
        assert!(bc.entities.iter().any(|e| e.name == "Invoice"));
        let invoice = bc.entities.iter().find(|e| e.name == "Invoice").unwrap();
        assert_eq!(invoice.fields.len(), 2);
        assert_eq!(invoice.methods.len(), 1);

        // Currency: has data fields, no methods → ValueObject (heuristic)
        assert!(bc.value_objects.iter().any(|v| v.name == "Currency"));

        // InvoiceRepository: naming convention → Repository
        assert!(bc.repositories.iter().any(|r| r.name == "InvoiceRepository"));

        let _ = fs::remove_dir_all(&tmp);
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
                name: "domain".into(),
                description: "".into(),
                module_path: "src/domain".into(),
                ownership: Ownership::default(),
                aggregates: vec![],
                policies: vec![],
                read_models: vec![],
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
            external_systems: vec![],
            architectural_decisions: vec![],
            ownership: Ownership::default(),
            rules: vec![],
            tech_stack: TechStack::default(),
            conventions: Conventions::default(),
        };

        let actual = scan_actual_model(&tmp, Some(&desired)).unwrap();
        assert_eq!(actual.bounded_contexts.len(), 1);
        let bc = &actual.bounded_contexts[0];

        // User classified as entity (from desired)
        assert_eq!(bc.entities.len(), 1);
        assert_eq!(bc.entities[0].name, "User");
        assert!(bc.entities[0].aggregate_root); // inherited from desired
        assert_eq!(bc.entities[0].fields.len(), 2); // name, email from AST
        assert_eq!(bc.entities[0].methods.len(), 1); // change_email
        // Invariants carried from desired model enrichment
        assert_eq!(bc.entities[0].invariants.len(), 1);
        assert_eq!(bc.entities[0].invariants[0], "Email must be unique");

        // Email classified as value_object (from desired)
        assert_eq!(bc.value_objects.len(), 1);
        assert_eq!(bc.value_objects[0].name, "Email");
        assert_eq!(bc.value_objects[0].fields.len(), 1);

        // Cleanup
        let _ = fs::remove_dir_all(&tmp);
    }
}
