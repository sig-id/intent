use std::collections::HashSet;
use std::path::Path;

use super::super::index::use_resolver;
use super::super::index::CrateIndex;
use super::super::{ConstraintResult, Violation};

/// Check `[from_modules] must_not depend_on [target_entities]`.
///
/// For each file in `from` modules: check if any `use` import resolves to a module
/// containing a target entity, or if a target entity name is directly imported.
pub fn check(
    constraint_name: &str,
    concern_name: &str,
    from_modules: &[String],
    target_entities: &[String],
    index: &CrateIndex,
) -> ConstraintResult {
    let mut violations = Vec::new();

    // Convert targets to HashSet for O(1) lookups
    let target_set: HashSet<&str> = target_entities.iter().map(|s| s.as_str()).collect();

    // Determine which modules the target entities belong to
    // (used for checking module-level dependency via imports)
    let target_modules: Vec<String> = target_entities
        .iter()
        .filter(|e| is_module_name(e))
        .cloned()
        .collect();

    for (path, analysis) in &index.rust_files {
        if !is_in_from_modules(path, from_modules, index) {
            continue;
        }

        // Check imports
        for import in &analysis.imports {
            // Check if import brings a target entity into scope
            for entity in &target_set {
                if use_resolver::import_brings_entity(import, entity) {
                    violations.push(Violation {
                        file: path.clone(),
                        line: import.line,
                        content: format!("use {}", import.segments.join("::")),
                        entity: entity.to_string(),
                    });
                }
            }

            // Check if import depends on a target module
            for module in &target_modules {
                if use_resolver::import_depends_on_module(import, module) {
                    violations.push(Violation {
                        file: path.clone(),
                        line: import.line,
                        content: format!("use {}", import.segments.join("::")),
                        entity: module.clone(),
                    });
                }
            }
        }

        // Check type references (qualified paths like crate::storage::DgraphClient)
        for type_ref in &analysis.type_refs {
            if target_set.contains(type_ref.name.as_str()) {
                // Avoid duplicate with import-based detection
                if !violations.iter().any(|v| v.file == *path && v.line == type_ref.line && v.entity == type_ref.name) {
                    violations.push(Violation {
                        file: path.clone(),
                        line: type_ref.line,
                        content: type_ref
                            .qualified
                            .as_ref()
                            .map(|q| q.join("::"))
                            .unwrap_or_else(|| type_ref.name.clone()),
                        entity: type_ref.name.clone(),
                    });
                }
            }
        }

        // Check call references (e.g. DgraphClient::new())
        for call_ref in &analysis.call_refs {
            if target_set.contains(call_ref.receiver.as_str()) {
                if !violations.iter().any(|v| v.file == *path && v.line == call_ref.line && v.entity == call_ref.receiver) {
                    violations.push(Violation {
                        file: path.clone(),
                        line: call_ref.line,
                        content: format!("{}::{}", call_ref.receiver, call_ref.method),
                        entity: call_ref.receiver.clone(),
                    });
                }
            }
        }
    }

    ConstraintResult::structural(
        constraint_name.to_string(),
        concern_name.to_string(),
        violations.is_empty(),
        violations,
    )
}

fn is_in_from_modules(path: &Path, from_modules: &[String], index: &CrateIndex) -> bool {
    index.file_is_in_modules(path, from_modules)
}

fn is_module_name(name: &str) -> bool {
    name.chars()
        .next()
        .is_some_and(|c| c.is_ascii_lowercase())
}
