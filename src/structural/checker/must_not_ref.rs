use std::path::Path;

use super::super::index::use_resolver;
use super::super::index::CrateIndex;
use super::super::{ConstraintResult, Violation};

/// Check `[from_modules] must_not reference [target_entities]`.
///
/// For each file in `from` modules: check if any TypeRef, CallRef, or use import
/// matches a target entity name.
pub fn check(
    constraint_name: &str,
    concern_name: &str,
    from_modules: &[String],
    target_entities: &[String],
    index: &CrateIndex,
) -> ConstraintResult {
    let mut violations = Vec::new();

    // Check Rust files
    for (path, analysis) in &index.rust_files {
        if !index.file_is_in_modules(path, from_modules) {
            continue;
        }

        // Check imports that bring target entities into scope
        for import in &analysis.imports {
            for entity in target_entities {
                if use_resolver::import_brings_entity(import, entity) {
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
                if !has_violation_at(&violations, path, type_ref.line, &type_ref.name) {
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
                if !has_violation_at(&violations, path, call_ref.line, &call_ref.receiver) {
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

    // Check TypeScript files
    for (path, analysis) in &index.ts_files {
        if !index.file_is_in_modules(path, from_modules) {
            continue;
        }

        // Check imports
        for import in &analysis.imports {
            for entity in target_entities {
                if import.names.contains(entity) {
                    if !has_violation_at(&violations, path, import.line, entity) {
                        violations.push(Violation {
                            file: path.clone(),
                            line: import.line,
                            content: format!(
                                "import {{ {} }} from '{}'",
                                import.names.join(", "),
                                import.source
                            ),
                            entity: entity.clone(),
                        });
                    }
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

fn has_violation_at(violations: &[Violation], path: &Path, line: usize, entity: &str) -> bool {
    violations
        .iter()
        .any(|v| v.file == path && v.line == line && v.entity == entity)
}
