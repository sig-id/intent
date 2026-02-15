use super::file_analysis::UseImport;

/// Resolve a `use` import to the module it originates from.
///
/// Returns the module name (e.g. "storage") that the import depends on.
/// For `use crate::storage::DgraphClient`, returns "storage".
/// For `use super::*`, returns None (can't resolve without context).
pub fn resolve_import_to_module(import: &UseImport) -> Option<String> {
    let segs = &import.segments;

    if segs.is_empty() {
        return None;
    }

    // `use crate::storage::DgraphClient` -> module is "storage"
    // `use crate::storage::dgraph::Client` -> module is "storage"
    if segs[0] == "crate" && segs.len() >= 2 {
        return Some(segs[1].clone());
    }

    // `use super::foo` -> can't resolve without knowing the current module
    if segs[0] == "super" || segs[0] == "self" {
        return None;
    }

    // External crate imports (e.g. `use std::...`, `use anyhow::...`) are not
    // internal dependencies
    None
}

/// Check if a `use` import brings a specific entity name into scope.
///
/// For `use crate::storage::DgraphClient`, entity "DgraphClient" -> true.
/// For `use crate::storage::*`, any entity -> true (conservative).
/// For external crate globs (`use contracts::*`), returns false.
/// For glob imports that don't target the entity's module, returns false.
pub fn import_brings_entity(import: &UseImport, entity: &str) -> bool {
    if import.is_glob {
        // Glob imports: only match if they start with `crate` (internal).
        // We don't try to resolve what the glob actually exports —
        // the caller should also check type_refs from the FileAnalysis
        // to detect actual usage.
        return false;
    }

    // Check if the last segment matches (or the alias)
    if let Some(ref alias) = import.alias {
        if alias == entity {
            return true;
        }
    }

    import
        .segments
        .last()
        .is_some_and(|last| last == entity)
}

/// Check if a `use` import targets a specific module (directly or transitively).
///
/// For `use crate::storage::DgraphClient`, module "storage" -> true.
/// For `use crate::storage::dgraph::Client`, module "storage" -> true.
/// For `use crate::storage::dgraph::Client`, module "storage::dgraph" -> true.
pub fn import_depends_on_module(import: &UseImport, module: &str) -> bool {
    let segs = &import.segments;
    if segs.is_empty() || segs[0] != "crate" {
        return false;
    }

    // Build the module path from segments (skip "crate" prefix)
    let path_segs = &segs[1..];
    let module_parts: Vec<&str> = module.split("::").collect();

    // Check if the import path starts with the module path
    if path_segs.len() >= module_parts.len() {
        path_segs
            .iter()
            .zip(module_parts.iter())
            .all(|(a, b)| a == b)
    } else {
        false
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn make_import(segments: &[&str], is_glob: bool) -> UseImport {
        UseImport {
            segments: segments.iter().map(|s| s.to_string()).collect(),
            alias: None,
            is_glob,
            line: 1,
        }
    }

    #[test]
    fn resolve_crate_import() {
        let imp = make_import(&["crate", "storage", "DgraphClient"], false);
        assert_eq!(resolve_import_to_module(&imp), Some("storage".into()));
    }

    #[test]
    fn resolve_deep_crate_import() {
        let imp = make_import(&["crate", "storage", "dgraph", "Client"], false);
        assert_eq!(resolve_import_to_module(&imp), Some("storage".into()));
    }

    #[test]
    fn resolve_super_import_returns_none() {
        let imp = make_import(&["super", "foo"], false);
        assert_eq!(resolve_import_to_module(&imp), None);
    }

    #[test]
    fn resolve_external_crate_returns_none() {
        let imp = make_import(&["std", "collections", "HashMap"], false);
        assert_eq!(resolve_import_to_module(&imp), None);
    }

    #[test]
    fn import_brings_named_entity() {
        let imp = make_import(&["crate", "storage", "DgraphClient"], false);
        assert!(import_brings_entity(&imp, "DgraphClient"));
        assert!(!import_brings_entity(&imp, "MilvusClient"));
    }

    #[test]
    fn glob_imports_do_not_match_entities() {
        // Glob imports are not matched by entity name — actual usage is
        // detected via type_refs from the syn visitor instead.
        let imp = make_import(&["crate", "storage"], true);
        assert!(!import_brings_entity(&imp, "DgraphClient"));

        let imp = make_import(&["contracts"], true);
        assert!(!import_brings_entity(&imp, "DgraphClient"));
    }

    #[test]
    fn import_depends_on_storage_module() {
        let imp = make_import(&["crate", "storage", "DgraphClient"], false);
        assert!(import_depends_on_module(&imp, "storage"));
        assert!(!import_depends_on_module(&imp, "services"));
    }

    #[test]
    fn import_depends_on_nested_module() {
        let imp = make_import(&["crate", "storage", "dgraph", "Client"], false);
        assert!(import_depends_on_module(&imp, "storage"));
        assert!(import_depends_on_module(&imp, "storage::dgraph"));
        assert!(!import_depends_on_module(&imp, "storage::milvus"));
    }
}
