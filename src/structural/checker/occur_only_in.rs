use super::super::index::CrateIndex;
use super::super::{ConstraintResult, Violation};

/// Check `Pattern occur_only_in [modules]`.
///
/// Look up all references to the entity. Report violations for any reference
/// outside the allowed modules.
pub fn check(
    constraint_name: &str,
    concern_name: &str,
    pattern: &str,
    allowed_modules: &[String],
    index: &CrateIndex,
) -> ConstraintResult {
    let mut violations = Vec::new();

    // Check entity_refs index for direct type/call references
    if let Some(refs) = index.entity_refs.get(pattern) {
        for (file, line) in refs {
            if !index.file_is_in_modules(file, allowed_modules) {
                violations.push(Violation {
                    file: file.clone(),
                    line: *line,
                    content: pattern.to_string(),
                    entity: pattern.to_string(),
                });
            }
        }
    }

    // Also check imports that bring the entity into scope
    for (path, analysis) in &index.files {
        if index.file_is_in_modules(path, allowed_modules) {
            continue;
        }
        for import in &analysis.imports {
            if super::super::index::use_resolver::import_brings_entity(import, pattern) {
                if !violations.iter().any(|v| v.file == *path && v.line == import.line) {
                    violations.push(Violation {
                        file: path.clone(),
                        line: import.line,
                        content: format!("use {}", import.segments.join("::")),
                        entity: pattern.to_string(),
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
