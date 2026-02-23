use super::super::index::CrateIndex;
use super::super::{ConstraintResult, Violation};
use super::must_depend_transitively::is_transitively_reachable;

/// Check `![from_modules].depends_transitively([target_entities])`.
///
/// Violation if a transitive dependency path exists from any from-module
/// to any target entity.
pub fn check(
    constraint_name: &str,
    concern_name: &str,
    from_modules: &[String],
    target_entities: &[String],
    index: &CrateIndex,
) -> ConstraintResult {
    let mut violations = Vec::new();

    for module in from_modules {
        if is_transitively_reachable(module, target_entities, index) {
            violations.push(Violation {
                file: index.codebase_root.clone(),
                line: 0,
                content: format!(
                    "module '{}' transitively depends on one of [{}] (forbidden)",
                    module,
                    target_entities.join(", ")
                ),
                entity: module.clone(),
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
