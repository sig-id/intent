use super::super::index::use_resolver;
use super::super::index::CrateIndex;
use super::super::{ConstraintResult, Violation};

/// Check `[from_modules] must_depend_on [target_entities]`.
///
/// For each from-module, verify at least one file has an import, type ref,
/// or call ref to at least one target entity. If no dependency found, report
/// a violation.
pub fn check(
    constraint_name: &str,
    concern_name: &str,
    from_modules: &[String],
    target_entities: &[String],
    index: &CrateIndex,
) -> ConstraintResult {
    let mut violations = Vec::new();

    for module in from_modules {
        if has_dependency_on_any(module, target_entities, index) {
            continue;
        }

        violations.push(Violation {
            file: index.codebase_root.clone(),
            line: 0,
            content: format!(
                "module '{}' does not depend on any of [{}]",
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

fn has_dependency_on_any(module: &str, targets: &[String], index: &CrateIndex) -> bool {
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
                if is_module_name(entity)
                    && use_resolver::import_depends_on_module(import, entity)
                {
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

fn is_module_name(name: &str) -> bool {
    name.chars()
        .next()
        .is_some_and(|c| c.is_ascii_lowercase())
}
