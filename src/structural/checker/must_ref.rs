use super::super::index::use_resolver;
use super::super::index::CrateIndex;
use super::super::{ConstraintResult, Violation};

/// Check `[from_modules] must_reference [target_entities]`.
///
/// For each from-module, verify at least one file references at least one
/// target entity (via import, type ref, or call ref). If no reference found,
/// report a violation.
pub fn check(
    constraint_name: &str,
    concern_name: &str,
    from_modules: &[String],
    target_entities: &[String],
    index: &CrateIndex,
) -> ConstraintResult {
    let mut violations = Vec::new();

    for module in from_modules {
        if has_reference_to_any(module, target_entities, index) {
            continue;
        }

        violations.push(Violation {
            file: index.codebase_root.clone(),
            line: 0,
            content: format!(
                "module '{}' does not reference any of [{}]",
                module,
                target_entities.join(", ")
            ),
            entity: module.clone(),
        });
    }

    ConstraintResult {
        name: constraint_name.to_string(),
        concern: concern_name.to_string(),
        passed: violations.is_empty(),
        violations,
    }
}

fn has_reference_to_any(module: &str, targets: &[String], index: &CrateIndex) -> bool {
    let module_slice = &[module.to_string()];

    for (path, analysis) in &index.files {
        if !index.file_is_in_modules(path, module_slice) {
            continue;
        }

        // Check imports
        for import in &analysis.imports {
            for entity in targets {
                if use_resolver::import_brings_entity(import, entity) {
                    return true;
                }
            }
        }

        // Check type references
        for type_ref in &analysis.type_refs {
            if targets.contains(&type_ref.name) {
                return true;
            }
        }

        // Check call references
        for call_ref in &analysis.call_refs {
            if targets.contains(&call_ref.receiver) {
                return true;
            }
        }
    }

    false
}
