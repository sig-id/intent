use std::path::{Path, PathBuf};

use syn::visit::Visit;
use syn::{Expr, ExprPath, Item, ItemImpl, ItemMod, ItemUse, Type, TypePath, UseTree};

/// Analysis results extracted from a single `.rs` file via `syn` AST parsing.
#[derive(Debug, Clone)]
pub struct FileAnalysis {
    pub path: PathBuf,
    pub module_path: Vec<String>,
    pub imports: Vec<UseImport>,
    pub type_refs: Vec<TypeRef>,
    pub call_refs: Vec<CallRef>,
    pub trait_impls: Vec<TraitImpl>,
    pub mod_decls: Vec<ModDecl>,
}

#[derive(Debug, Clone)]
pub struct UseImport {
    pub segments: Vec<String>,
    pub alias: Option<String>,
    pub is_glob: bool,
    pub line: usize,
}

#[derive(Debug, Clone)]
pub struct TypeRef {
    pub name: String,
    pub qualified: Option<Vec<String>>,
    pub line: usize,
}

#[derive(Debug, Clone)]
pub struct CallRef {
    pub receiver: String,
    pub method: String,
    pub line: usize,
}

#[derive(Debug, Clone)]
pub struct TraitImpl {
    pub trait_name: String,
    pub self_type: String,
    pub line: usize,
}

#[derive(Debug, Clone)]
pub struct ModDecl {
    pub name: String,
    pub is_inline: bool,
    pub is_cfg_test: bool,
    pub line: usize,
}

/// Parse a `.rs` file and extract structural information.
///
/// Returns `None` if `syn::parse_file` fails (e.g. macro-heavy code).
pub fn analyze_file(path: &Path, module_path: Vec<String>) -> Option<FileAnalysis> {
    let source = std::fs::read_to_string(path).ok()?;
    analyze_source(&source, path, module_path)
}

/// Parse source text and extract structural information.
pub fn analyze_source(source: &str, path: &Path, module_path: Vec<String>) -> Option<FileAnalysis> {
    let file = syn::parse_file(source).ok()?;

    let mut visitor = AstVisitor::default();
    visitor.visit_file(&file);

    Some(FileAnalysis {
        path: path.to_path_buf(),
        module_path,
        imports: visitor.imports,
        type_refs: visitor.type_refs,
        call_refs: visitor.call_refs,
        trait_impls: visitor.trait_impls,
        mod_decls: visitor.mod_decls,
    })
}

#[derive(Default)]
struct AstVisitor {
    imports: Vec<UseImport>,
    type_refs: Vec<TypeRef>,
    call_refs: Vec<CallRef>,
    trait_impls: Vec<TraitImpl>,
    mod_decls: Vec<ModDecl>,
    in_cfg_test: bool,
}

impl AstVisitor {
    fn span_line(span: proc_macro2::Span) -> usize {
        span.start().line
    }

    fn collect_use_tree(&mut self, tree: &UseTree, prefix: &[String], line: usize) {
        match tree {
            UseTree::Path(p) => {
                let mut segments = prefix.to_vec();
                segments.push(p.ident.to_string());
                self.collect_use_tree(&p.tree, &segments, line);
            }
            UseTree::Name(n) => {
                let mut segments = prefix.to_vec();
                segments.push(n.ident.to_string());
                self.imports.push(UseImport {
                    segments,
                    alias: None,
                    is_glob: false,
                    line,
                });
            }
            UseTree::Rename(r) => {
                let mut segments = prefix.to_vec();
                segments.push(r.ident.to_string());
                self.imports.push(UseImport {
                    segments,
                    alias: Some(r.rename.to_string()),
                    is_glob: false,
                    line,
                });
            }
            UseTree::Glob(_) => {
                self.imports.push(UseImport {
                    segments: prefix.to_vec(),
                    alias: None,
                    is_glob: true,
                    line,
                });
            }
            UseTree::Group(g) => {
                for tree in &g.items {
                    self.collect_use_tree(tree, prefix, line);
                }
            }
        }
    }

    fn is_pascal_case(s: &str) -> bool {
        s.len() > 1
            && s.starts_with(|c: char| c.is_ascii_uppercase())
            && s.contains(|c: char| c.is_ascii_lowercase())
    }

    fn extract_type_name(ty: &Type) -> Option<String> {
        if let Type::Path(tp) = ty {
            tp.path.segments.last().map(|s| s.ident.to_string())
        } else {
            None
        }
    }
}

impl<'ast> Visit<'ast> for AstVisitor {
    fn visit_item_use(&mut self, node: &'ast ItemUse) {
        if self.in_cfg_test {
            return;
        }
        let line = Self::span_line(node.use_token.span);
        self.collect_use_tree(&node.tree, &[], line);
    }

    fn visit_item_impl(&mut self, node: &'ast ItemImpl) {
        if self.in_cfg_test {
            return;
        }
        if let Some((_, ref trait_path, _)) = node.trait_ {
            let trait_name = trait_path
                .segments
                .last()
                .map(|s| s.ident.to_string())
                .unwrap_or_default();
            let self_type = Self::extract_type_name(&node.self_ty).unwrap_or_default();
            if !trait_name.is_empty() && !self_type.is_empty() {
                self.trait_impls.push(TraitImpl {
                    trait_name,
                    self_type,
                    line: Self::span_line(node.impl_token.span),
                });
            }
        }
        // Continue visiting children for type refs inside impl blocks
        syn::visit::visit_item_impl(self, node);
    }

    fn visit_item_mod(&mut self, node: &'ast ItemMod) {
        let is_cfg_test = node.attrs.iter().any(|a| {
            if a.path().is_ident("cfg") {
                a.parse_args::<syn::Ident>()
                    .map(|id| id == "test")
                    .unwrap_or(false)
            } else {
                false
            }
        });

        self.mod_decls.push(ModDecl {
            name: node.ident.to_string(),
            is_inline: node.content.is_some(),
            is_cfg_test,
            line: Self::span_line(node.mod_token.span),
        });

        if is_cfg_test {
            // Skip children of #[cfg(test)] modules entirely
            return;
        }

        // Visit children of inline modules
        if let Some((_, ref items)) = node.content {
            let prev = self.in_cfg_test;
            for item in items {
                self.visit_item(item);
            }
            self.in_cfg_test = prev;
        }
    }

    fn visit_type_path(&mut self, node: &'ast TypePath) {
        if self.in_cfg_test {
            syn::visit::visit_type_path(self, node);
            return;
        }
        if let Some(last) = node.path.segments.last() {
            let name = last.ident.to_string();
            if Self::is_pascal_case(&name) {
                let qualified = if node.path.segments.len() > 1 {
                    Some(
                        node.path
                            .segments
                            .iter()
                            .map(|s| s.ident.to_string())
                            .collect(),
                    )
                } else {
                    None
                };
                self.type_refs.push(TypeRef {
                    name,
                    qualified,
                    line: Self::span_line(last.ident.span()),
                });
            }
        }
        syn::visit::visit_type_path(self, node);
    }

    fn visit_expr_path(&mut self, node: &'ast ExprPath) {
        if self.in_cfg_test {
            syn::visit::visit_expr_path(self, node);
            return;
        }
        // Detect Type::method() patterns and PascalCase references
        let segments = &node.path.segments;
        if segments.len() >= 2 {
            let first = segments[0].ident.to_string();
            if Self::is_pascal_case(&first) {
                let method = segments.last().map(|s| s.ident.to_string()).unwrap_or_default();
                self.call_refs.push(CallRef {
                    receiver: first.clone(),
                    method,
                    line: Self::span_line(segments[0].ident.span()),
                });
                // Also record as a type ref
                self.type_refs.push(TypeRef {
                    name: first,
                    qualified: Some(
                        segments
                            .iter()
                            .map(|s| s.ident.to_string())
                            .collect(),
                    ),
                    line: Self::span_line(segments[0].ident.span()),
                });
            }
        } else if segments.len() == 1 {
            let name = segments[0].ident.to_string();
            if Self::is_pascal_case(&name) {
                self.type_refs.push(TypeRef {
                    name,
                    qualified: None,
                    line: Self::span_line(segments[0].ident.span()),
                });
            }
        }
        syn::visit::visit_expr_path(self, node);
    }

    fn visit_item(&mut self, node: &'ast Item) {
        if self.in_cfg_test {
            return;
        }
        syn::visit::visit_item(self, node);
    }

    fn visit_expr(&mut self, node: &'ast Expr) {
        if self.in_cfg_test {
            return;
        }
        syn::visit::visit_expr(self, node);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn analyze(source: &str) -> FileAnalysis {
        analyze_source(source, &PathBuf::from("test.rs"), vec![])
            .expect("parse should succeed")
    }

    #[test]
    fn extracts_use_imports() {
        let fa = analyze("use crate::storage::DgraphClient;");
        assert_eq!(fa.imports.len(), 1);
        assert_eq!(
            fa.imports[0].segments,
            vec!["crate", "storage", "DgraphClient"]
        );
        assert!(!fa.imports[0].is_glob);
    }

    #[test]
    fn extracts_grouped_use_imports() {
        let fa = analyze("use crate::storage::{DgraphClient, MilvusClient};");
        assert_eq!(fa.imports.len(), 2);
        assert_eq!(
            fa.imports[0].segments,
            vec!["crate", "storage", "DgraphClient"]
        );
        assert_eq!(
            fa.imports[1].segments,
            vec!["crate", "storage", "MilvusClient"]
        );
    }

    #[test]
    fn extracts_glob_import() {
        let fa = analyze("use crate::storage::*;");
        assert_eq!(fa.imports.len(), 1);
        assert!(fa.imports[0].is_glob);
        assert_eq!(fa.imports[0].segments, vec!["crate", "storage"]);
    }

    #[test]
    fn ignores_comments_and_strings() {
        let source = r#"
// DgraphClient in a comment
/* DgraphClient in block comment */
fn foo() {
    let _s = "DgraphClient in a string";
}
"#;
        let fa = analyze(source);
        // syn naturally excludes comments and string contents from the AST
        assert!(
            fa.type_refs
                .iter()
                .all(|r| r.name != "DgraphClient"),
            "should not find DgraphClient in comments/strings"
        );
    }

    #[test]
    fn skips_cfg_test_blocks() {
        let source = r#"
fn real_code() -> DgraphClient { todo!() }
#[cfg(test)]
mod tests {
    fn test_thing() -> DgraphClient { todo!() }
}
"#;
        let fa = analyze(source);
        // Should only find DgraphClient from real_code, not from tests
        let refs: Vec<_> = fa
            .type_refs
            .iter()
            .filter(|r| r.name == "DgraphClient")
            .collect();
        assert_eq!(refs.len(), 1, "should find exactly 1 DgraphClient ref");
    }

    #[test]
    fn extracts_trait_impls() {
        let source = r#"
impl GraphStore for DgraphClient {
    fn query(&self) {}
}
"#;
        let fa = analyze(source);
        assert_eq!(fa.trait_impls.len(), 1);
        assert_eq!(fa.trait_impls[0].trait_name, "GraphStore");
        assert_eq!(fa.trait_impls[0].self_type, "DgraphClient");
    }

    #[test]
    fn extracts_qualified_type_refs() {
        let source = "fn foo(c: crate::storage::DgraphClient) {}";
        let fa = analyze(source);
        let refs: Vec<_> = fa
            .type_refs
            .iter()
            .filter(|r| r.name == "DgraphClient")
            .collect();
        assert!(!refs.is_empty());
        assert!(refs[0].qualified.is_some());
    }

    #[test]
    fn extracts_call_refs() {
        let source = "fn foo() { DgraphClient::new(); }";
        let fa = analyze(source);
        let calls: Vec<_> = fa
            .call_refs
            .iter()
            .filter(|c| c.receiver == "DgraphClient")
            .collect();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].method, "new");
    }

    #[test]
    fn extracts_mod_decls() {
        let source = r#"
mod storage;
pub mod services;
#[cfg(test)]
mod tests { }
"#;
        let fa = analyze(source);
        assert_eq!(fa.mod_decls.len(), 3);
        assert_eq!(fa.mod_decls[0].name, "storage");
        assert!(!fa.mod_decls[0].is_inline);
        assert!(!fa.mod_decls[0].is_cfg_test);
        assert_eq!(fa.mod_decls[1].name, "services");
        assert_eq!(fa.mod_decls[2].name, "tests");
        assert!(fa.mod_decls[2].is_cfg_test);
    }

    #[test]
    fn handles_use_alias() {
        let fa = analyze("use crate::storage::DgraphClient as Dg;");
        assert_eq!(fa.imports.len(), 1);
        assert_eq!(fa.imports[0].alias.as_deref(), Some("Dg"));
    }
}
