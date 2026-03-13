use anyhow::{Context, Result};
use std::path::Path;
use syn::visit::Visit;
use syn::spanned::Spanned;

use super::analyze::{DiscoveredEnum, DiscoveredMethod, DiscoveredModule, DiscoveredStruct, LiveDependency, ScanResult};
use super::model::Field;
use super::scanner::AstScanner;

pub struct RustSynScanner;

impl AstScanner for RustSynScanner {
    fn extract_live_dependencies(
        &self,
        file_path: &Path,
        source_code: &str,
    ) -> Result<Vec<LiveDependency>> {
        let syntax_tree = syn::parse_file(source_code)
            .with_context(|| format!("Failed to parse rust file: {}", file_path.display()))?;

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
                            &format!(
                                "{}{}{}::",
                                prefix,
                                if prefix.is_empty() { "" } else { "::" },
                                path.ident
                            ),
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

    fn scan_file(
        &self,
        file_path: &Path,
        source_code: &str,
    ) -> Result<ScanResult> {
        let syntax_tree = syn::parse_file(source_code)
            .with_context(|| format!("Failed to parse: {}", file_path.display()))?;

        let mut visitor = StructMethodVisitor {
            structs: vec![],
            enums: vec![],
            methods: vec![],
            modules: vec![],
            file_path: file_path.to_string_lossy().to_string(),
        };
        visitor.visit_file(&syntax_tree);

        Ok((visitor.structs, visitor.enums, visitor.methods, visitor.modules))
    }
}

fn is_public(vis: &syn::Visibility) -> bool {
    matches!(vis, syn::Visibility::Public(_))
}

fn has_cfg_test(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        if !attr.path().is_ident("cfg") {
            return false;
        }
        attr.parse_args::<syn::Ident>()
            .map(|ident| ident == "test")
            .unwrap_or(false)
    })
}

fn type_to_string(ty: &syn::Type) -> String {
    let mut tokens = proc_macro2::TokenStream::new();
    quote::ToTokens::to_tokens(ty, &mut tokens);
    tokens.to_string().replace(' ', "")
}

fn is_option_type(ty: &syn::Type) -> bool {
    let type_str = type_to_string(ty);
    type_str.starts_with("Option<") || type_str.starts_with("std::option::Option<")
}

struct StructMethodVisitor {
    structs: Vec<DiscoveredStruct>,
    enums: Vec<DiscoveredEnum>,
    methods: Vec<DiscoveredMethod>,
    modules: Vec<DiscoveredModule>,
    file_path: String,
}

impl<'ast> Visit<'ast> for StructMethodVisitor {
    fn visit_item_struct(&mut self, node: &'ast syn::ItemStruct) {
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
            syn::Fields::Unnamed(_) => {
                tracing::warn!(
                    "Tuple struct {} encountered in {}, tuple structs are currently mapped as empty-field records",
                    name,
                    self.file_path
                );
                vec![]
            }
            syn::Fields::Unit => {
                tracing::warn!(
                    "Unit struct {} encountered in {}, mapping as zero-field record",
                    name,
                    self.file_path
                );
                vec![]
            }
        };

        self.structs.push(DiscoveredStruct {
            name,
            start_line: node.span().start().line,
            end_line: node.span().end().line,
            fields,
            file_path: self.file_path.clone(),
        });

        syn::visit::visit_item_struct(self, node);
    }

    fn visit_item_enum(&mut self, node: &'ast syn::ItemEnum) {
        if !is_public(&node.vis) || has_cfg_test(&node.attrs) {
            return;
        }

        let name = node.ident.to_string();
        let variants = node
            .variants
            .iter()
            .map(|v| {
                let variant_name = v.ident.to_string();
                let variant_type = match &v.fields {
                    syn::Fields::Unit => "()".to_string(),
                    syn::Fields::Unnamed(u) => {
                        let types: Vec<String> =
                            u.unnamed.iter().map(|f| type_to_string(&f.ty)).collect();
                        types.join(", ")
                    }
                    syn::Fields::Named(n) => {
                        let parts: Vec<String> = n
                            .named
                            .iter()
                            .filter_map(|f| {
                                let fname = f.ident.as_ref()?.to_string();
                                Some(format!("{}: {}", fname, type_to_string(&f.ty)))
                            })
                            .collect();
                        format!("{{ {} }}", parts.join(", "))
                    }
                };
                Field {
                    name: variant_name,
                    field_type: variant_type,
                    required: true,
                    description: String::new(),
                }
            })
            .collect();

        self.enums.push(DiscoveredEnum {
            name,
            start_line: node.span().start().line,
            end_line: node.span().end().line,
            variants,
            file_path: self.file_path.clone(),
        });

        syn::visit::visit_item_enum(self, node);
    }

    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        let name = node.ident.to_string();
        if name == "tests" || has_cfg_test(&node.attrs) {
            return;
        }
        self.modules.push(DiscoveredModule {
            name,
            public: is_public(&node.vis),
            file_path: self.file_path.clone(),
        });
        syn::visit::visit_item_mod(self, node);
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
                                _ => {
                                    tracing::warn!("Unrecognized param pattern in method {}::{}, skipping param", owner, name);
                                    return None;
                                }
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
                    start_line: method.span().start().line,
                    end_line: method.span().end().line,
                    parameters,
                    return_type,
                    file_path: self.file_path.clone(),
                });
            }
        }

        syn::visit::visit_item_impl(self, node);
    }
}
