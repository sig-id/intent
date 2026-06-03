//! Language connector trait for extensible structural analysis.
//!
//! This trait provides an abstraction layer so that structural analysis
//! can work with any language, not just Rust. Each language implements
//! this trait to provide dependency resolution, reference checking,
//! and trait/interface implementation checking.

use std::path::{Path, PathBuf};

use anyhow::Result;

/// A language-specific connector for structural analysis.
pub trait LanguageConnector: Send + Sync {
    /// The language this connector handles.
    fn language(&self) -> &str;

    /// File extensions this connector handles.
    fn extensions(&self) -> &[&str];

    /// Analyze a single file and return its analysis result.
    fn analyze_file(&self, path: &Path) -> Result<FileAnalysisResult>;

    /// Check if module `from` depends on (imports from) module `to`.
    fn check_dependency(&self, from: &str, to: &str, analysis: &[FileAnalysisResult]) -> bool;

    /// Check if module `from` references type/entity `entity`.
    fn check_reference(&self, from: &str, entity: &str, analysis: &[FileAnalysisResult]) -> bool;

    /// Check if type `type_name` implements trait/interface `trait_name`.
    fn check_implements(
        &self,
        type_name: &str,
        trait_name: &str,
        analysis: &[FileAnalysisResult],
    ) -> bool;
}

/// Language-agnostic file analysis result.
#[derive(Debug, Clone)]
pub struct FileAnalysisResult {
    /// The module path (e.g., "services::auth" for Rust, "services/auth" for TS)
    pub module_path: String,
    /// Import/dependency statements
    pub imports: Vec<ImportInfo>,
    /// Type/entity references
    pub type_references: Vec<String>,
    /// Trait/interface implementations
    pub implementations: Vec<ImplementationInfo>,
    /// File path
    pub file_path: PathBuf,
    /// Line count for reference
    pub line_count: usize,
}

/// An import/dependency in a file.
#[derive(Debug, Clone)]
pub struct ImportInfo {
    /// The module being imported from
    pub from_module: String,
    /// The specific entities imported
    pub entities: Vec<String>,
    /// Line number
    pub line: usize,
}

/// A trait/interface implementation.
#[derive(Debug, Clone)]
pub struct ImplementationInfo {
    /// The type implementing the trait/interface
    pub type_name: String,
    /// The trait/interface being implemented
    pub trait_name: String,
}

/// Registry of language connectors.
pub struct ConnectorRegistry {
    connectors: Vec<Box<dyn LanguageConnector>>,
}

impl ConnectorRegistry {
    pub fn new() -> Self {
        Self {
            connectors: Vec::new(),
        }
    }

    /// Create a registry with default connectors (Rust, TypeScript).
    pub fn with_defaults() -> Self {
        let mut registry = Self::new();
        registry.register(Box::new(RustConnector));
        registry.register(Box::new(TypeScriptConnector));
        registry
    }

    pub fn register(&mut self, connector: Box<dyn LanguageConnector>) {
        self.connectors.push(connector);
    }

    /// Get the connector for a file based on its extension.
    pub fn connector_for_file(&self, path: &Path) -> Option<&dyn LanguageConnector> {
        let ext = path.extension()?.to_str()?;
        self.connectors
            .iter()
            .find(|c| c.extensions().contains(&ext))
            .map(|c| c.as_ref())
    }

    /// Get all registered connectors.
    pub fn connectors(&self) -> &[Box<dyn LanguageConnector>] {
        &self.connectors
    }
}

impl Default for ConnectorRegistry {
    fn default() -> Self {
        Self::with_defaults()
    }
}

// ---------------------------------------------------------------------------
// Built-in Rust language connector
// ---------------------------------------------------------------------------

/// Built-in Rust language connector.
///
/// Delegates to `file_analysis::analyze_file` (which uses `syn` under the hood)
/// and maps the Rust-specific structs (`UseImport`, `TypeRef`, `TraitImpl`) into
/// the language-agnostic `FileAnalysisResult`.
pub struct RustConnector;

impl LanguageConnector for RustConnector {
    fn language(&self) -> &str {
        "rust"
    }

    fn extensions(&self) -> &[&str] {
        &["rs"]
    }

    fn analyze_file(&self, path: &Path) -> Result<FileAnalysisResult> {
        use crate::structural::index::file_analysis;

        let source = std::fs::read_to_string(path)?;
        let analysis = file_analysis::analyze_source(&source, path, vec![])
            .ok_or_else(|| anyhow::anyhow!("failed to parse Rust file: {}", path.display()))?;

        let module_path = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        Ok(FileAnalysisResult {
            module_path,
            imports: analysis
                .imports
                .iter()
                .map(|u| ImportInfo {
                    from_module: u.segments.join("::"),
                    entities: vec![u.segments.last().cloned().unwrap_or_default()],
                    line: u.line,
                })
                .collect(),
            type_references: analysis.type_refs.iter().map(|r| r.name.clone()).collect(),
            implementations: analysis
                .trait_impls
                .iter()
                .map(|i| ImplementationInfo {
                    type_name: i.self_type.clone(),
                    trait_name: i.trait_name.clone(),
                })
                .collect(),
            file_path: path.to_path_buf(),
            line_count: source.lines().count(),
        })
    }

    fn check_dependency(&self, from: &str, to: &str, analysis: &[FileAnalysisResult]) -> bool {
        analysis
            .iter()
            .filter(|a| a.module_path == from || a.module_path.starts_with(&format!("{}::", from)))
            .any(|a| {
                a.imports
                    .iter()
                    .any(|i| i.from_module.contains(to) || i.entities.iter().any(|e| e == to))
            })
    }

    fn check_reference(&self, from: &str, entity: &str, analysis: &[FileAnalysisResult]) -> bool {
        analysis
            .iter()
            .filter(|a| a.module_path == from || a.module_path.starts_with(&format!("{}::", from)))
            .any(|a| a.type_references.iter().any(|r| r == entity))
    }

    fn check_implements(
        &self,
        type_name: &str,
        trait_name: &str,
        analysis: &[FileAnalysisResult],
    ) -> bool {
        analysis.iter().any(|a| {
            a.implementations
                .iter()
                .any(|i| i.type_name == type_name && i.trait_name == trait_name)
        })
    }
}

// ---------------------------------------------------------------------------
// Built-in TypeScript language connector
// ---------------------------------------------------------------------------

/// Built-in TypeScript language connector.
///
/// Delegates to `ts_analysis::analyze_source` (regex-based parser) and maps the
/// TypeScript-specific structs (`TsImport`, `ClassDecl`) into the
/// language-agnostic `FileAnalysisResult`.
pub struct TypeScriptConnector;

impl LanguageConnector for TypeScriptConnector {
    fn language(&self) -> &str {
        "typescript"
    }

    fn extensions(&self) -> &[&str] {
        &["ts", "tsx", "js", "jsx"]
    }

    fn analyze_file(&self, path: &Path) -> Result<FileAnalysisResult> {
        use crate::structural::index::ts_analysis;

        let source = std::fs::read_to_string(path)?;
        let analysis = ts_analysis::analyze_source(&source, path);

        let module_path = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        Ok(FileAnalysisResult {
            module_path,
            imports: analysis
                .imports
                .iter()
                .map(|i| ImportInfo {
                    from_module: i.source.clone(),
                    entities: i.names.clone(),
                    line: i.line,
                })
                .collect(),
            type_references: analysis
                .imports
                .iter()
                .flat_map(|i| i.names.clone())
                .collect(),
            implementations: analysis
                .classes
                .iter()
                .flat_map(|c| {
                    c.implements.iter().map(move |iface| ImplementationInfo {
                        type_name: c.name.clone(),
                        trait_name: iface.clone(),
                    })
                })
                .collect(),
            file_path: path.to_path_buf(),
            line_count: source.lines().count(),
        })
    }

    fn check_dependency(&self, from: &str, to: &str, analysis: &[FileAnalysisResult]) -> bool {
        analysis.iter().filter(|a| a.module_path == from).any(|a| {
            a.imports
                .iter()
                .any(|i| i.from_module.contains(to) || i.entities.iter().any(|e| e == to))
        })
    }

    fn check_reference(&self, from: &str, entity: &str, analysis: &[FileAnalysisResult]) -> bool {
        analysis
            .iter()
            .filter(|a| a.module_path == from)
            .any(|a| a.type_references.iter().any(|r| r == entity))
    }

    fn check_implements(
        &self,
        type_name: &str,
        trait_name: &str,
        analysis: &[FileAnalysisResult],
    ) -> bool {
        analysis.iter().any(|a| {
            a.implementations
                .iter()
                .any(|i| i.type_name == type_name && i.trait_name == trait_name)
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn registry_default_has_connectors() {
        let registry = ConnectorRegistry::with_defaults();
        assert_eq!(registry.connectors().len(), 2);
        assert_eq!(registry.connectors()[0].language(), "rust");
        assert_eq!(registry.connectors()[1].language(), "typescript");
    }

    #[test]
    fn registry_finds_connector_by_extension() {
        let registry = ConnectorRegistry::with_defaults();

        let rs = registry.connector_for_file(Path::new("foo.rs"));
        assert!(rs.is_some());
        assert_eq!(rs.unwrap().language(), "rust");

        let ts = registry.connector_for_file(Path::new("bar.ts"));
        assert!(ts.is_some());
        assert_eq!(ts.unwrap().language(), "typescript");

        let tsx = registry.connector_for_file(Path::new("baz.tsx"));
        assert!(tsx.is_some());
        assert_eq!(tsx.unwrap().language(), "typescript");

        let js = registry.connector_for_file(Path::new("qux.js"));
        assert!(js.is_some());
        assert_eq!(js.unwrap().language(), "typescript");

        let py = registry.connector_for_file(Path::new("script.py"));
        assert!(py.is_none());
    }

    #[test]
    fn rust_connector_analyzes_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("test.rs");
        std::fs::write(
            &path,
            r#"
use crate::storage::DgraphClient;

impl GraphStore for DgraphClient {
    fn query(&self) {}
}

fn foo() -> DgraphClient { todo!() }
"#,
        )
        .unwrap();

        let connector = RustConnector;
        let result = connector.analyze_file(&path).unwrap();

        assert_eq!(result.module_path, "test");
        assert!(!result.imports.is_empty());
        assert!(result.type_references.iter().any(|r| r == "DgraphClient"));
        assert!(result
            .implementations
            .iter()
            .any(|i| i.type_name == "DgraphClient" && i.trait_name == "GraphStore"));
    }

    #[test]
    fn ts_connector_analyzes_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("test.ts");
        std::fs::write(
            &path,
            r#"
import { Database } from './db';
import { Connection } from './conn';

class UserService implements IService {
    constructor() {}
}
"#,
        )
        .unwrap();

        let connector = TypeScriptConnector;
        let result = connector.analyze_file(&path).unwrap();

        assert_eq!(result.module_path, "test");
        assert_eq!(result.imports.len(), 2);
        assert_eq!(result.imports[0].from_module, "./db");
        assert_eq!(result.imports[0].entities, vec!["Database"]);
        assert!(result
            .implementations
            .iter()
            .any(|i| i.type_name == "UserService" && i.trait_name == "IService"));
    }

    #[test]
    fn check_dependency_works() {
        let analysis = vec![FileAnalysisResult {
            module_path: "services".into(),
            imports: vec![ImportInfo {
                from_module: "crate::storage::DgraphClient".into(),
                entities: vec!["DgraphClient".into()],
                line: 1,
            }],
            type_references: vec!["DgraphClient".into()],
            implementations: vec![],
            file_path: PathBuf::from("services.rs"),
            line_count: 10,
        }];

        let connector = RustConnector;
        assert!(connector.check_dependency("services", "storage", &analysis));
        assert!(connector.check_dependency("services", "DgraphClient", &analysis));
        assert!(!connector.check_dependency("services", "unknown", &analysis));
    }

    #[test]
    fn check_reference_works() {
        let analysis = vec![FileAnalysisResult {
            module_path: "services".into(),
            imports: vec![],
            type_references: vec!["DgraphClient".into(), "AppError".into()],
            implementations: vec![],
            file_path: PathBuf::from("services.rs"),
            line_count: 10,
        }];

        let connector = RustConnector;
        assert!(connector.check_reference("services", "DgraphClient", &analysis));
        assert!(connector.check_reference("services", "AppError", &analysis));
        assert!(!connector.check_reference("services", "Unknown", &analysis));
    }

    #[test]
    fn check_implements_works() {
        let analysis = vec![FileAnalysisResult {
            module_path: "storage".into(),
            imports: vec![],
            type_references: vec![],
            implementations: vec![ImplementationInfo {
                type_name: "DgraphClient".into(),
                trait_name: "GraphStore".into(),
            }],
            file_path: PathBuf::from("storage.rs"),
            line_count: 10,
        }];

        let connector = RustConnector;
        assert!(connector.check_implements("DgraphClient", "GraphStore", &analysis));
        assert!(!connector.check_implements("DgraphClient", "VectorStore", &analysis));
    }
}
