use super::super::index::CrateIndex;
use super::super::{ConstraintResult, Violation};

/// Check `only [accessors] accesses [entities]`.
///
/// Scan files NOT in accessor modules. If `within` is set, restrict scanning to those
/// directories only. Report any reference to target entities from non-accessor files.
pub fn check(
    constraint_name: &str,
    concern_name: &str,
    accessors: &[String],
    target_entities: &[String],
    index: &CrateIndex,
    within: Option<&[String]>,
) -> ConstraintResult {
    let mut violations = Vec::new();

    for (path, analysis) in &index.rust_files {
        // If `within` is set, only check files under those directories
        if let Some(within_dirs) = within {
            if !index.file_is_in_modules(path, &within_dirs.iter().map(|s| s.to_string()).collect::<Vec<_>>()) {
                continue;
            }
        }

        // Skip files in accessor modules (they're allowed to access)
        if index.file_is_in_modules(path, accessors) {
            continue;
        }

        // Check imports
        for import in &analysis.imports {
            for entity in target_entities {
                if super::super::index::use_resolver::import_brings_entity(import, entity) {
                    violations.push(Violation {
                        file: path.clone(),
                        line: import.line,
                        content: format!("use {}", import.segments.join("::")),
                        entity: entity.clone(),
                    });
                }
            }
        }

        // Check type references
        for type_ref in &analysis.type_refs {
            if target_entities.contains(&type_ref.name) {
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

        // Check call references
        for call_ref in &analysis.call_refs {
            if target_entities.contains(&call_ref.receiver) {
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

    ConstraintResult {
        name: constraint_name.to_string(),
        concern: concern_name.to_string(),
        passed: violations.is_empty(),
        violations,
    }
}
