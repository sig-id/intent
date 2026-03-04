use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Hierarchical module tree built by parsing `mod` declarations from the crate root.
#[derive(Debug)]
pub struct ModuleTree {
    root: ModuleNode,
    /// Module path (e.g. "storage::dgraph") -> list of files in that module subtree
    path_to_files: HashMap<String, Vec<PathBuf>>,
    /// File path -> module path components (e.g. ["storage", "dgraph"])
    file_to_module: HashMap<PathBuf, Vec<String>>,
}

#[derive(Debug, Default)]
struct ModuleNode {
    #[allow(dead_code)]
    name: String,
    children: HashMap<String, ModuleNode>,
    file: Option<PathBuf>,
}

impl ModuleTree {
    /// Build the module tree starting from the crate root directory.
    ///
    /// Looks for `lib.rs` (library crate) or `main.rs` (binary crate) as the entry point,
    /// then recursively resolves `mod` declarations.
    pub fn build(crate_root: &Path) -> Result<Self> {
        let entry = if crate_root.join("lib.rs").exists() {
            crate_root.join("lib.rs")
        } else if crate_root.join("main.rs").exists() {
            crate_root.join("main.rs")
        } else {
            anyhow::bail!(
                "no lib.rs or main.rs found in {}",
                crate_root.display()
            );
        };

        let mut root = ModuleNode {
            name: String::new(),
            children: HashMap::new(),
            file: Some(entry.clone()),
        };

        Self::discover_children(&entry, crate_root, &mut root)?;

        let mut tree = ModuleTree {
            root,
            path_to_files: HashMap::new(),
            file_to_module: HashMap::new(),
        };
        tree.build_lookups(crate_root);
        Ok(tree)
    }

    /// Discover child modules by parsing `mod` declarations in a file.
    fn discover_children(
        file: &Path,
        crate_root: &Path,
        node: &mut ModuleNode,
    ) -> Result<()> {
        let source = std::fs::read_to_string(file)
            .with_context(|| format!("reading {}", file.display()))?;

        let parsed = match syn::parse_file(&source) {
            Ok(f) => f,
            Err(_) => return Ok(()), // Skip unparseable files
        };

        let parent_dir = file.parent().unwrap_or(crate_root);

        for item in &parsed.items {
            if let syn::Item::Mod(m) = item {
                // Skip #[cfg(test)] modules
                let is_cfg_test = m.attrs.iter().any(|a| {
                    if a.path().is_ident("cfg") {
                        a.parse_args::<syn::Ident>()
                            .map(|id| id == "test")
                            .unwrap_or(false)
                    } else {
                        false
                    }
                });
                if is_cfg_test {
                    continue;
                }

                let mod_name = m.ident.to_string();

                if m.content.is_some() {
                    // Inline module – no separate file, but register the node
                    let child = node
                        .children
                        .entry(mod_name.clone())
                        .or_insert_with(|| ModuleNode {
                            name: mod_name,
                            children: HashMap::new(),
                            file: Some(file.to_path_buf()),
                        });
                    child.file = Some(file.to_path_buf());
                    continue;
                }

                // External module: resolve to foo.rs or foo/mod.rs
                let mod_file = Self::resolve_mod_file(parent_dir, &mod_name);
                if let Some(ref mod_path) = mod_file {
                    let child = node
                        .children
                        .entry(mod_name.clone())
                        .or_insert_with(|| ModuleNode {
                            name: mod_name,
                            children: HashMap::new(),
                            file: None,
                        });
                    child.file = Some(mod_path.clone());
                    // Recurse into the child module
                    Self::discover_children(mod_path, crate_root, child)?;
                }
            }
        }

        Ok(())
    }

    /// Resolve `mod foo;` to either `foo.rs` or `foo/mod.rs`.
    fn resolve_mod_file(parent_dir: &Path, mod_name: &str) -> Option<PathBuf> {
        // Try foo.rs first
        let direct = parent_dir.join(format!("{mod_name}.rs"));
        if direct.exists() {
            return Some(direct);
        }
        // Try foo/mod.rs
        let nested = parent_dir.join(mod_name).join("mod.rs");
        if nested.exists() {
            return Some(nested);
        }
        None
    }

    /// Build the lookup maps after the tree is constructed.
    fn build_lookups(&mut self, crate_root: &Path) {
        let mut path_to_files: HashMap<String, Vec<PathBuf>> = HashMap::new();
        let mut file_to_module: HashMap<PathBuf, Vec<String>> = HashMap::new();

        // Register the root file
        if let Some(ref file) = self.root.file {
            file_to_module.insert(file.clone(), vec![]);
        }

        Self::collect_lookups(
            &self.root,
            &[],
            crate_root,
            &mut path_to_files,
            &mut file_to_module,
        );

        self.path_to_files = path_to_files;
        self.file_to_module = file_to_module;
    }

    fn collect_lookups(
        node: &ModuleNode,
        prefix: &[String],
        crate_root: &Path,
        path_to_files: &mut HashMap<String, Vec<PathBuf>>,
        file_to_module: &mut HashMap<PathBuf, Vec<String>>,
    ) {
        for (name, child) in &node.children {
            let mut mod_path = prefix.to_vec();
            mod_path.push(name.clone());
            let key = mod_path.join("::");

            if let Some(ref file) = child.file {
                path_to_files
                    .entry(key.clone())
                    .or_default()
                    .push(file.clone());
                file_to_module.insert(file.clone(), mod_path.clone());
            }

            // Also collect all files in subdirectories for this module
            if let Some(ref file) = child.file {
                let mod_dir = if file.file_name().is_some_and(|f| f == "mod.rs") {
                    file.parent().map(|p| p.to_path_buf())
                } else {
                    // For foo.rs, check if foo/ directory exists
                    let dir = file.with_extension("");
                    if dir.is_dir() {
                        Some(dir)
                    } else {
                        None
                    }
                };

                if let Some(dir) = mod_dir {
                    Self::collect_dir_files(&dir, &key, path_to_files, file_to_module, &mod_path);
                }
            }

            Self::collect_lookups(child, &mod_path, crate_root, path_to_files, file_to_module);
        }
    }

    /// Collect all `.rs` files in a directory tree, registering them under the given module key.
    fn collect_dir_files(
        dir: &Path,
        mod_key: &str,
        path_to_files: &mut HashMap<String, Vec<PathBuf>>,
        file_to_module: &mut HashMap<PathBuf, Vec<String>>,
        mod_path: &[String],
    ) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let sub_key = format!("{}::{}", mod_key, path.file_name().unwrap_or_default().to_string_lossy());
                Self::collect_dir_files(&path, &sub_key, path_to_files, file_to_module, &{
                    let mut p = mod_path.to_vec();
                    p.push(path.file_name().unwrap_or_default().to_string_lossy().to_string());
                    p
                });
                // Also register sub-files under the parent module
                if let Some(sub_files) = path_to_files.get(&sub_key) {
                    let sub_files_clone = sub_files.clone();
                    path_to_files
                        .entry(mod_key.to_string())
                        .or_default()
                        .extend(sub_files_clone);
                }
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                path_to_files
                    .entry(mod_key.to_string())
                    .or_default()
                    .push(path.clone());
                if !file_to_module.contains_key(&path) {
                    file_to_module.insert(path, mod_path.to_vec());
                }
            }
        }
    }

    /// Get all files belonging to a module (including nested submodules).
    ///
    /// Accepts both simple names ("storage") and qualified paths ("storage::dgraph").
    pub fn resolve_module(&self, name: &str) -> Option<&Vec<PathBuf>> {
        self.path_to_files.get(name)
    }

    /// Check if a file is under a given module subtree.
    pub fn file_is_under_module(&self, file: &Path, module: &str) -> bool {
        if let Some(files) = self.path_to_files.get(module) {
            if files.iter().any(|f| f == file) {
                return true;
            }
        }

        // Also check by file-to-module mapping
        if let Some(mod_path) = self.file_to_module.get(file) {
            let mod_str = mod_path.join("::");
            return mod_str == module || mod_str.starts_with(&format!("{module}::"));
        }

        // Fallback: check if the relative path starts with the module directory name
        // This handles files discovered by directory walking but not in the module tree
        false
    }

    /// Get the module path for a file.
    pub fn module_of_file(&self, file: &Path) -> Option<&Vec<String>> {
        self.file_to_module.get(file)
    }

    /// Get all files tracked by the module tree.
    pub fn all_files(&self) -> impl Iterator<Item = &PathBuf> {
        self.file_to_module.keys()
    }

    /// Get all module paths in the tree (e.g. "storage", "storage::dgraph").
    pub fn module_paths(&self) -> impl Iterator<Item = &String> {
        self.path_to_files.keys()
    }

    /// Check if a file is under a module using directory-based heuristic.
    /// This is used as a fallback for files not in the module tree
    /// (e.g. when scanning a codebase that isn't the crate being indexed).
    pub fn file_is_under_directory(&self, file: &Path, codebase: &Path, dir_name: &str) -> bool {
        if let Ok(rel) = file.strip_prefix(codebase) {
            let rel_str = rel.to_string_lossy();
            // Use path component check to avoid matching "storage_backup" for "storage"
            rel.components().next().is_some_and(|c| {
                c.as_os_str().to_string_lossy() == dir_name
            }) || rel_str.starts_with(&format!("{dir_name}/"))
        } else {
            false
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn setup_tempdir() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();

        // Create lib.rs with mod declarations
        std::fs::write(
            tmp.path().join("lib.rs"),
            "mod storage;\nmod services;\n",
        )
        .unwrap();

        // Create storage/mod.rs with sub-modules
        std::fs::create_dir(tmp.path().join("storage")).unwrap();
        std::fs::write(
            tmp.path().join("storage/mod.rs"),
            "mod dgraph;\npub mod coordinator;\n",
        )
        .unwrap();

        // Create storage/dgraph/mod.rs
        std::fs::create_dir(tmp.path().join("storage/dgraph")).unwrap();
        std::fs::write(
            tmp.path().join("storage/dgraph/mod.rs"),
            "pub struct DgraphClient;\n",
        )
        .unwrap();

        // Create storage/coordinator.rs
        std::fs::write(
            tmp.path().join("storage/coordinator.rs"),
            "pub struct StorageCoordinator;\n",
        )
        .unwrap();

        // Create services.rs
        std::fs::write(
            tmp.path().join("services.rs"),
            "pub struct ServiceManager;\n",
        )
        .unwrap();

        tmp
    }

    #[test]
    fn builds_nested_module_tree() {
        let tmp = setup_tempdir();
        let tree = ModuleTree::build(tmp.path()).unwrap();

        assert!(tree.resolve_module("storage").is_some());
        assert!(tree.resolve_module("services").is_some());
    }

    #[test]
    fn resolves_storage_to_files() {
        let tmp = setup_tempdir();
        let tree = ModuleTree::build(tmp.path()).unwrap();

        let files = tree.resolve_module("storage").unwrap();
        assert!(!files.is_empty());
    }

    #[test]
    fn resolves_nested_module() {
        let tmp = setup_tempdir();
        let tree = ModuleTree::build(tmp.path()).unwrap();

        let files = tree.resolve_module("storage::dgraph");
        assert!(files.is_some());
    }

    #[test]
    fn file_is_under_module_works() {
        let tmp = setup_tempdir();
        let tree = ModuleTree::build(tmp.path()).unwrap();

        let dgraph_mod = tmp.path().join("storage/dgraph/mod.rs");
        assert!(tree.file_is_under_module(&dgraph_mod, "storage"));
    }

    #[test]
    fn handles_mod_rs_and_foo_rs_styles() {
        let tmp = setup_tempdir();
        let tree = ModuleTree::build(tmp.path()).unwrap();

        // services.rs style
        let services_file = tmp.path().join("services.rs");
        assert!(tree.file_is_under_module(&services_file, "services"));

        // storage/mod.rs style
        let storage_mod = tmp.path().join("storage/mod.rs");
        assert!(tree.file_is_under_module(&storage_mod, "storage"));
    }

    #[test]
    fn directory_based_check() {
        let tmp = setup_tempdir();
        let tree = ModuleTree::build(tmp.path()).unwrap();

        let path = tmp.path().join("storage/dgraph/client.rs");
        assert!(tree.file_is_under_directory(&path, tmp.path(), "storage"));
        assert!(!tree.file_is_under_directory(&path, tmp.path(), "services"));
        // Should NOT match "storage_backup" for "storage"
        let backup_path = tmp.path().join("storage_backup/file.rs");
        assert!(!tree.file_is_under_directory(&backup_path, tmp.path(), "storage"));
    }
}
