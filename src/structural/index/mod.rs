pub mod file_analysis;
pub mod module_tree;
pub mod ts_analysis;
pub mod use_resolver;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use rayon::prelude::*;
use walkdir::WalkDir;

use file_analysis::FileAnalysis;
use module_tree::ModuleTree;
use ts_analysis::TsFileAnalysis;
use crate::structural::Language;

/// Pre-built index of a codebase's structure (multi-language support).
///
/// Built once by parsing Rust, TypeScript, and JavaScript files, then queried by constraint checkers.
pub struct CrateIndex {
    pub module_tree: Option<ModuleTree>,
    pub rust_files: HashMap<PathBuf, FileAnalysis>,
    pub ts_files: HashMap<PathBuf, TsFileAnalysis>,
    /// Entity name -> list of (file, line) where it appears as a type/call reference
    pub entity_refs: HashMap<String, Vec<(PathBuf, usize)>>,
    /// (trait_name, self_type) -> list of files containing `impl Trait for Type`
    pub trait_impls: HashMap<(String, String), Vec<PathBuf>>,
    /// Root directory of the codebase being indexed
    pub codebase_root: PathBuf,
}

impl CrateIndex {
    /// Build a complete index of the codebase at `codebase_root`.
    ///
    /// 1. Build the module tree from `lib.rs`/`main.rs` (for Rust)
    /// 2. Parse every source file (Rust, TypeScript, JavaScript) in parallel
    /// 3. Build entity reference and trait impl indexes
    pub fn build(codebase_root: &Path) -> Result<Self> {
        // Force proc_macro2 to use fallback span locations for accurate line numbers
        proc_macro2::fallback::force();

        // Build module tree if there's a Rust entry point (lib.rs or main.rs)
        // For TypeScript-only codebases, module_tree will be None
        let module_tree = ModuleTree::build(codebase_root).ok();

        // Collect all source file paths with their languages
        let entries: Vec<(PathBuf, Language)> = WalkDir::new(codebase_root)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let path = e.path();
                let ext = path.extension()?.to_str()?;
                let lang = Language::from_extension(ext);
                match lang {
                    Language::Rust | Language::TypeScript | Language::JavaScript => {
                        Some((path.to_path_buf(), lang))
                    }
                    Language::Unsupported => None,
                }
            })
            .collect();

        // Define an enum to hold either type of analysis result
        enum AnalysisResult {
            Rust(PathBuf, FileAnalysis),
            TypeScript(PathBuf, TsFileAnalysis),
        }

        // Parse files in parallel
        let results: Vec<AnalysisResult> = entries
            .par_iter()
            .filter_map(|(path, lang)| {
                match lang {
                    Language::Rust => {
                        let module_path = module_tree
                            .as_ref()
                            .and_then(|mt| mt.module_of_file(path))
                            .cloned()
                            .unwrap_or_default();
                        let analysis = file_analysis::analyze_file(path, module_path)?;
                        Some(AnalysisResult::Rust(path.clone(), analysis))
                    }
                    Language::TypeScript | Language::JavaScript => {
                        let analysis = ts_analysis::analyze_file(path)?;
                        Some(AnalysisResult::TypeScript(path.clone(), analysis))
                    }
                    Language::Unsupported => None,
                }
            })
            .collect();

        // Build indexes from results
        let mut rust_files = HashMap::new();
        let mut ts_files = HashMap::new();
        let mut entity_refs: HashMap<String, Vec<(PathBuf, usize)>> = HashMap::new();
        let mut trait_impls: HashMap<(String, String), Vec<PathBuf>> = HashMap::new();

        for result in results {
            match result {
                AnalysisResult::Rust(path, analysis) => {
                    // Build entity reference index from Rust files
                    for type_ref in &analysis.type_refs {
                        entity_refs
                            .entry(type_ref.name.clone())
                            .or_default()
                            .push((path.clone(), type_ref.line));
                    }
                    for call_ref in &analysis.call_refs {
                        entity_refs
                            .entry(call_ref.receiver.clone())
                            .or_default()
                            .push((path.clone(), call_ref.line));
                    }

                    // Build trait impl index
                    for ti in &analysis.trait_impls {
                        trait_impls
                            .entry((ti.trait_name.clone(), ti.self_type.clone()))
                            .or_default()
                            .push(path.clone());
                    }

                    rust_files.insert(path, analysis);
                }
                AnalysisResult::TypeScript(path, analysis) => {
                    // Build entity reference index from TypeScript files
                    for import in &analysis.imports {
                        for name in &import.names {
                            entity_refs
                                .entry(name.clone())
                                .or_default()
                                .push((path.clone(), import.line));
                        }
                    }

                    // Build trait impl index from class implements
                    for class in &analysis.classes {
                        for interface in &class.implements {
                            trait_impls
                                .entry((interface.clone(), class.name.clone()))
                                .or_default()
                                .push(path.clone());
                        }
                    }

                    ts_files.insert(path, analysis);
                }
            }
        }

        Ok(CrateIndex {
            module_tree,
            rust_files,
            ts_files,
            entity_refs,
            trait_impls,
            codebase_root: codebase_root.to_path_buf(),
        })
    }

    /// Check if a file is under one of the given module/directory names.
    ///
    /// Tries module tree lookup first (if available), falls back to directory-based matching.
    pub fn file_is_in_modules(&self, file: &Path, modules: &[String]) -> bool {
        for module in modules {
            // If we have a module tree (Rust codebase), use it
            if let Some(ref mt) = self.module_tree {
                if mt.file_is_under_module(file, module) {
                    return true;
                }
                // Fallback: directory-based check for top-level modules
                if mt.file_is_under_directory(file, &self.codebase_root, module) {
                    return true;
                }
            } else {
                // For non-Rust codebases, use simple directory matching
                if file.starts_with(&self.codebase_root.join(module)) {
                    return true;
                }
            }
        }
        false
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn setup_crate() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();

        std::fs::write(
            tmp.path().join("lib.rs"),
            r#"
mod storage;
mod services;
"#,
        )
        .unwrap();

        std::fs::create_dir(tmp.path().join("storage")).unwrap();
        std::fs::write(
            tmp.path().join("storage/mod.rs"),
            r#"
pub struct DgraphClient;
pub struct MilvusClient;

impl GraphStore for DgraphClient {
    fn query(&self) {}
}
"#,
        )
        .unwrap();

        std::fs::write(
            tmp.path().join("services.rs"),
            r#"
use crate::storage::DgraphClient;

pub fn init() {
    let _c = DgraphClient::new();
}
"#,
        )
        .unwrap();

        tmp
    }

    #[test]
    fn builds_index_successfully() {
        let tmp = setup_crate();
        let index = CrateIndex::build(tmp.path()).unwrap();
        assert!(!index.rust_files.is_empty());
    }

    #[test]
    fn indexes_entity_refs() {
        let tmp = setup_crate();
        let index = CrateIndex::build(tmp.path()).unwrap();
        assert!(index.entity_refs.contains_key("DgraphClient"));
    }

    #[test]
    fn indexes_trait_impls() {
        let tmp = setup_crate();
        let index = CrateIndex::build(tmp.path()).unwrap();
        assert!(index
            .trait_impls
            .contains_key(&("GraphStore".into(), "DgraphClient".into())));
    }

    #[test]
    fn file_is_in_modules_works() {
        let tmp = setup_crate();
        let index = CrateIndex::build(tmp.path()).unwrap();

        let storage_file = tmp.path().join("storage/mod.rs");
        assert!(index.file_is_in_modules(&storage_file, &["storage".into()]));
        assert!(!index.file_is_in_modules(&storage_file, &["services".into()]));
    }
}
