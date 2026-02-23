use super::super::index::CrateIndex;
use super::super::{ConstraintResult, Violation};

/// Check `!entity.implements(TraitName)`.
///
/// For each entity, verify that it does NOT implement the given trait.
pub fn check(
    constraint_name: &str,
    concern_name: &str,
    entities: &[String],
    trait_name: &str,
    index: &CrateIndex,
) -> ConstraintResult {
    let mut violations = Vec::new();

    for type_name in entities {
        let key = (trait_name.to_string(), type_name.to_string());
        if index.trait_impls.contains_key(&key) {
            violations.push(Violation {
                file: index.codebase_root.clone(),
                line: 0,
                content: format!("`impl {trait_name} for {type_name}` found but should not exist"),
                entity: type_name.to_string(),
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
