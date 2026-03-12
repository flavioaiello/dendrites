use anyhow::{Context, Result};
use std::path::Path;
use tree_sitter::{Language, Node, Parser, Query, QueryCursor, StreamingIterator};

use super::analyze::{DiscoveredMethod, DiscoveredStruct, LiveDependency};
use super::model::Field;
use super::scanner::AstScanner;

/// Identifies which language family a file belongs to for query selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LangFamily {
    TypeScript,
    Python,
}

/// A polyglot scanner using Tree-Sitter to parse non-Rust files.
pub struct TreeSitterScanner;

impl TreeSitterScanner {
    pub fn new() -> Self {
        Self
    }

    fn get_language(&self, file_path: &Path) -> Option<(Language, LangFamily)> {
        let ext = file_path.extension()?.to_str()?;
        match ext {
            "ts" => Some((
                tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
                LangFamily::TypeScript,
            )),
            "tsx" => Some((
                tree_sitter_typescript::LANGUAGE_TSX.into(),
                LangFamily::TypeScript,
            )),
            "py" => Some((
                tree_sitter_python::LANGUAGE.into(),
                LangFamily::Python,
            )),
            _ => None,
        }
    }

    fn parse_tree(
        &self,
        file_path: &Path,
        source_code: &str,
    ) -> Result<Option<(tree_sitter::Tree, LangFamily)>> {
        let (language, family) = match self.get_language(file_path) {
            Some(pair) => pair,
            None => {
                tracing::debug!(
                    "Unsupported file extension for tree-sitter: {:?}",
                    file_path
                );
                return Ok(None);
            }
        };

        let mut parser = Parser::new();
        parser
            .set_language(&language)
            .map_err(|e| anyhow::anyhow!("Failed to set tree-sitter language: {:?}", e))?;

        let tree = parser
            .parse(source_code, None)
            .context("Failed to parse code with tree-sitter")?;

        Ok(Some((tree, family)))
    }
}

// ─── Helper: extract node text ─────────────────────────────────────────────

fn node_text<'a>(node: Node<'a>, source: &'a str) -> &'a str {
    node.utf8_text(source.as_bytes()).unwrap_or("")
}

/// Iterate children of a node (tree-sitter 0.26 uses u32 indices).
fn children(node: Node<'_>) -> impl Iterator<Item = Node<'_>> + '_ {
    (0..node.child_count() as u32).filter_map(move |i| node.child(i))
}

// ─── Python extraction ─────────────────────────────────────────────────────

fn python_extract_imports(source: &str, tree: &tree_sitter::Tree) -> Vec<String> {
    // Match `import X` and `from X import Y`
    let query_src = r#"
        (import_statement
          name: (dotted_name) @module)
        (import_from_statement
          module_name: (dotted_name) @module)
    "#;

    let language = tree.language();
    let query = match Query::new(&language, query_src) {
        Ok(q) => q,
        Err(e) => {
            tracing::warn!("Failed to compile Python import query: {e}");
            return vec![];
        }
    };

    let mut cursor = QueryCursor::new();
    let root = tree.root_node();
    let mut imports = Vec::new();

    let mut matches = cursor.matches(&query, root, source.as_bytes());
    while let Some(m) = matches.next() {
        for cap in m.captures {
            imports.push(node_text(cap.node, source).to_string());
        }
    }

    imports
}

fn python_extract_classes(
    source: &str,
    tree: &tree_sitter::Tree,
    file_path: &str,
) -> (Vec<DiscoveredStruct>, Vec<DiscoveredMethod>) {
    let query_src = r#"
        (class_definition
          name: (identifier) @class_name
          body: (block) @class_body)
    "#;

    let language = tree.language();
    let query = match Query::new(&language, query_src) {
        Ok(q) => q,
        Err(e) => {
            tracing::warn!("Failed to compile Python class query: {e}");
            return (vec![], vec![]);
        }
    };

    let mut cursor = QueryCursor::new();
    let root = tree.root_node();
    let mut structs = Vec::new();
    let mut methods = Vec::new();

    let class_name_idx = query.capture_index_for_name("class_name").unwrap();
    let class_body_idx = query.capture_index_for_name("class_body").unwrap();

    let mut matches = cursor.matches(&query, root, source.as_bytes());
    while let Some(m) = matches.next() {
        let mut name = "";
        let mut body_node = None;

        for cap in m.captures {
            if cap.index == class_name_idx {
                name = node_text(cap.node, source);
            } else if cap.index == class_body_idx {
                body_node = Some(cap.node);
            }
        }

        if name.is_empty() {
            continue;
        }

        // Extract __init__ fields from type annotations in class body
        let fields = body_node
            .map(|body| python_extract_init_fields(source, body))
            .unwrap_or_default();

        structs.push(DiscoveredStruct {
            name: name.to_string(),
            fields,
            file_path: file_path.to_string(),
        });

        // Extract methods from class body
        if let Some(body) = body_node {
            let class_methods = python_extract_methods(source, body, name, file_path);
            methods.extend(class_methods);
        }
    }

    (structs, methods)
}

/// Extracts fields from `self.x = ...` assignments in `__init__`, using type annotations if present.
fn python_extract_init_fields(source: &str, class_body: Node) -> Vec<Field> {
    let mut fields = Vec::new();

    // Walk the body for function_definition named __init__
    // then find `self.x` attribute assignments
    for child in children(class_body) {
        // Typed class-level field: `name: str = "default"` or `name: int`
        if child.kind() == "expression_statement" {
            if let Some(assign) = child.child(0) {
                if assign.kind() == "assignment" {
                    if let (Some(left), Some(right)) = (
                        assign.child_by_field_name("left"),
                        assign.child_by_field_name("type"),
                    ) {
                        if left.kind() == "identifier" {
                            let field_name = node_text(left, source).to_string();
                            let field_type = node_text(right, source).to_string();
                            // Skip private/dunder unless it's a known pattern
                            if !field_name.starts_with('_') {
                                fields.push(Field {
                                    name: field_name,
                                    field_type,
                                    required: true,
                                    description: String::new(),
                                });
                            }
                        }
                    }
                } else if assign.kind() == "type" {
                    // bare type annotation: `name: str`
                    if let (Some(ident), Some(ty)) = (
                        assign.child_by_field_name("identifier"),
                        assign.child_by_field_name("type"),
                    ) {
                        let field_name = node_text(ident, source).to_string();
                        let field_type = node_text(ty, source).to_string();
                        if !field_name.starts_with('_') {
                            fields.push(Field {
                                name: field_name,
                                field_type,
                                required: true,
                                description: String::new(),
                            });
                        }
                    }
                }
            }
        }

        // Also scan __init__ for `self.x: type = ...` or `self.x = ...`
        if child.kind() == "function_definition" {
            let name_node = child.child_by_field_name("name");
            if name_node.map(|n| node_text(n, source)) == Some("__init__") {
                if let Some(body) = child.child_by_field_name("body") {
                    fields.extend(python_extract_self_assignments(source, body));
                }
            }
        }
    }

    fields
}

fn python_extract_self_assignments(source: &str, init_body: Node) -> Vec<Field> {
    let mut fields = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for stmt in children(init_body) {
        if stmt.kind() != "expression_statement" {
            continue;
        }
        let assign = match stmt.child(0) {
            Some(a) if a.kind() == "assignment" => a,
            _ => continue,
        };
        let left = match assign.child_by_field_name("left") {
            Some(l) => l,
            None => continue,
        };
        if left.kind() != "attribute" {
            continue;
        }
        // Check it starts with `self.`
        let obj = match left.child_by_field_name("object") {
            Some(o) if node_text(o, source) == "self" => o,
            _ => continue,
        };
        let _ = obj;
        let attr = match left.child_by_field_name("attribute") {
            Some(a) => a,
            None => continue,
        };
        let field_name = node_text(attr, source).to_string();
        if field_name.starts_with('_') || !seen.insert(field_name.clone()) {
            continue;
        }

        // Try to get type annotation: `self.x: Type = ...`
        let field_type = assign
            .child_by_field_name("type")
            .map(|t| node_text(t, source).to_string())
            .unwrap_or_default();

        fields.push(Field {
            name: field_name,
            field_type,
            required: true,
            description: String::new(),
        });
    }

    fields
}

fn python_extract_methods(
    source: &str,
    class_body: Node,
    owner: &str,
    file_path: &str,
) -> Vec<DiscoveredMethod> {
    let mut methods = Vec::new();

    for child in children(class_body) {
        if child.kind() != "function_definition" {
            continue;
        }
        let name_node = match child.child_by_field_name("name") {
            Some(n) => n,
            None => continue,
        };
        let method_name = node_text(name_node, source);
        // Skip dunder methods except __init__ is already handled separately
        if method_name.starts_with("__") && method_name.ends_with("__") {
            continue;
        }
        // Skip private methods
        if method_name.starts_with('_') {
            continue;
        }

        let params = child
            .child_by_field_name("parameters")
            .map(|pnode| python_parse_params(source, pnode))
            .unwrap_or_default();

        let return_type = child
            .child_by_field_name("return_type")
            .map(|rt| node_text(rt, source).to_string())
            .unwrap_or_default();

        methods.push(DiscoveredMethod {
            owner: owner.to_string(),
            name: method_name.to_string(),
            parameters: params,
            return_type,
            file_path: file_path.to_string(),
            pre_conditions: vec![],
            post_conditions: vec![],
        });
    }

    methods
}

fn python_parse_params(source: &str, params_node: Node) -> Vec<Field> {
    let mut fields = Vec::new();
    for param in children(params_node) {
        match param.kind() {
            "identifier" => {
                let name = node_text(param, source);
                if name == "self" || name == "cls" {
                    continue;
                }
                fields.push(Field {
                    name: name.to_string(),
                    field_type: String::new(),
                    required: true,
                    description: String::new(),
                });
            }
            "typed_parameter" => {
                let name = param
                    .child_by_field_name("name")
                    .map(|n| node_text(n, source))
                    .unwrap_or("");
                if name == "self" || name == "cls" {
                    continue;
                }
                let ty = param
                    .child_by_field_name("type")
                    .map(|t| node_text(t, source).to_string())
                    .unwrap_or_default();
                fields.push(Field {
                    name: name.to_string(),
                    field_type: ty,
                    required: true,
                    description: String::new(),
                });
            }
            "typed_default_parameter" | "default_parameter" => {
                let name = param
                    .child_by_field_name("name")
                    .map(|n| node_text(n, source))
                    .unwrap_or("");
                if name == "self" || name == "cls" {
                    continue;
                }
                let ty = param
                    .child_by_field_name("type")
                    .map(|t| node_text(t, source).to_string())
                    .unwrap_or_default();
                fields.push(Field {
                    name: name.to_string(),
                    field_type: ty,
                    required: false,
                    description: String::new(),
                });
            }
            _ => {}
        }
    }
    fields
}

// ─── TypeScript/TSX extraction ─────────────────────────────────────────────

fn ts_extract_imports(source: &str, tree: &tree_sitter::Tree) -> Vec<String> {
    let query_src = r#"
        (import_statement
          source: (string) @module)
    "#;

    let language = tree.language();
    let query = match Query::new(&language, query_src) {
        Ok(q) => q,
        Err(e) => {
            tracing::warn!("Failed to compile TS import query: {e}");
            return vec![];
        }
    };

    let mut cursor = QueryCursor::new();
    let root = tree.root_node();
    let mut imports = Vec::new();

    let mut matches = cursor.matches(&query, root, source.as_bytes());
    while let Some(m) = matches.next() {
        for cap in m.captures {
            // Strip quotes from the string literal
            let raw = node_text(cap.node, source);
            let cleaned = raw.trim_matches(|c| c == '\'' || c == '"');
            imports.push(cleaned.to_string());
        }
    }

    imports
}

fn ts_extract_classes(
    source: &str,
    tree: &tree_sitter::Tree,
    file_path: &str,
) -> (Vec<DiscoveredStruct>, Vec<DiscoveredMethod>) {
    let mut structs = Vec::new();
    let mut methods = Vec::new();

    // Extract classes
    let class_query_src = r#"
        (class_declaration
          name: (type_identifier) @class_name
          body: (class_body) @class_body)
    "#;

    let language = tree.language();
    let query = match Query::new(&language, class_query_src) {
        Ok(q) => q,
        Err(e) => {
            tracing::warn!("Failed to compile TS class query: {e}");
            return (vec![], vec![]);
        }
    };

    let class_name_idx = query.capture_index_for_name("class_name").unwrap();
    let class_body_idx = query.capture_index_for_name("class_body").unwrap();

    let mut cursor = QueryCursor::new();
    let root = tree.root_node();

    let mut matches = cursor.matches(&query, root, source.as_bytes());
    while let Some(m) = matches.next() {
        let mut name = "";
        let mut body_node = None;

        for cap in m.captures {
            if cap.index == class_name_idx {
                name = node_text(cap.node, source);
            } else if cap.index == class_body_idx {
                body_node = Some(cap.node);
            }
        }

        if name.is_empty() {
            continue;
        }

        let body = match body_node {
            Some(b) => b,
            None => continue,
        };

        let fields = ts_extract_class_fields(source, body);
        structs.push(DiscoveredStruct {
            name: name.to_string(),
            fields,
            file_path: file_path.to_string(),
        });

        let class_methods = ts_extract_class_methods(source, body, name, file_path);
        methods.extend(class_methods);
    }

    // Also extract interfaces as structs
    let iface_query_src = r#"
        (interface_declaration
          name: (type_identifier) @iface_name
          body: (interface_body) @iface_body)
    "#;

    if let Ok(iface_query) = Query::new(&language, iface_query_src) {
        let iface_name_idx = iface_query.capture_index_for_name("iface_name").unwrap();
        let iface_body_idx = iface_query.capture_index_for_name("iface_body").unwrap();
        let mut cursor2 = QueryCursor::new();

        let mut iface_matches = cursor2.matches(&iface_query, root, source.as_bytes());
        while let Some(m) = iface_matches.next() {
            let mut name = "";
            let mut body_node = None;

            for cap in m.captures {
                if cap.index == iface_name_idx {
                    name = node_text(cap.node, source);
                } else if cap.index == iface_body_idx {
                    body_node = Some(cap.node);
                }
            }

            if name.is_empty() {
                continue;
            }

            let fields = body_node
                .map(|b| ts_extract_interface_fields(source, b))
                .unwrap_or_default();

            structs.push(DiscoveredStruct {
                name: name.to_string(),
                fields,
                file_path: file_path.to_string(),
            });
        }
    }

    (structs, methods)
}

fn ts_extract_class_fields(source: &str, class_body: Node) -> Vec<Field> {
    let mut fields = Vec::new();

    for child in children(class_body) {
        // public_field_definition or property_declaration with type annotation
        let kind = child.kind();
        if kind != "public_field_definition" && kind != "property_declaration" {
            continue;
        }

        // Skip private/protected
        let mut is_private = false;
        for modifier in children(child) {
            let txt = node_text(modifier, source);
            if txt == "private" || txt == "protected" {
                is_private = true;
                break;
            }
        }
        if is_private {
            continue;
        }

        let field_name = child
            .child_by_field_name("name")
            .map(|n| node_text(n, source).to_string())
            .unwrap_or_default();

        if field_name.is_empty() || field_name.starts_with('#') {
            continue;
        }

        let field_type = child
            .child_by_field_name("type")
            .map(|t| node_text(t, source).to_string())
            .unwrap_or_default();

        let required = !field_name.ends_with('?');
        let clean_name = field_name.trim_end_matches('?').to_string();

        fields.push(Field {
            name: clean_name,
            field_type,
            required,
            description: String::new(),
        });
    }

    fields
}

fn ts_extract_interface_fields(source: &str, iface_body: Node) -> Vec<Field> {
    let mut fields = Vec::new();

    for child in children(iface_body) {
        if child.kind() != "property_signature" {
            continue;
        }

        let field_name = child
            .child_by_field_name("name")
            .map(|n| node_text(n, source).to_string())
            .unwrap_or_default();

        if field_name.is_empty() {
            continue;
        }

        let field_type = child
            .child_by_field_name("type")
            .map(|t| node_text(t, source).to_string())
            .unwrap_or_default();

        let required = !field_name.ends_with('?');
        let clean_name = field_name.trim_end_matches('?').to_string();

        fields.push(Field {
            name: clean_name,
            field_type,
            required,
            description: String::new(),
        });
    }

    fields
}

fn ts_extract_class_methods(
    source: &str,
    class_body: Node,
    owner: &str,
    file_path: &str,
) -> Vec<DiscoveredMethod> {
    let mut methods = Vec::new();

    for child in children(class_body) {
        if child.kind() != "method_definition" {
            continue;
        }

        // Skip private/protected
        let mut is_private = false;
        for modifier in children(child) {
            let txt = node_text(modifier, source);
            if txt == "private" || txt == "protected" {
                is_private = true;
                break;
            }
        }
        if is_private {
            continue;
        }

        let method_name = child
            .child_by_field_name("name")
            .map(|n| node_text(n, source))
            .unwrap_or("");

        // Skip constructor – it's a lifecycle method, not domain behavior
        if method_name.is_empty() || method_name == "constructor" {
            continue;
        }

        let params = child
            .child_by_field_name("parameters")
            .map(|p| ts_parse_params(source, p))
            .unwrap_or_default();

        let return_type = child
            .child_by_field_name("return_type")
            .map(|rt| node_text(rt, source).to_string())
            // Strip leading ": " from type annotation
            .map(|s| s.trim_start_matches(':').trim().to_string())
            .unwrap_or_default();

        methods.push(DiscoveredMethod {
            owner: owner.to_string(),
            name: method_name.to_string(),
            parameters: params,
            return_type,
            file_path: file_path.to_string(),
            pre_conditions: vec![],
            post_conditions: vec![],
        });
    }

    methods
}

fn ts_parse_params(source: &str, params_node: Node) -> Vec<Field> {
    let mut fields = Vec::new();

    for param in children(params_node) {
        match param.kind() {
            "required_parameter" | "optional_parameter" => {
                let name = param
                    .child_by_field_name("pattern")
                    .map(|n| node_text(n, source))
                    .unwrap_or("");
                if name.is_empty() {
                    continue;
                }
                let ty = param
                    .child_by_field_name("type")
                    .map(|t| node_text(t, source).to_string())
                    // Strip leading ": "
                    .map(|s| s.trim_start_matches(':').trim().to_string())
                    .unwrap_or_default();

                let required = param.kind() == "required_parameter";
                fields.push(Field {
                    name: name.to_string(),
                    field_type: ty,
                    required,
                    description: String::new(),
                });
            }
            _ => {}
        }
    }

    fields
}

// ─── AstScanner implementation ─────────────────────────────────────────────

impl AstScanner for TreeSitterScanner {
    fn extract_live_dependencies(
        &self,
        file_path: &Path,
        source_code: &str,
    ) -> Result<Vec<LiveDependency>> {
        let (tree, family) = match self.parse_tree(file_path, source_code)? {
            Some(pair) => pair,
            None => return Ok(vec![]),
        };

        let from_file = file_path.to_string_lossy().to_string();

        let raw_imports = match family {
            LangFamily::Python => python_extract_imports(source_code, &tree),
            LangFamily::TypeScript => ts_extract_imports(source_code, &tree),
        };

        let deps = raw_imports
            .into_iter()
            .map(|to_module| LiveDependency {
                from_file: from_file.clone(),
                to_module,
            })
            .collect();

        Ok(deps)
    }

    fn scan_file(
        &self,
        file_path: &Path,
        source_code: &str,
    ) -> Result<(Vec<DiscoveredStruct>, Vec<DiscoveredMethod>)> {
        let (tree, family) = match self.parse_tree(file_path, source_code)? {
            Some(pair) => pair,
            None => return Ok((vec![], vec![])),
        };

        let fp = file_path.to_string_lossy().to_string();

        match family {
            LangFamily::Python => Ok(python_extract_classes(source_code, &tree, &fp)),
            LangFamily::TypeScript => Ok(ts_extract_classes(source_code, &tree, &fp)),
        }
    }
}
