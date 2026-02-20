pub mod file_analysis;
pub mod module_tree;
pub mod use_resolver;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use rayon::prelude::*;
use walkdir::WalkDir;

use file_analysis::FileAnalysis;
use module_tree::ModuleTree;

/// Pre-built index of a Rust crate's structure.
///
/// Built once by parsing all `.rs` files with `syn`, then queried by constraint checkers.
pub struct CrateIndex {
    pub module_tree: ModuleTree,
    pub files: HashMap<PathBuf, FileAnalysis>,
    /// Entity name -> list of (file, line) where it appears as a type/call reference
    pub entity_refs: HashMap<String, Vec<(PathBuf, usize)>>,
    /// (trait_name, self_type) -> list of files containing `impl Trait for Type`
    pub trait_impls: HashMap<(String, String), Vec<PathBuf>>,
    /// Root directory of the codebase being indexed
    pub codebase_root: PathBuf,
}

impl CrateIndex {
    /// Build a complete index of the crate at `codebase_root`.
    ///
    /// 1. Build the module tree from `lib.rs`/`main.rs`
    /// 2. Parse every `.rs` file with `syn` (in parallel)
    /// 3. Build entity reference and trait impl indexes
    pub fn build(codebase_root: &Path) -> Result<Self> {
        // Force proc_macro2 to use fallback span locations for accurate line numbers
        proc_macro2::fallback::force();

        let module_tree = ModuleTree::build(codebase_root)?;

        // Collect all .rs file paths first
        let entries: Vec<PathBuf> = WalkDir::new(codebase_root)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "rs"))
            .map(|e| e.path().to_path_buf())
            .collect();

        // Parse files in parallel
        let results: Vec<_> = entries
            .par_iter()
            .filter_map(|path| {
                let module_path = module_tree
                    .module_of_file(path)
                    .cloned()
                    .unwrap_or_default();
                let analysis = file_analysis::analyze_file(path, module_path)?;
                Some((path.clone(), analysis))
            })
            .collect();

        // Build indexes from results
        let mut files = HashMap::new();
        let mut entity_refs: HashMap<String, Vec<(PathBuf, usize)>> = HashMap::new();
        let mut trait_impls: HashMap<(String, String), Vec<PathBuf>> = HashMap::new();

        for (path, analysis) in results {
            // Build entity reference index
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

            files.insert(path, analysis);
        }

        Ok(CrateIndex {
            module_tree,
            files,
            entity_refs,
            trait_impls,
            codebase_root: codebase_root.to_path_buf(),
        })
    }

    /// Check if a file is under one of the given module/directory names.
    ///
    /// Tries module tree lookup first, falls back to directory-based matching.
    pub fn file_is_in_modules(&self, file: &Path, modules: &[String]) -> bool {
        for module in modules {
            if self.module_tree.file_is_under_module(file, module) {
                return true;
            }
            // Fallback: directory-based check for top-level modules
            if self
                .module_tree
                .file_is_under_directory(file, &self.codebase_root, module)
            {
                return true;
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
        assert!(!index.files.is_empty());
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
