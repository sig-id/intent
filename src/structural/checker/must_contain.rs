use std::path::PathBuf;

use super::super::index::CrateIndex;
use super::super::{ConstraintResult, Violation};

/// Check `container.contains(entities)`.
///
/// For each target entity, verify it appears (as a type/call reference) in at
/// least one file that belongs to the container module subtree.  This uses the
/// same module-membership check (`file_is_in_modules`) that powers
/// `must_depend` / `must_not_depend`.
pub fn check(
    constraint_name: &str,
    concern_name: &str,
    container_modules: &[String],
    target_entities: &[String],
    index: &CrateIndex,
) -> ConstraintResult {
    let mut violations = Vec::new();

    for entity in target_entities {
        let found = index.entity_refs.get(entity).is_some_and(|refs| {
            refs.iter()
                .any(|(file, _)| index.file_is_in_modules(file, container_modules))
        });

        if !found {
            violations.push(Violation {
                file: PathBuf::from("<constraint>"),
                line: 0,
                content: format!(
                    "'{}' not found in [{}]",
                    entity,
                    container_modules.join(", ")
                ),
                entity: entity.clone(),
            });
        }
    }

    ConstraintResult::structural(
        constraint_name.to_string(),
        concern_name.to_string(),
        violations.is_empty(),
        violations,
    )
}
