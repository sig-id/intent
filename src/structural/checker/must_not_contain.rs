use super::super::index::CrateIndex;
use super::super::{ConstraintResult, Violation};

/// Check `!container.contains(entities)`.
///
/// For each target entity, verify it does NOT appear in any file that belongs
/// to the container module subtree.
pub fn check(
    constraint_name: &str,
    concern_name: &str,
    container_modules: &[String],
    target_entities: &[String],
    index: &CrateIndex,
) -> ConstraintResult {
    let mut violations = Vec::new();

    for entity in target_entities {
        if let Some(refs) = index.entity_refs.get(entity) {
            for (file, line) in refs {
                if index.file_is_in_modules(file, container_modules) {
                    violations.push(Violation {
                        file: file.clone(),
                        line: *line,
                        content: format!(
                            "'{}' found in [{}]",
                            entity,
                            container_modules.join(", ")
                        ),
                        entity: entity.clone(),
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
